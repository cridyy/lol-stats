use std::fmt::Write as _;
use std::time::{Duration, Instant};

use reqwest::Client;
use serde::Serialize;

use super::aramkit::{self, ChampionAugmentStats, ChampionWinrateTable, DatasetCache};
use super::proxy::{self, AramkitProxySettings, TlsBackend, MAX_REQUEST_RETRIES};
use crate::services::runtime_log::redact_diagnostic_report;

const VAYNE_ID: u32 = 67;
const TEST_BUDGET: Duration = Duration::from_secs(180);
const STEP_INTERVAL: Duration = Duration::from_millis(250);
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AramkitConnectionTest {
    outcome: &'static str,
    elapsed_ms: u64,
    champion_count: usize,
    augment_count: usize,
    base_win_rate: Option<f64>,
    summary: String,
    report: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transport {
    Rustls,
    #[cfg(windows)]
    WindowsTls,
}

const TRANSPORTS: &[Transport] = &[
    Transport::Rustls,
    #[cfg(windows)]
    Transport::WindowsTls,
];

impl Transport {
    fn label(self) -> &'static str {
        match self {
            Self::Rustls => "Rustls 首选 / HTTP/1.1",
            #[cfg(windows)]
            Self::WindowsTls => "Windows 系统 TLS / Schannel / HTTP/1.1",
        }
    }

    fn client(self, settings: &AramkitProxySettings) -> Result<Client, String> {
        let backend = match self {
            Self::Rustls => TlsBackend::Rustls,
            #[cfg(windows)]
            Self::WindowsTls => TlsBackend::Windows,
        };
        proxy::client_for_backend(settings, backend)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Config,
    Rankings,
    Vayne,
}

const STAGES: [Stage; 3] = [Stage::Config, Stage::Rankings, Stage::Vayne];

impl Stage {
    fn label(self) -> &'static str {
        match self {
            Self::Config => "主站数据版本",
            Self::Rankings => "英雄胜率总表",
            Self::Vayne => "薇恩海克斯数据（英雄 ID 67）",
        }
    }

    fn parse(self, body: &str) -> Result<ProbeData, String> {
        match self {
            Self::Config => aramkit::parse_dataset(body).map(ProbeData::Dataset),
            Self::Rankings => aramkit::parse_champion_rankings(body).map(ProbeData::Rankings),
            Self::Vayne => {
                let stats = aramkit::parse_champion_augments(body)?;
                if stats.augment_win_rates.is_empty() {
                    return Err("薇恩 JSON 已下载，但没有有效海克斯胜率数据".to_string());
                }
                if stats.base_win_rate > 1.0
                    || stats.augment_win_rates.values().any(|rate| *rate > 1.0)
                {
                    return Err("薇恩 JSON 的胜率超出 0 到 1 范围，未判定为读取成功".to_string());
                }
                Ok(ProbeData::Vayne(stats))
            }
        }
    }
}

enum ProbeData {
    Dataset(DatasetCache),
    Rankings(ChampionWinrateTable),
    Vayne(ChampionAugmentStats),
}

impl ProbeData {
    fn summary(&self) -> String {
        match self {
            Self::Dataset(dataset) => format!("数据版本：{}", dataset.version()),
            Self::Rankings(table) => format!("有效英雄：{} 个", table.len()),
            Self::Vayne(stats) => {
                let mut samples: Vec<_> = stats.augment_win_rates.iter().collect();
                samples.sort_unstable_by_key(|(id, _)| **id);
                let samples = samples
                    .into_iter()
                    .take(3)
                    .map(|(id, rate)| format!("ID {id} / 胜率 {:.2}%", rate * 100.0))
                    .collect::<Vec<_>>()
                    .join("；");
                format!(
                    "薇恩：{} 个海克斯，基础胜率 {:.2}%\n海克斯样例：{samples}",
                    stats.augment_win_rates.len(),
                    stats.base_win_rate * 100.0
                )
            }
        }
    }
}

struct Step {
    transport: Transport,
    stage: Stage,
    retry_index: usize,
    url: String,
    final_url: String,
    passed: bool,
    skipped: bool,
    retryable: bool,
    phase: &'static str,
    elapsed_ms: u64,
    headers_ms: Option<u64>,
    body_ms: Option<u64>,
    http_status: Option<u16>,
    http_version: String,
    content_type: String,
    bytes: usize,
    detail: String,
}

impl Step {
    fn new(transport: Transport, stage: Stage, url: &str, retry_index: usize) -> Self {
        Self {
            transport,
            stage,
            retry_index,
            url: url.to_string(),
            final_url: String::new(),
            passed: false,
            skipped: false,
            retryable: false,
            phase: "创建请求",
            elapsed_ms: 0,
            headers_ms: None,
            body_ms: None,
            http_status: None,
            http_version: String::new(),
            content_type: String::new(),
            bytes: 0,
            detail: String::new(),
        }
    }

    fn request_error(&mut self, error: &reqwest::Error) {
        if error.is_connect() {
            self.phase = "建连（DNS / 代理隧道 / TLS，详见错误链）";
        }
        self.retryable = proxy::retryable_request_error(error);
        self.detail = proxy::format_request_error(self.phase, error);
    }

    fn report(&self) -> String {
        let state = if self.passed {
            "成功"
        } else if self.skipped {
            "未执行"
        } else {
            "失败"
        };
        let attempt = if self.retry_index > 0 {
            format!("第 {} 次重试", self.retry_index)
        } else {
            "首次请求".to_string()
        };
        let mut report = format!(
            "{} | {} | {attempt} | {state}\nURL：{}\n总耗时：{} ms\n阶段：{}\n",
            self.transport.label(),
            self.stage.label(),
            self.url,
            self.elapsed_ms,
            self.phase
        );
        if let Some(status) = self.http_status {
            let _ = writeln!(report, "HTTP：{status} / {}", self.http_version);
        }
        if !self.final_url.is_empty() && self.final_url != self.url {
            let _ = writeln!(report, "最终 URL：{}", self.final_url);
        }
        if !self.content_type.is_empty() {
            let _ = writeln!(report, "Content-Type：{}", self.content_type);
        }
        if let Some(headers_ms) = self.headers_ms {
            let _ = writeln!(report, "收到响应头：{headers_ms} ms");
        }
        if let Some(body_ms) = self.body_ms {
            let _ = writeln!(report, "读取正文：{body_ms} ms");
        }
        let _ = writeln!(report, "已读取：{} bytes\n{}", self.bytes, self.detail);
        report
    }
}

async fn read_probe(client: Client, step: &mut Step) -> Option<ProbeData> {
    let sent_at = Instant::now();
    step.phase = "发送请求 / 等待响应头";
    let mut response = match client.get(&step.url).send().await {
        Ok(response) => response,
        Err(error) => {
            step.request_error(&error);
            return None;
        }
    };
    step.headers_ms = Some(sent_at.elapsed().as_millis() as u64);
    step.http_status = Some(response.status().as_u16());
    step.http_version = format!("{:?}", response.version());
    step.final_url = response.url().to_string();
    step.content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    if !response.status().is_success() {
        step.phase = "HTTP 状态检查";
        step.retryable = proxy::retryable_response(&response);
        step.detail = format!("服务器返回 {}，未按有效数据处理", response.status());
        return None;
    }

    let body_started_at = Instant::now();
    let mut body = Vec::new();
    step.phase = "读取响应正文";
    loop {
        let chunk = response.chunk().await;
        step.body_ms = Some(body_started_at.elapsed().as_millis() as u64);
        match chunk {
            Ok(Some(chunk)) => {
                step.bytes += chunk.len();
                if step.bytes > MAX_BODY_BYTES {
                    step.detail = "响应正文超过 8 MiB 诊断上限，已停止读取".to_string();
                    return None;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(error) => {
                step.request_error(&error);
                return None;
            }
        }
    }

    step.phase = "解析 / 校验数据";
    let parsed = std::str::from_utf8(&body)
        .map_err(|error| format!("响应正文不是有效 UTF-8：{error}"))
        .and_then(|body| step.stage.parse(body));
    match parsed {
        Ok(data) => {
            step.passed = true;
            step.phase = "完成";
            step.detail = data.summary();
            Some(data)
        }
        Err(error) => {
            step.detail = error;
            None
        }
    }
}

fn record_step(steps: &mut Vec<Step>, step: Step, diagnostic_id: &str) {
    let report = redact_diagnostic_report(&step.report());
    if step.passed {
        tracing::info!(target: "lol_stats::aramkit", %diagnostic_id, "连接诊断：{report}");
    } else {
        tracing::warn!(target: "lol_stats::aramkit", %diagnostic_id, "连接诊断：{report}");
    }
    steps.push(step);
}

async fn check_stage(
    settings: &AramkitProxySettings,
    transport: Transport,
    stage: Stage,
    url: &str,
    deadline: Instant,
    steps: &mut Vec<Step>,
    diagnostic_id: &str,
) -> Option<ProbeData> {
    for retry_index in 0..=MAX_REQUEST_RETRIES {
        let mut step = Step::new(transport, stage, url, retry_index);
        if !steps.is_empty() {
            tokio::time::sleep(
                STEP_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
            )
            .await;
        }
        if Instant::now() >= deadline {
            step.skipped = true;
            step.phase = "诊断总时限";
            step.detail = "已达到 180 秒诊断上限，保留此前结果".to_string();
            record_step(steps, step, diagnostic_id);
            return None;
        }

        let started_at = Instant::now();
        // 各次请求独立建连，同一代理、URL、HTTP 版本和超时，仅改变 TLS 后端。
        let data = match transport.client(settings) {
            Ok(client) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match tokio::time::timeout(remaining, read_probe(client, &mut step)).await {
                    Ok(data) => data,
                    Err(_) => {
                        step.detail = format!("诊断达到 180 秒总时限，终止于：{}", step.phase);
                        None
                    }
                }
            }
            Err(error) => {
                step.detail = error;
                None
            }
        };
        step.elapsed_ms = started_at.elapsed().as_millis() as u64;
        let retry =
            step.retryable && retry_index < MAX_REQUEST_RETRIES && Instant::now() < deadline;
        record_step(steps, step, diagnostic_id);
        if !retry {
            return data;
        }
    }
    None
}

fn completed(steps: &[Step], transport: Transport) -> bool {
    STAGES.iter().all(|stage| {
        steps
            .iter()
            .rev()
            .find(|step| step.transport == transport && step.stage == *stage)
            .is_some_and(|step| step.passed)
    })
}

fn summarize(steps: &[Step]) -> (&'static str, &'static str) {
    if completed(steps, Transport::Rustls) {
        if steps.iter().all(|step| step.passed) {
            ("success", "连接正常，英雄总表与薇恩海克斯数据均已读取。")
        } else {
            ("warning", "Rustls 可读取完整数据，但对照或重试中存在异常。")
        }
    } else if TRANSPORTS
        .iter()
        .any(|transport| completed(steps, *transport))
    {
        (
            "warning",
            "Rustls 未全部通过，WTLS 对照通过；本轮诊断未修改已保存的设置。",
        )
    } else {
        ("error", "连接诊断未全部通过，请复制结果反馈。")
    }
}

pub(crate) async fn test_connection(
    settings: AramkitProxySettings,
) -> Result<AramkitConnectionTest, String> {
    let _guard = TEST_LOCK
        .try_lock()
        .map_err(|_| "已有连接诊断正在进行，请等待完成".to_string())?;
    let started_at = Instant::now();
    let tested_at = chrono::Local::now();
    let diagnostic_id = format!("aramkit-{}", tested_at.timestamp_millis());
    let mut report = format!(
        "ARAMKit 连接诊断\n诊断编号：{diagnostic_id}\n时间：{}\n软件版本：{}\n系统：{} {} / {}\n",
        tested_at.format("%Y-%m-%d %H:%M:%S%.3f %:z"),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        sysinfo::System::os_version().unwrap_or_default(),
        std::env::consts::ARCH,
    );
    let settings = match settings.normalized() {
        Ok(settings) => settings,
        Err(error) => {
            let summary = format!("未开始测试：{error}");
            let _ = writeln!(
                report,
                "{summary}\n未发送网络请求，未输出无效代理地址原文。"
            );
            tracing::warn!(target: "lol_stats::aramkit", %diagnostic_id, "{summary}");
            return Ok(AramkitConnectionTest {
                outcome: "error",
                elapsed_ms: started_at.elapsed().as_millis() as u64,
                champion_count: 0,
                augment_count: 0,
                base_win_rate: None,
                summary,
                report: redact_diagnostic_report(&report),
            });
        }
    };
    let connection = if settings.enabled {
        format!("本地代理 {}", settings.address)
    } else {
        "默认网络配置（未指定本地代理；不切换为强制直连）".to_string()
    };
    let _ = writeln!(report, "连接方式：{connection}");
    let _ = writeln!(
        report,
        "WTLS 兜底：{}\n统一超时：建连与完整请求均为 {} 秒，重试不延长超时。",
        if settings.wtls_enabled {
            "开启"
        } else {
            "关闭"
        },
        settings.timeout_seconds
    );
    report.push_str(
        "正式请求：开启 WTLS 时，每个 URL 最多尝试 Rustls、WTLS、WTLS；关闭时仅 Rustls。\n",
    );
    report.push_str(
        "测试英雄：薇恩 / Vayne / 67\n\
         诊断独立对照两种 TLS；每组瞬时网络失败最多重试 2 次，保留全部失败记录。\n\
         总时限：180 秒；串行请求，间隔 250 ms；不读取或写入业务数据缓存。\n\
         当前构建未启用 HTTP/2，当前方式已经是 HTTP/1.1，省略重复的 HTTP/1.1 对照。\n\
         两种方式均验证证书；Rustls 使用 webpki 根证书，Windows TLS 使用系统证书库。\n\
         每次请求独立建连；响应头耗时包含建连、代理隧道、TLS 和服务端等待，并非单独的 TLS 耗时。\n",
    );
    #[cfg(not(windows))]
    report.push_str("当前系统不是 Windows，跳过 Schannel 对照。\n");
    tracing::info!(target: "lol_stats::aramkit", %diagnostic_id, "开始连接诊断：{connection}");

    let deadline = started_at + TEST_BUDGET;
    let mut steps = Vec::new();
    let mut dataset: Option<DatasetCache> = None;
    let mut notes = Vec::new();
    for &transport in TRANSPORTS {
        if let Some(ProbeData::Dataset(candidate)) = check_stage(
            &settings,
            transport,
            Stage::Config,
            aramkit::CONFIG_URL,
            deadline,
            &mut steps,
            &diagnostic_id,
        )
        .await
        {
            if let Some(pinned) = &dataset {
                if pinned.rankings_url() != candidate.rankings_url() {
                    notes.push(
                        "主站返回了不同数据版本；后续对照统一使用首次成功解析的版本和相同 URL。"
                            .to_string(),
                    );
                }
            } else {
                dataset = Some(candidate);
            }
        }
    }

    let mut champion_count = 0;
    let mut augment_count = 0;
    let mut base_win_rate = None;
    for stage in [Stage::Rankings, Stage::Vayne] {
        let url = dataset.as_ref().map(|dataset| match stage {
            Stage::Rankings => dataset.rankings_url(),
            _ => dataset.champion_url(VAYNE_ID),
        });
        for &transport in TRANSPORTS {
            let Some(url) = &url else {
                let mut step = Step::new(transport, stage, "未获得数据地址", 0);
                step.skipped = true;
                step.phase = "等待有效数据版本";
                step.detail =
                    "所有方式均未读到有效版本，未猜测 JSON 地址，也未使用旧缓存".to_string();
                record_step(&mut steps, step, &diagnostic_id);
                continue;
            };
            match check_stage(
                &settings,
                transport,
                stage,
                url,
                deadline,
                &mut steps,
                &diagnostic_id,
            )
            .await
            {
                Some(ProbeData::Rankings(table)) if champion_count == 0 => {
                    champion_count = table.len()
                }
                Some(ProbeData::Vayne(stats)) if augment_count == 0 => {
                    augment_count = stats.augment_win_rates.len();
                    base_win_rate = Some(stats.base_win_rate);
                }
                _ => {}
            }
        }
    }

    if steps.iter().any(|step| step.retry_index > 0 && step.passed) {
        notes.push(
            "有请求在相同超时的重试中成功；首次失败仍保留，不能把重试成功等同于链路稳定。"
                .to_string(),
        );
    }
    if steps.iter().any(|step| !step.passed && !step.skipped) {
        notes.push("EOF / 超时本身不能确定断连方；对照差异不等于 Rustls 有 bug，也不自动修改正式请求或代理设置。".to_string());
    }
    let (outcome, conclusion) = summarize(&steps);
    let mut summary = conclusion.to_string();
    if let Some(base_win_rate) = base_win_rate {
        let rankings = if champion_count > 0 {
            format!("英雄总表 {champion_count} 个")
        } else {
            "英雄总表未加载".to_string()
        };
        let _ = write!(
            summary,
            "\n{rankings}；薇恩 {augment_count} 个海克斯，基础胜率 {:.2}%。",
            base_win_rate * 100.0
        );
    }
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let _ = writeln!(report, "\n结论：{summary}\n总耗时：{elapsed_ms} ms");
    for note in notes {
        let _ = writeln!(report, "备注：{note}");
    }
    for (index, step) in steps.iter().enumerate() {
        let _ = writeln!(report, "\n[{}] {}", index + 1, step.report());
    }
    tracing::info!(target: "lol_stats::aramkit", %diagnostic_id, elapsed_ms, outcome, "连接诊断结束：{summary}");
    Ok(AramkitConnectionTest {
        outcome,
        elapsed_ms,
        champion_count,
        augment_count,
        base_win_rate,
        summary,
        report: redact_diagnostic_report(&report),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vayne_requires_real_augment_data_not_only_http_success() {
        let parsed = Stage::Vayne.parse(r#"{"champion":{"stats":{"winRate":0.52}},"augments":{"all":[{"id":101,"winRate":0.55}]}}"#).unwrap();
        let ProbeData::Vayne(stats) = parsed else {
            panic!("wrong probe data")
        };
        assert_eq!(stats.augment_win_rates.len(), 1);
        assert_eq!(stats.base_win_rate, 0.52);
        assert!(Stage::Vayne
            .parse(r#"{"champion":{"stats":{"winRate":0.52}},"augments":{"all":[]}}"#)
            .is_err());
        assert!(Stage::Vayne.parse("<html>blocked</html>").is_err());
        assert!(Stage::Vayne.parse(r#"{"champion":{"stats":{"winRate":52}},"augments":{"all":[{"id":101,"winRate":0.55}]}}"#).is_err());
    }

    #[test]
    fn the_pinned_dataset_builds_both_json_urls() {
        let ProbeData::Dataset(dataset) = Stage::Config
            .parse(r#"aramKitDataEndpoint:"https://data.aramkit.com/",dataPath:"data/test-build""#)
            .unwrap()
        else {
            panic!("wrong probe data")
        };
        assert_eq!(
            dataset.rankings_url(),
            "https://data.aramkit.com/data/test-build/stats/all/champion-rankings.json"
        );
        assert_eq!(
            dataset.champion_url(VAYNE_ID),
            "https://data.aramkit.com/data/test-build/stats/all/champion-details/67.json"
        );
    }

    fn success_steps() -> Vec<Step> {
        STAGES
            .into_iter()
            .map(|stage| {
                let mut step = Step::new(Transport::Rustls, stage, "https://aramkit.com/", 0);
                step.passed = true;
                step
            })
            .collect()
    }

    #[test]
    fn a_partial_download_is_not_a_complete_connection_test() {
        let mut steps = success_steps();
        assert_eq!(summarize(&steps).0, "success");
        steps[2].passed = false;
        assert!(!completed(&steps, Transport::Rustls));
        assert_eq!(summarize(&steps).0, "error");
    }

    #[test]
    fn successful_timeout_retry_still_reports_the_original_failure() {
        let mut steps = success_steps();
        steps[2].passed = false;
        steps[2].retryable = true;
        let mut retry = Step::new(Transport::Rustls, Stage::Vayne, "https://aramkit.com/", 1);
        retry.passed = true;
        steps.push(retry);
        assert!(completed(&steps, Transport::Rustls));
        assert_eq!(summarize(&steps).0, "warning");
    }

    #[test]
    fn clipboard_report_keeps_lines_and_removes_credentials() {
        let report = redact_diagnostic_report(
            "诊断结果\nhttp://user:secret@127.0.0.1:7890/\nAuthorization: Bearer example-token\n原因 2：tls handshake eof",
        );
        assert_eq!(report.lines().count(), 4);
        assert!(!report.contains("user:secret"));
        assert!(!report.contains("example-token"));
        assert!(report.contains("原因 2：tls handshake eof"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_tls_success_does_not_disguise_current_transport_failure() {
        let mut steps = success_steps();
        steps[0].passed = false;
        for stage in STAGES {
            let mut step = Step::new(Transport::WindowsTls, stage, "https://aramkit.com/", 0);
            step.passed = true;
            steps.push(step);
        }
        let (outcome, summary) = summarize(&steps);
        assert_eq!(outcome, "warning");
        assert!(summary.contains("Rustls 未全部通过"));
    }

    #[tokio::test]
    #[ignore = "手动触发：真实访问 ARAMKit 主站、英雄总表和薇恩海克斯数据"]
    async fn manual_test_aramkit_connection() {
        let settings = std::env::var("ARAMKIT_TEST_PROXY")
            .map(|address| AramkitProxySettings {
                enabled: true,
                address,
                ..Default::default()
            })
            .unwrap_or_default();
        let result = test_connection(settings).await.unwrap();
        println!("{}", result.report);
        assert_ne!(result.outcome, "error");
        assert!(result.champion_count > 0);
        assert!(result.augment_count > 0);
    }
}
