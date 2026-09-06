use chrono::{Local, NaiveDate};
use file_rotate::compression::Compression;
use file_rotate::suffix::{AppendTimestamp, FileLimit, SuffixScheme, TimestampSuffix};
use file_rotate::{ContentLimit, FileRotate};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tracing_appender::non_blocking::{ErrorCounter, NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::filter::{dynamic_filter_fn, LevelFilter};
use tracing_subscriber::fmt::{format::Writer, time::FormatTime};
use tracing_subscriber::prelude::*;
use zip::write::SimpleFileOptions;

const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_ARCHIVES: usize = 9;
const RETENTION_DAYS: i64 = 7;
const DETAIL_DURATION: Duration = Duration::from_secs(10 * 60);
const REPEAT_INTERVAL: Duration = Duration::from_secs(30);
const MAX_REPEAT_KEYS: usize = 128;
const FLUSH_MARKER: &str = "\0lol-stats-flush:";

static INITIALIZED: OnceLock<Result<(), String>> = OnceLock::new();
static LOGGER: OnceLock<Logger> = OnceLock::new();

struct Logger {
    directory: PathBuf,
    started: Instant,
    started_at: String,
    detail_until: AtomicU64,
    writer: NonBlocking,
    guard: Mutex<Option<WorkerGuard>>,
    dropped: ErrorCounter,
    output: Arc<Mutex<LogOutput>>,
    health: Arc<WriterHealth>,
    next_flush: AtomicU64,
    export_lock: Mutex<()>,
}

#[derive(Default)]
struct WriterHealth {
    flushed: AtomicU64,
    error: Mutex<Option<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogStatus {
    pub directory: String,
    pub detailed_enabled: bool,
    pub detailed_until_ms: Option<i64>,
    pub dropped_lines: usize,
    pub write_error: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FrontendLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrontendLogEntry {
    level: FrontendLogLevel,
    source: String,
    message: String,
    command: Option<String>,
    request_id: Option<String>,
    elapsed_ms: Option<u64>,
}

struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result {
        write!(writer, "[{}]", Local::now().format("%Y-%m-%d %H:%M:%S%.3f"))
    }
}

// 轮转与删除交给 file-rotate，同时限制归档数量和保存天数。
struct RetainedSuffix(AppendTimestamp);

impl SuffixScheme for RetainedSuffix {
    type Repr = TimestampSuffix;

    fn rotate_file(
        &mut self,
        basepath: &Path,
        newest_suffix: Option<&Self::Repr>,
        suffix: &Option<Self::Repr>,
    ) -> io::Result<Self::Repr> {
        self.0.rotate_file(basepath, newest_suffix, suffix)
    }

    fn parse(&self, suffix: &str) -> Option<Self::Repr> {
        self.0.parse(suffix)
    }

    fn too_old(&self, suffix: &Self::Repr, file_number: usize) -> bool {
        let cutoff = (Local::now() - chrono::Duration::days(RETENTION_DAYS))
            .format(self.0.format)
            .to_string();
        self.0.too_old(suffix, file_number) || suffix.timestamp < cutoff
    }
}

struct RepeatEntry {
    last_written: Instant,
    suppressed: u64,
}

struct LogOutput {
    file: FileRotate<RetainedSuffix>,
    directory: PathBuf,
    bytes: usize,
    day: NaiveDate,
    redactor: Redactor,
    repeats: HashMap<String, RepeatEntry>,
    health: Arc<WriterHealth>,
}

impl LogOutput {
    fn append(&mut self, line: &str) -> io::Result<()> {
        let today = Local::now().date_naive();
        if today != self.day || self.bytes + line.len() > MAX_FILE_BYTES {
            self.file.rotate()?;
            self.bytes = 0;
            self.day = today;
        }
        self.file.write_all(line.as_bytes())?;
        self.bytes += line.len();
        Ok(())
    }

    fn flush_repeats(&mut self, all: bool) -> io::Result<()> {
        let mut summaries = Vec::new();
        for (key, entry) in &mut self.repeats {
            if entry.suppressed > 0 && (all || entry.last_written.elapsed() >= REPEAT_INTERVAL) {
                summaries.push(format!(
                    "[{}]  INFO lol_stats::log: 合并重复日志 {} 次 | {}\n",
                    Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                    entry.suppressed,
                    key,
                ));
                entry.suppressed = 0;
            }
        }
        for summary in summaries {
            self.append(&summary)?;
        }
        Ok(())
    }

    fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        let raw = String::from_utf8_lossy(bytes);
        // appender 的 flush 本身不等待队列，导出/退出用同一队列中的屏障确认刷盘。
        if let Some(sequence) = raw
            .strip_prefix(FLUSH_MARKER)
            .and_then(|value| value.parse::<u64>().ok())
        {
            self.flush_repeats(true)?;
            self.file.flush()?;
            self.health.flushed.fetch_max(sequence, Ordering::Release);
            return Ok(());
        }

        self.flush_repeats(false)?;
        let mut line = self.redactor.clean(&raw);
        let body = line
            .split_once(']')
            .map(|(_, body)| body.trim_start())
            .unwrap_or(&line);
        if body.starts_with("WARN ") || body.starts_with("ERROR ") {
            let key = self.redactor.repeat_key(&line);
            if let Some(entry) = self.repeats.get_mut(&key) {
                if entry.last_written.elapsed() < REPEAT_INTERVAL {
                    entry.suppressed += 1;
                    return Ok(());
                }
                entry.last_written = Instant::now();
            } else {
                if self.repeats.len() >= MAX_REPEAT_KEYS {
                    self.flush_repeats(true)?;
                    self.repeats.clear();
                }
                self.repeats.insert(
                    key,
                    RepeatEntry {
                        last_written: Instant::now(),
                        suppressed: 0,
                    },
                );
            }
        }
        line.push('\n');
        self.append(&line)
    }
}

struct BackgroundWriter(Arc<Mutex<LogOutput>>);

impl Write for BackgroundWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut output = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Err(error) = output.record(bytes) {
            let message = output.redactor.clean(&error.to_string());
            *output
                .health
                .error
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(message);
        }
        // 磁盘异常由设置页和诊断信息呈现，不能让业务线程等待磁盘恢复。
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut output = self.0.lock().unwrap_or_else(|error| error.into_inner());
        let result = output.file.flush();
        if let Err(ref error) = result {
            let message = output.redactor.clean(&error.to_string());
            *output
                .health
                .error
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(message);
        }
        result
    }
}

struct Redactor {
    credentials: Regex,
    secret_fields: Regex,
    urls: Regex,
    identifiers: Regex,
    uuid: Regex,
    riot_id: Regex,
    volatile_fields: Regex,
    user_directory: Option<Regex>,
}

impl Redactor {
    fn new() -> Self {
        let user_directory = std::env::var("USERPROFILE")
            .ok()
            .filter(|path| !path.is_empty())
            .map(|path| {
                let native = regex::escape(&path);
                let slash = regex::escape(&path.replace('\\', "/"));
                Regex::new(&format!("(?i){native}|{slash}")).expect("user directory pattern")
            });
        Self {
            credentials: Regex::new(r"(?i)\b(Basic|Bearer)\s+[A-Za-z0-9_~+./=-]+").unwrap(),
            secret_fields: Regex::new(r#"(?i)(["']?(?:authorization|proxy-authorization|cookie|set-cookie|(?:access|refresh|id|auth)[_-]?token|token|password|passwd|secret|api[_-]?key|--remoting-auth-token|--riotclient-auth-token)["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,;]+)"#).unwrap(),
            urls: Regex::new(r#"https?://[^\s"'<>]+"#).unwrap(),
            identifiers: Regex::new(r#"(?i)(\b(?:puuid|game_?name|display_?name|summoner_?name|summoner_?id|tag_?line|riot_?id|player_?id)\b["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,;]+)"#).unwrap(),
            uuid: Regex::new(r"(?i)\b[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\b").unwrap(),
            riot_id: Regex::new(r"[^\s\[\]()=,：:]+[#＃][\p{L}\p{N}]{3,8}\b").unwrap(),
            volatile_fields: Regex::new(r"\s+(?:request_id|elapsed_ms)=[^\s]+").unwrap(),
            user_directory,
        }
    }

    fn clean(&self, message: &str) -> String {
        let message = self.credentials.replace_all(message, "$1 [redacted]");
        let message = self.secret_fields.replace_all(&message, "$1[redacted]");
        let message = self
            .urls
            .replace_all(&message, |captures: &regex::Captures<'_>| {
                let original = &captures[0];
                let Ok(mut url) = reqwest::Url::parse(original) else {
                    return "[invalid-url]".to_string();
                };
                let had_query = url.query().is_some();
                let _ = url.set_username("");
                let _ = url.set_password(None);
                url.set_query(None);
                url.set_fragment(None);
                format!("{url}{}", if had_query { "?[redacted]" } else { "" })
            });
        let message = self.identifiers.replace_all(&message, "$1[player]");
        let message = self.uuid.replace_all(&message, "[player-id]");
        let message = self.riot_id.replace_all(&message, "[riot-id]");
        let message = match &self.user_directory {
            Some(pattern) => pattern.replace_all(&message, "<user>").into_owned(),
            None => message.into_owned(),
        };
        let mut clean = message.trim().replace(['\r', '\n'], " | ");
        clean.retain(|character| !character.is_control());
        truncate(&mut clean, MAX_MESSAGE_BYTES);
        clean
    }

    fn repeat_key(&self, line: &str) -> String {
        let body = line.split_once(']').map(|(_, body)| body).unwrap_or(line);
        self.volatile_fields
            .replace_all(body, "")
            .trim()
            .to_string()
    }
}

fn truncate(message: &mut String, limit: usize) {
    if message.len() <= limit {
        return;
    }
    let mut end = limit.saturating_sub(32);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    message.truncate(end);
    message.push_str("...[已截断]");
}

pub(crate) fn redact_diagnostic_report(report: &str) -> String {
    let redactor = Redactor::new();
    report
        .lines()
        .map(|line| redactor.clean(line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn start_session() -> Result<(), String> {
    INITIALIZED.get_or_init(initialize).clone()
}

fn initialize() -> Result<(), String> {
    let directory = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("lol-stats")
        .join("logs");
    fs::create_dir_all(&directory).map_err(|error| format!("创建日志目录失败：{error}"))?;
    let path = directory.join("app.log");
    if fs::metadata(&path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age.as_secs() > RETENTION_DAYS as u64 * 86400)
    {
        fs::remove_file(&path).map_err(|error| format!("清理过期日志失败：{error}"))?;
    }
    // 先验证目录可写，使常见权限错误正常返回，不影响软件继续启动。
    drop(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| format!("打开日志文件失败：{error}"))?,
    );
    let mut file = FileRotate::new(
        &path,
        RetainedSuffix(AppendTimestamp::default(FileLimit::MaxFiles(MAX_ARCHIVES))),
        ContentLimit::None,
        Compression::None,
        #[cfg(unix)]
        Some(0o600),
    );
    file.rotate()
        .map_err(|error| format!("初始化日志归档失败：{error}"))?;
    let health = Arc::new(WriterHealth::default());
    let output = Arc::new(Mutex::new(LogOutput {
        file,
        directory: directory.clone(),
        bytes: 0,
        day: Local::now().date_naive(),
        redactor: Redactor::new(),
        repeats: HashMap::new(),
        health: health.clone(),
    }));
    let (writer, guard) = NonBlockingBuilder::default()
        .buffered_lines_limit(512)
        .lossy(true)
        .thread_name("lol-stats-log")
        .finish(BackgroundWriter(output.clone()));
    let dropped = writer.error_counter();
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_timer(LocalTime)
        .with_writer(writer.clone())
        .with_filter(
            dynamic_filter_fn(|metadata, _| {
                metadata.target().starts_with("lol_stats")
                    && (*metadata.level() <= tracing::Level::INFO
                        || (*metadata.level() == tracing::Level::DEBUG && detailed_enabled()))
            })
            .with_max_level_hint(LevelFilter::DEBUG),
        );
    tracing_subscriber::registry()
        .with(layer)
        .try_init()
        .map_err(|error| format!("初始化日志订阅器失败：{error}"))?;
    LOGGER
        .set(Logger {
            directory,
            started: Instant::now(),
            started_at: Local::now().to_rfc3339(),
            detail_until: AtomicU64::new(0),
            writer,
            guard: Mutex::new(Some(guard)),
            dropped,
            output,
            health,
            next_flush: AtomicU64::new(0),
            export_lock: Mutex::new(()),
        })
        .map_err(|_| "日志系统已初始化".to_string())?;

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("非文本 panic");
        tracing::error!(target: "lol_stats::panic", location = ?info.location(), "Rust panic: {message}");
        if std::thread::current().name() != Some("lol-stats-log") {
            let _ = flush_pending(Duration::from_secs(1));
        }
        previous_hook(info);
    }));

    tracing::info!(target: "lol_stats::app", version = env!("CARGO_PKG_VERSION"), pid = std::process::id(), "本次运行开始");
    if std::env::var("LOL_STATS_VERBOSE_LOG").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "on"
        )
    }) {
        let _ = set_detailed(true);
    }
    Ok(())
}

pub(crate) fn detailed_enabled() -> bool {
    LOGGER.get().is_some_and(|logger| {
        logger.detail_until.load(Ordering::Relaxed) > logger.started.elapsed().as_millis() as u64
    })
}

pub(crate) fn status() -> Result<LogStatus, String> {
    let logger = LOGGER.get().ok_or_else(|| {
        INITIALIZED
            .get()
            .and_then(|result| result.as_ref().err())
            .cloned()
            .unwrap_or_else(|| "日志系统尚未初始化".to_string())
    })?;
    let remaining = logger
        .detail_until
        .load(Ordering::Relaxed)
        .saturating_sub(logger.started.elapsed().as_millis() as u64);
    Ok(LogStatus {
        directory: logger.directory.to_string_lossy().into_owned(),
        detailed_enabled: remaining > 0,
        detailed_until_ms: (remaining > 0)
            .then(|| chrono::Utc::now().timestamp_millis() + remaining as i64),
        dropped_lines: logger.dropped.dropped_lines(),
        write_error: logger
            .health
            .error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone(),
    })
}

pub(crate) fn set_detailed(enabled: bool) -> Result<LogStatus, String> {
    let logger = LOGGER.get().ok_or("日志系统尚未初始化")?;
    let deadline = if enabled {
        logger.started.elapsed().as_millis() as u64 + DETAIL_DURATION.as_millis() as u64
    } else {
        0
    };
    logger.detail_until.store(deadline, Ordering::Relaxed);
    tracing::info!(target: "lol_stats::log", enabled, "详细日志设置已更新，开启后 10 分钟自动关闭");
    status()
}

pub(crate) fn hex(message: impl AsRef<str>) {
    tracing::info!(target: "lol_stats::hex", "{}", message.as_ref());
}

pub(crate) fn hex_detail(message: impl FnOnce() -> String) {
    tracing::debug!(target: "lol_stats::hex", "{}", message());
}

pub(crate) fn quick_bench(message: impl AsRef<str>) {
    tracing::info!(target: "lol_stats::quick_bench", "{}", message.as_ref());
}

pub(crate) fn write_frontend(entries: Vec<FrontendLogEntry>) -> Result<(), String> {
    if LOGGER.get().is_none() {
        return Err("日志系统尚未初始化".to_string());
    }
    if entries.len() > 64 {
        return Err("单批前端日志不能超过 64 条".to_string());
    }
    for mut entry in entries {
        if matches!(entry.level, FrontendLogLevel::Debug) && !detailed_enabled() {
            continue;
        }
        truncate(&mut entry.message, MAX_MESSAGE_BYTES);
        let source = safe_label(&entry.source);
        let command = entry.command.as_deref().map(safe_label);
        let request_id = entry
            .request_id
            .as_deref()
            .map(safe_label)
            .unwrap_or_default();
        let elapsed_ms = entry.elapsed_ms.unwrap_or(0);
        macro_rules! record {
            ($level:expr) => {
                if let Some(command) = &command {
                    tracing::event!(target: "lol_stats::frontend", $level,
                        source, command, request_id, elapsed_ms, "{}", entry.message);
                } else {
                    tracing::event!(target: "lol_stats::frontend", $level, source, "{}", entry.message);
                }
            };
        }
        match entry.level {
            FrontendLogLevel::Debug => record!(tracing::Level::DEBUG),
            FrontendLogLevel::Info => record!(tracing::Level::INFO),
            FrontendLogLevel::Warn => record!(tracing::Level::WARN),
            FrontendLogLevel::Error => record!(tracing::Level::ERROR),
        }
    }
    Ok(())
}

fn safe_label(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(*ch, '_' | '-' | '.'))
        .take(96)
        .collect()
}

fn flush_pending(timeout: Duration) -> Result<(), String> {
    let logger = LOGGER.get().ok_or("日志系统尚未初始化")?;
    let sequence = logger.next_flush.fetch_add(1, Ordering::Relaxed) + 1;
    let marker = format!("{FLUSH_MARKER}{sequence}");
    let mut writer = logger.writer.clone();
    writer
        .write_all(marker.as_bytes())
        .map_err(|error| error.to_string())?;
    let start = Instant::now();
    while logger.health.flushed.load(Ordering::Acquire) < sequence {
        if start.elapsed() >= timeout {
            return Err("等待日志写入超时，请稍后重试".to_string());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

pub(crate) fn export_diagnostics() -> Result<PathBuf, String> {
    let logger = LOGGER.get().ok_or("日志系统尚未初始化")?;
    let _export = logger
        .export_lock
        .try_lock()
        .map_err(|_| "诊断包正在导出，请稍候".to_string())?;
    tracing::info!(target: "lol_stats::log", "开始导出诊断包");
    flush_pending(Duration::from_secs(3))?;
    let snapshots = {
        let mut output = logger
            .output
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut paths = output.file.log_paths();
        paths.push(output.directory.join("app.log"));
        let mut snapshots = Vec::new();
        for path in paths {
            let file = File::open(&path).map_err(|error| format!("读取日志失败：{error}"))?;
            let length = file.metadata().map_err(|error| error.to_string())?.len();
            let name = path
                .file_name()
                .ok_or("日志文件名无效")?
                .to_string_lossy()
                .into_owned();
            snapshots.push((name, file, length.min(MAX_FILE_BYTES as u64)));
        }
        snapshots
    };
    // 只持有打开的文件句柄，压缩在锁外进行，避免阻塞日志写线程。
    let directory = logger
        .directory
        .parent()
        .ok_or("诊断目录无效")?
        .join("diagnostics");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join("lol-stats-diagnostics.zip");
    let temporary = directory.join("lol-stats-diagnostics.tmp.zip");
    let file = File::create(&temporary).map_err(|error| format!("创建诊断包失败：{error}"))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let result = (|| -> Result<(), String> {
        for (name, file, length) in snapshots {
            archive
                .start_file(format!("logs/{name}"), options)
                .map_err(|error| error.to_string())?;
            io::copy(&mut file.take(length), &mut archive).map_err(|error| error.to_string())?;
        }
        let health = status()?;
        let manifest = serde_json::json!({
            "appVersion": env!("CARGO_PKG_VERSION"),
            "exportedAt": Local::now().to_rfc3339(),
            "sessionStartedAt": &logger.started_at,
            "uptimeSeconds": logger.started.elapsed().as_secs(),
            "os": std::env::consts::OS,
            "osVersion": sysinfo::System::os_version(),
            "arch": std::env::consts::ARCH,
            "debugBuild": cfg!(debug_assertions),
            "detailedLogging": health.detailed_enabled,
            "droppedLogLines": health.dropped_lines,
            "lastWriteError": health.write_error,
        });
        archive
            .start_file("environment.json", options)
            .map_err(|error| error.to_string())?;
        serde_json::to_writer_pretty(&mut archive, &manifest).map_err(|error| error.to_string())?;
        archive.finish().map_err(|error| error.to_string())?;
        Ok(())
    })()
    .and_then(|()| {
        fs::rename(&temporary, &path).map_err(|error| format!("保存诊断包失败：{error}"))
    });
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    tracing::info!(target: "lol_stats::log", "诊断包导出完成");
    Ok(path)
}

pub(crate) fn shutdown() {
    let Some(logger) = LOGGER.get() else { return };
    tracing::info!(target: "lol_stats::app", uptime_seconds = logger.started.elapsed().as_secs(), "本次运行结束");
    let _ = flush_pending(Duration::from_secs(2));
    let guard = logger
        .guard
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .take();
    drop(guard);
}
