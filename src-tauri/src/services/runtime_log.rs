use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_MESSAGE_CHARS: usize = 4096;
const LOG_FILES: [&str; 2] = ["hex-winrate.log", "quick-bench-swap.log"];

static SESSION_INIT: Once = Once::new();
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 初始化本次软件运行的日志文件。
///
/// 当前日志保留上一轮运行的副本，当前文件重新开始；这样既不会丢掉最近一次
/// 的故障上下文，也不会让日志跨天无限增长。
pub(crate) fn start_session() {
    SESSION_INIT.call_once(|| {
        let _guard = WRITE_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        for file_name in LOG_FILES {
            prepare_session_file(file_name);
        }
    });
}

pub(crate) fn hex(message: impl AsRef<str>) {
    write_line("hex-winrate", "hex-winrate.log", "INFO", message.as_ref());
}

pub(crate) fn hex_detail(message: impl AsRef<str>) {
    if verbose_enabled() {
        write_line("hex-winrate", "hex-winrate.log", "DEBUG", message.as_ref());
    }
}

pub(crate) fn quick_bench(message: impl AsRef<str>) {
    write_line(
        "quick-bench-swap",
        "quick-bench-swap.log",
        "INFO",
        message.as_ref(),
    );
}

fn write_line(channel: &str, file_name: &str, level: &str, message: &str) {
    start_session();
    let _guard = WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let Some(directory) = log_directory() else {
        return;
    };
    let _ = fs::create_dir_all(&directory);
    let path = directory.join(file_name);

    rotate_if_needed(&path, &previous_path(&path));

    let clean_message = normalize_message(message);
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("[{timestamp}] [{level}] {clean_message}\n");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
    eprint!("[{channel}] {line}");
}

fn log_directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| Some(std::env::temp_dir()))
        .map(|path| path.join("lol-stats"))
}

fn prepare_session_file(file_name: &str) {
    let Some(directory) = log_directory() else {
        return;
    };
    if fs::create_dir_all(&directory).is_err() {
        return;
    }

    let path = directory.join(file_name);
    let previous = previous_path(&path);
    if path.exists() {
        let _ = fs::remove_file(&previous);
        if fs::rename(&path, &previous).is_err() {
            let _ = OpenOptions::new().write(true).truncate(true).open(&path);
        }
    }

    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
    else {
        return;
    };
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let _ = writeln!(
        file,
        "=== lol-stats {file_name} | 本次运行开始：{timestamp} ==="
    );
}

fn rotate_if_needed(path: &Path, previous: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() < MAX_LOG_BYTES {
        return;
    }

    let _ = fs::remove_file(previous);
    if fs::rename(path, previous).is_err() {
        let _ = OpenOptions::new().write(true).truncate(true).open(path);
    }

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
    {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let _ = writeln!(
            file,
            "=== 日志达到 2 MB，已自动轮转；继续记录：{timestamp} ==="
        );
    }
}

fn previous_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("runtime");
    path.with_file_name(format!("{stem}.previous.log"))
}

fn normalize_message(message: &str) -> String {
    let clean = message.replace('\r', " ").replace('\n', " ");
    if clean.chars().count() <= MAX_MESSAGE_CHARS {
        return clean;
    }

    let mut shortened = clean.chars().take(MAX_MESSAGE_CHARS).collect::<String>();
    shortened.push_str("...[已截断]");
    shortened
}

fn verbose_enabled() -> bool {
    std::env::var("LOL_STATS_VERBOSE_LOG")
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        })
        .unwrap_or(false)
}
