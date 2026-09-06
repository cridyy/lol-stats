mod aramkit;
mod diagnostics;
mod proxy;

pub(crate) use diagnostics::{test_connection as test_aramkit_connection, AramkitConnectionTest};
pub(crate) use proxy::{configure as set_aramkit_proxy, AramkitProxySettings};
#[cfg(target_os = "windows")]
mod capture;
#[cfg(target_os = "windows")]
pub(crate) mod client_window;
#[cfg(target_os = "windows")]
mod d3d11;
#[cfg(target_os = "windows")]
mod detector;
#[cfg(target_os = "windows")]
mod dxgi;
#[cfg(target_os = "windows")]
mod ncnn;
#[cfg(target_os = "windows")]
mod ocr;
#[cfg(target_os = "windows")]
mod overlay;
#[cfg(target_os = "windows")]
mod wgc;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::services::client_discovery::discover_primary_client;
use crate::services::lcu::LcuClient;
use crate::services::models::{GameAssetEntry, GameflowSession, SummonerInfo};

#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const GAMEFLOW_POLL_INTERVAL: Duration = Duration::from_millis(500);
const OCR_INTERVAL: Duration = Duration::from_millis(100);
const HIDDEN_FRAME_LIMIT: u32 = 4;
const ACTIVE_PHASES: [&str; 3] = ["GameStart", "InProgress", "Reconnect"];
#[cfg(target_os = "windows")]
const CHAMPION_RANKINGS_RETRY_INITIAL: Duration = Duration::from_secs(5);
#[cfg(target_os = "windows")]
const CHAMPION_RANKINGS_RETRY_MAX: Duration = Duration::from_secs(60);
#[cfg(target_os = "windows")]
const IDENTITY_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
#[cfg(target_os = "windows")]
const OCR_INVALID_RESULT_LIMIT: u8 = 2;
const OCR_REPLACEMENT_CONFIRMATIONS: u8 = 2;
const CAPTURE_MODE_AUTO: u8 = 0;
const CAPTURE_MODE_WGC: u8 = 2;
const CAPTURE_MODE_DESKTOP_DC: u8 = 3;
const CAPTURE_MODE_DXGI: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum DetectionMode {
    HighPrecision = 0,
    Compatibility = 1,
}

impl DetectionMode {
    #[cfg(target_os = "windows")]
    fn label(self) -> &'static str {
        match self {
            Self::HighPrecision => "高精度方案",
            Self::Compatibility => "兼容方案",
        }
    }
}

#[cfg(target_os = "windows")]
fn runtime_log(message: impl AsRef<str>) {
    crate::services::runtime_log::hex(message);
}

#[cfg(target_os = "windows")]
fn runtime_log_detail(message: impl FnOnce() -> String) {
    crate::services::runtime_log::hex_detail(message);
}

static CONTROLLER: LazyLock<Controller> = LazyLock::new(Controller::new);
static AUGMENT_NAMES: LazyLock<AugmentNameIndex> = LazyLock::new(AugmentNameIndex::load);

struct Controller {
    enabled: AtomicBool,
    worker_started: AtomicBool,
    capture_mode: AtomicU8,
    detection_mode: AtomicU8,
    recognition_text_visible: AtomicBool,
}

impl Controller {
    fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            worker_started: AtomicBool::new(false),
            capture_mode: AtomicU8::new(CAPTURE_MODE_DXGI),
            detection_mode: AtomicU8::new(DetectionMode::HighPrecision as u8),
            recognition_text_visible: AtomicBool::new(false),
        }
    }

    fn ensure_worker(&self) {
        if self
            .worker_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        thread::spawn(|| {
            #[cfg(target_os = "windows")]
            run_windows_worker();

            #[cfg(not(target_os = "windows"))]
            while CONTROLLER.enabled.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_secs(1));
            }
        });
    }
}

/// 切换局内海克斯收益率识别。
///
/// 识别服务只在开关开启后启动；关闭时立即清空叠层，后台线程不再截图或发请求。
pub(crate) fn set_enabled(enabled: bool) {
    CONTROLLER.enabled.store(enabled, Ordering::SeqCst);
    tracing::info!(target: "lol_stats::hex", enabled, "海克斯胜率识别设置已更新");
    if enabled {
        CONTROLLER.ensure_worker();
    } else {
        #[cfg(target_os = "windows")]
        overlay::hide();
    }
}

/// 设置局内海克斯识别的截图来源。
///
/// 配置由前端本地持久化；这里仅保存当前进程状态，便于运行中立即切换。
pub(crate) fn set_capture_mode(mode: &str) -> Result<(), String> {
    let value = match mode.trim().to_ascii_lowercase().as_str() {
        "auto" => CAPTURE_MODE_AUTO,
        // 兼容旧版本已经写入 localStorage 的配置；窗口 DC 已移除，降级为桌面 DC。
        "window-dc" => CAPTURE_MODE_DESKTOP_DC,
        "wgc" => CAPTURE_MODE_WGC,
        "dxgi" => CAPTURE_MODE_DXGI,
        "desktop-dc" => CAPTURE_MODE_DESKTOP_DC,
        _ => return Err("不支持的海克斯截图模式，可选：dxgi、wgc、auto、desktop-dc".to_string()),
    };
    CONTROLLER.capture_mode.store(value, Ordering::SeqCst);
    tracing::info!(target: "lol_stats::hex", mode, "截图来源已设置");
    Ok(())
}

pub(crate) fn set_detection_mode(mode: &str) -> Result<(), String> {
    let mode = match mode.trim().to_ascii_lowercase().as_str() {
        "high-precision" => DetectionMode::HighPrecision,
        "compatibility" => DetectionMode::Compatibility,
        _ => return Err("不支持的海克斯检测方案，可选：high-precision、compatibility".to_string()),
    };
    CONTROLLER
        .detection_mode
        .store(mode as u8, Ordering::SeqCst);
    tracing::info!(target: "lol_stats::hex", ?mode, "检测方案已设置");
    Ok(())
}

pub(crate) fn set_recognition_text_visible(visible: bool) {
    CONTROLLER
        .recognition_text_visible
        .store(visible, Ordering::SeqCst);
    crate::services::runtime_log::hex(format!("海克斯识别名称显示：{visible}"));
}

#[cfg(target_os = "windows")]
fn current_detection_mode() -> DetectionMode {
    match CONTROLLER.detection_mode.load(Ordering::SeqCst) {
        value if value == DetectionMode::Compatibility as u8 => DetectionMode::Compatibility,
        _ => DetectionMode::HighPrecision,
    }
}

#[cfg(target_os = "windows")]
fn current_capture_mode() -> capture::CaptureMode {
    match CONTROLLER.capture_mode.load(Ordering::SeqCst) {
        CAPTURE_MODE_WGC => capture::CaptureMode::Wgc,
        CAPTURE_MODE_DXGI => capture::CaptureMode::Dxgi,
        CAPTURE_MODE_DESKTOP_DC => capture::CaptureMode::DesktopDc,
        _ => capture::CaptureMode::Auto,
    }
}

#[cfg(target_os = "windows")]
fn run_windows_worker() {
    runtime_log("识别线程启动");
    // 截图坐标和叠层坐标必须都使用物理像素，避免高 DPI / 多显示器时错位。
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::error!(target: "lol_stats::hex", %error, "创建识别运行时失败");
            return;
        }
    };

    runtime.block_on(async { recognition_loop().await });
}

#[cfg(target_os = "windows")]
async fn recognition_loop() {
    let mut lcu: Option<LcuClient> = None;
    let mut current_player = CurrentPlayerIdentity::default();
    let mut last_gameflow_poll = Instant::now() - GAMEFLOW_POLL_INTERVAL;
    let mut session_info: Option<ActiveSession> = None;
    let mut champ_select_info: Option<ChampSelectState> = None;
    let mut champion_rankings_task: Option<tokio::task::JoinHandle<_>> = None;
    let mut champion_rankings = None;
    let mut next_champion_rankings_retry = Instant::now();
    let mut champion_rankings_retry_delay = CHAMPION_RANKINGS_RETRY_INITIAL;
    let mut selection_overlay_key = None::<SelectionOverlayKey>;
    let mut capture = capture::GameWindowCapture::new();
    let mut recognizer: Option<ocr::PpOcrRec> = None;
    // 本地词表中的 ID 可能是旧版本 ID；启动识别后用当前 LCU 清单补齐名称映射。
    let mut augment_names = AUGMENT_NAMES.clone();
    let mut augment_assets_loaded = false;
    let mut next_augment_assets_retry = Instant::now();
    let mut last_ocr = Instant::now() - OCR_INTERVAL;
    let mut cards: [TrackedCard; 3] = std::array::from_fn(|_| TrackedCard::default());
    let mut last_frame_signature: Option<FrameSignature> = None;
    let mut missed_frames = 0u32;
    let mut capture_available: Option<bool> = None;
    let mut cards_detected: Option<bool> = None;
    let mut overlay_visible: Option<bool> = None;
    let mut last_client_error = None::<String>;
    let mut last_capture_mode = current_capture_mode();
    let mut last_detection_mode = current_detection_mode();
    let mut proxy_changes = proxy::subscribe();
    runtime_log(format!("海克斯检测方案：{}", last_detection_mode.label()));

    loop {
        if proxy_changes.has_changed().unwrap_or(false) {
            proxy_changes.borrow_and_update();
            if let Some(task) = champion_rankings_task.take() {
                task.abort();
            }
            next_champion_rankings_retry = Instant::now();
            champion_rankings_retry_delay = CHAMPION_RANKINGS_RETRY_INITIAL;
            if let Some(active) = session_info.as_mut() {
                active.next_stats_retry = Instant::now();
            }
            runtime_log("AramKit 代理配置已更新，保留有效缓存并重试未完成的数据请求");
        }

        if champion_rankings.is_none()
            && champion_rankings_task.is_none()
            && CONTROLLER.enabled.load(Ordering::SeqCst)
            && Instant::now() >= next_champion_rankings_retry
        {
            runtime_log("开始读取 AramKit 英雄总表");
            champion_rankings_task = Some(tokio::spawn(aramkit::load_champion_rankings_once()));
        }

        if champion_rankings_task
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            let task = champion_rankings_task
                .take()
                .expect("已确认英雄总表任务存在");
            match task.await {
                Ok(Ok(table)) => {
                    runtime_log(format!("AramKit 英雄总表加载完成：{} 个英雄", table.len()));
                    champion_rankings = Some(table);
                    champion_rankings_retry_delay = CHAMPION_RANKINGS_RETRY_INITIAL;
                }
                Ok(Err(error)) => {
                    tracing::warn!(target: "lol_stats::aramkit",
                        "AramKit 英雄总表读取失败，{} 秒后重试：{error}",
                        champion_rankings_retry_delay.as_secs()
                    );
                    next_champion_rankings_retry = Instant::now() + champion_rankings_retry_delay;
                    champion_rankings_retry_delay = champion_rankings_retry_delay
                        .saturating_mul(2)
                        .min(CHAMPION_RANKINGS_RETRY_MAX);
                }
                Err(error) => {
                    tracing::error!(target: "lol_stats::aramkit",
                        "英雄总表任务异常终止，{} 秒后重试：{error}",
                        champion_rankings_retry_delay.as_secs()
                    );
                    next_champion_rankings_retry = Instant::now() + champion_rankings_retry_delay;
                    champion_rankings_retry_delay = champion_rankings_retry_delay
                        .saturating_mul(2)
                        .min(CHAMPION_RANKINGS_RETRY_MAX);
                }
            }
        }

        if !CONTROLLER.enabled.load(Ordering::SeqCst) {
            overlay::hide();
            selection_overlay_key = None;
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        }

        if last_gameflow_poll.elapsed() >= GAMEFLOW_POLL_INTERVAL {
            last_gameflow_poll = Instant::now();
            match refresh_phase_snapshot(&mut lcu, &mut current_player).await {
                Ok(snapshot) => {
                    last_client_error = None;
                    let next_session = snapshot.active;
                    let active_changed = match (&session_info, &next_session) {
                        (Some(previous), Some(next)) => previous.key() != next.key(),
                        (None, None) => false,
                        _ => true,
                    };
                    if active_changed {
                        if let Some(next) = next_session.as_ref() {
                            runtime_log(format!(
                                "进入识别 session：game_id={} champion_id={}",
                                next.game_id, next.champion_id
                            ));
                        } else {
                            runtime_log("当前没有符合条件的海克斯大乱斗 session");
                        }
                        recognizer = None;
                        cards = std::array::from_fn(|_| TrackedCard::default());
                        last_frame_signature = None;
                        last_ocr = Instant::now() - OCR_INTERVAL;
                        missed_frames = 0;
                        capture_available = None;
                        cards_detected = None;
                        overlay_visible = Some(false);
                        overlay::hide();
                        session_info = next_session;
                    }

                    // 游戏阶段仍是 ChampSelect 时，选人 session 接口可能在页面过渡的
                    // 极短窗口返回 404/422。此时保留上一帧，避免已显示的候选卡胜率闪灭。
                    if !snapshot.retain_champ_select || snapshot.champ_select.is_some() {
                        let next_champ_select = snapshot.champ_select;
                        let champ_select_changed = match (&champ_select_info, &next_champ_select) {
                            (Some(previous), Some(next)) => previous != next,
                            (None, None) => false,
                            _ => true,
                        };
                        if champ_select_changed {
                            if let Some(next) = next_champ_select.as_ref() {
                                let (visible_layout, visible_ids) = next
                                    .visible_cards()
                                    .map(|(ids, layout)| (selection_layout_name(layout), ids))
                                    .unwrap_or(("无", Vec::new()));
                                runtime_log_detail(|| {
                                    format!(
                                    "进入选人胜率 session：game_id={} 阶段={} 候选={} 顶部={} 画面={}（原始候选顺序：{:?}，显示顺序：{:?}）",
                                    next.game_id,
                                    next.timer_phase,
                                    next.candidate_ids.len(),
                                    next.bench_ids.len(),
                                    visible_layout,
                                    next.candidate_ids,
                                    visible_ids,
                                )
                                });
                            } else {
                                runtime_log("当前没有可显示的选人英雄胜率 session");
                            }
                            champ_select_info = next_champ_select;
                            selection_overlay_key = None;
                            overlay::hide();
                        }
                    }
                }
                Err(error) => {
                    // 客户端重启或短暂切换阶段时，保留线程并在下一轮重新发现连接。
                    let error_text = error.to_string();
                    if last_client_error.as_deref() != Some(error_text.as_str()) {
                        tracing::warn!(target: "lol_stats::hex", error = %error_text, "游戏阶段读取失败");
                        last_client_error = Some(error_text);
                    }
                    lcu = None;
                    augment_names = AUGMENT_NAMES.clone();
                    augment_assets_loaded = false;
                    next_augment_assets_retry = Instant::now();
                    current_player.clear();
                    session_info = None;
                    champ_select_info = None;
                    recognizer = None;
                    cards = std::array::from_fn(|_| TrackedCard::default());
                    last_frame_signature = None;
                    selection_overlay_key = None;
                    overlay::hide();
                }
            }
        }

        if session_info.is_none() {
            let mut rendered = false;
            if let (Some(select), Some(table), Some(client)) = (
                champ_select_info.as_ref(),
                champion_rankings.as_ref(),
                lcu.as_ref(),
            ) {
                if let Some(bounds) = client_window::find_client_window(client.ux_pid()) {
                    if client_window::is_foreground(&bounds) {
                        let labels =
                            champ_select_overlay_labels(select, table, bounds.width, bounds.height);
                        if !labels.is_empty() {
                            let key = SelectionOverlayKey::new(bounds, &labels);
                            if selection_overlay_key.as_ref() != Some(&key) {
                                overlay::show(
                                    bounds.origin_x,
                                    bounds.origin_y,
                                    bounds.width,
                                    bounds.height,
                                    labels,
                                );
                                selection_overlay_key = Some(key);
                            }
                            rendered = true;
                        }
                    }
                }
            }
            if !rendered && (selection_overlay_key.take().is_some() || champ_select_info.is_none())
            {
                overlay::hide();
            }
            tokio::time::sleep(POLL_INTERVAL).await;
            continue;
        }

        let Some(active) = session_info.as_mut() else {
            tokio::time::sleep(POLL_INTERVAL).await;
            continue;
        };

        if !augment_assets_loaded && Instant::now() >= next_augment_assets_retry {
            next_augment_assets_retry = Instant::now() + Duration::from_secs(5);
            if let Some(client) = lcu.as_ref() {
                let assets = client.augment_assets().await;
                if assets.is_empty() {
                    runtime_log("当前客户端未返回 cherry-augments 清单，继续使用本地词表");
                } else {
                    let asset_count = assets.len();
                    augment_names.extend_assets(&assets);
                    augment_assets_loaded = true;
                    runtime_log(format!("已加载当前客户端海克斯映射：{asset_count} 项"));
                }
            }
        }

        if active.stats.is_none() && Instant::now() >= active.next_stats_retry {
            let mut request_proxy_changes = proxy_changes.clone();
            let result = tokio::select! {
                result = aramkit::load_champion_augments(active.champion_id) => result,
                _ = request_proxy_changes.changed() => continue,
            };
            match result {
                Ok(stats) => {
                    runtime_log(format!(
                        "AramKit 数据加载完成：champion_id={} augments={} base_win_rate={:.4}",
                        active.champion_id,
                        stats.augment_win_rates.len(),
                        stats.base_win_rate
                    ));
                    active.stats = Some(stats);
                }
                Err(error) => {
                    // 网络波动时使用短退避，避免 100ms 截图循环触发重复请求。
                    tracing::warn!(target: "lol_stats::aramkit", %error, "AramKit 数据读取失败");
                    active.next_stats_retry = Instant::now() + Duration::from_secs(5);
                }
            }
        }

        if active.stats.is_none() {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        let capture_mode = current_capture_mode();
        let detection_mode = current_detection_mode();
        let capture_mode_changed = capture_mode != last_capture_mode;
        let detection_mode_changed = detection_mode != last_detection_mode;
        if capture_mode_changed {
            runtime_log(format!(
                "截图模式切换：{:?} -> {:?}",
                last_capture_mode, capture_mode
            ));
            last_capture_mode = capture_mode;
        }
        if detection_mode_changed {
            runtime_log(format!(
                "海克斯检测方案切换：{} -> {}，重置卡片定位和 OCR 确认缓存",
                last_detection_mode.label(),
                detection_mode.label()
            ));
            last_detection_mode = detection_mode;
        }
        if capture_mode_changed || detection_mode_changed {
            cards = std::array::from_fn(|_| TrackedCard::default());
            last_frame_signature = None;
            capture_available = None;
            cards_detected = None;
            last_ocr = Instant::now() - OCR_INTERVAL;
            overlay::hide();
            overlay_visible = Some(false);
        }

        let frame = match capture.capture(capture_mode) {
            Ok(Some(frame)) => {
                if capture_available != Some(true) {
                    runtime_log(format!(
                        "截图成功：source={:?} origin=({}, {}) size={}x{}",
                        frame.source, frame.origin_x, frame.origin_y, frame.width, frame.height
                    ));
                    capture_available = Some(true);
                }
                frame
            }
            Ok(None) => {
                if capture_available != Some(false) {
                    runtime_log("截图暂不可用：未找到可捕获的游戏窗口或窗口区域");
                    capture_available = Some(false);
                }
                missed_frames = missed_frames.saturating_add(1);
                if missed_frames >= HIDDEN_FRAME_LIMIT {
                    overlay::hide();
                    cards = std::array::from_fn(|_| TrackedCard::default());
                    last_frame_signature = None;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
                continue;
            }
            Err(error) => {
                if capture_available != Some(false) {
                    tracing::warn!(target: "lol_stats::capture", %error, "截图失败");
                    capture_available = Some(false);
                }
                missed_frames = missed_frames.saturating_add(1);
                if missed_frames >= HIDDEN_FRAME_LIMIT {
                    overlay::hide();
                    cards = std::array::from_fn(|_| TrackedCard::default());
                    last_frame_signature = None;
                }
                tokio::time::sleep(POLL_INTERVAL).await;
                continue;
            }
        };
        missed_frames = 0;
        let frame_signature = FrameSignature::from(&frame);
        if last_frame_signature != Some(frame_signature) {
            if last_frame_signature.is_some() {
                runtime_log("截图来源或客户区尺寸变化，重置海克斯卡片定位缓存");
            }
            cards = std::array::from_fn(|_| TrackedCard::default());
            last_frame_signature = Some(frame_signature);
            cards_detected = None;
            last_ocr = Instant::now() - OCR_INTERVAL;
            overlay::hide();
            overlay_visible = Some(false);
        }

        // 每帧重新实测间距并更新区域，不锁定整局布局。检测短暂失败时，已确认
        // 槽位仍由原有 OCR 连续失败机制退出，避免光效导致全部标签同时闪动。
        if let Some(detection) =
            detector::detect_cards(frame.bgra, frame.width, frame.height, detection_mode)
        {
            for (slot, card) in cards.iter_mut().enumerate() {
                card.update_measured_area(
                    detection.title_rects[slot],
                    detection.label_centers[slot],
                );
            }
            if cards_detected != Some(true) {
                let spacing =
                    (detection.label_centers[2].0 - detection.label_centers[0].0) as f32 / 2.0;
                let card = detection.card_rects[1];
                runtime_log(format!(
                    "三张海克斯卡片定位成功：{}，实测间距={spacing:.1}px，卡片区域={}x{}",
                    detection_mode.label(),
                    card.width(),
                    card.height(),
                ));
                cards_detected = Some(true);
            }
        } else if cards_detected != Some(false) {
            runtime_log(format!(
                "三卡检测未命中（截图正常）：{}",
                detection_mode.label()
            ));
            cards_detected = Some(false);
        }

        if last_ocr.elapsed() >= OCR_INTERVAL
            && cards.iter().any(|card| card.default_title_rect.is_some())
        {
            if recognizer.is_none() {
                match ocr::PpOcrRec::from_embedded_model() {
                    Ok(value) => recognizer = Some(value),
                    Err(error) => {
                        tracing::error!(target: "lol_stats::ocr", %error, "OCR 初始化失败");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                }
            }

            let Some(recognizer) = recognizer.as_ref() else {
                continue;
            };
            let stats = active.stats.as_ref().expect("已检查 AramKit 数据存在");

            for (slot, card) in cards.iter_mut().enumerate() {
                let Some(default_title_rect) = card.default_title_rect else {
                    continue;
                };
                // 紧裁切只服务于这一帧 OCR。定位区域始终是最新检测给出的默认大标题区，
                // 不能被某次光效或短标题收紧后的临时区域污染。
                let crop_rect =
                    tighten_title_rect(frame.bgra, frame.width, frame.height, default_title_rect)
                        .unwrap_or_else(|| {
                            clip_rect(default_title_rect, frame.width, frame.height)
                        });
                let crop_signature = (
                    crop_rect.left,
                    crop_rect.top,
                    crop_rect.right,
                    crop_rect.bottom,
                );
                if card.last_ocr_crop != Some(crop_signature) {
                    runtime_log_detail(|| {
                        format!(
                            "OCR slot={} 裁剪：{}x{} -> {}x{}",
                            slot + 1,
                            default_title_rect.width(),
                            default_title_rect.height(),
                            crop_rect.width(),
                            crop_rect.height(),
                        )
                    });
                    card.last_ocr_crop = Some(crop_signature);
                }

                let crop = crop_bgra_to_rgb(frame.bgra, frame.width, frame.height, crop_rect);
                let text = match recognizer.recognize_rgb(&crop) {
                    Ok(text) => text,
                    Err(error) => {
                        if card.last_ocr_text != "<error>" {
                            tracing::warn!(target: "lol_stats::ocr", slot = slot + 1, %error, "OCR 识别失败");
                            card.last_ocr_text = "<error>".to_string();
                        }
                        if card.record_invalid_result() {
                            runtime_log(format!(
                                "OCR slot={} 连续失败，重新等待三卡定位",
                                slot + 1
                            ));
                        }
                        continue;
                    }
                };
                if card.last_ocr_text != text {
                    runtime_log_detail(|| format!("OCR slot={}：{}", slot + 1, text));
                    card.last_ocr_text = text.clone();
                }

                let Some(resolution) = resolve_augment(&augment_names, &text, stats) else {
                    if card.record_invalid_result() {
                        runtime_log_detail(|| {
                            format!(
                                "OCR slot={} 未匹配海克斯，连续失败后重新等待三卡定位：{}",
                                slot + 1,
                                text
                            )
                        });
                    }
                    continue;
                };

                if card.accept_resolution(resolution, &text) {
                    if let Some(resolution) = card.resolution.as_ref() {
                        runtime_log(format_ocr_confirmation(slot + 1, &text, resolution, stats));
                    }
                }
            }

            last_ocr = Instant::now();
            let show_recognition_text = CONTROLLER.recognition_text_visible.load(Ordering::SeqCst);
            let labels = cards
                .iter()
                .filter_map(|card| card.overlay_label(show_recognition_text))
                .collect::<Vec<_>>();
            if labels.is_empty() {
                overlay::hide();
                if overlay_visible != Some(false) {
                    runtime_log("叠层隐藏：没有可确认的海克斯标题");
                    overlay_visible = Some(false);
                }
            } else {
                overlay::show(
                    frame.origin_x,
                    frame.origin_y,
                    frame.width,
                    frame.height,
                    labels,
                );
                if overlay_visible != Some(true) {
                    runtime_log("收益率叠层已显示");
                    overlay_visible = Some(true);
                }
            }
        }

        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct PhaseSnapshot {
    active: Option<ActiveSession>,
    champ_select: Option<ChampSelectState>,
    /// 当 gameflow 仍在选人阶段、但 `/lol-champ-select/v1/session` 短暂不可读时，
    /// 不覆盖上一帧的选人状态。
    retain_champ_select: bool,
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct ChampSelectState {
    game_id: u64,
    timer_phase: String,
    allow_subset_champion_picks: bool,
    bench_enabled: bool,
    candidate_ids: Vec<u32>,
    bench_ids: Vec<u32>,
}

#[cfg(target_os = "windows")]
async fn refresh_phase_snapshot(
    lcu: &mut Option<LcuClient>,
    current_player: &mut CurrentPlayerIdentity,
) -> Result<PhaseSnapshot, String> {
    if lcu.is_none() {
        let auth = discover_primary_client().map_err(|error| error.to_string())?;
        *lcu = Some(LcuClient::new(&auth).map_err(|error| error.to_string())?);
        current_player.clear();
    }
    let Some(client) = lcu.as_ref() else {
        return Ok(PhaseSnapshot::default());
    };

    let session = match client.gameflow_session().await {
        Ok(session) => session,
        Err(error) => {
            *lcu = None;
            return Err(error.to_string());
        }
    };
    let Some(session) = session else {
        return Ok(PhaseSnapshot::default());
    };

    if session.phase.eq_ignore_ascii_case("ChampSelect") {
        if !is_hex_aram(&session) {
            return Ok(PhaseSnapshot::default());
        }

        let champ_select = match client.champ_select_session().await {
            Ok(session) => session,
            Err(error) => {
                // 选人接口在阶段切换的极短窗口可能返回 404/422；这不代表 LCU
                // 连接失效，保留下一轮重试，避免整个识别线程反复重建客户端。
                runtime_log(format!("选人 session 暂不可用，下一轮重试：{error}"));
                return Ok(PhaseSnapshot {
                    retain_champ_select: true,
                    ..PhaseSnapshot::default()
                });
            }
        };
        let Some(champ_select) = champ_select else {
            runtime_log("选人 session 暂未创建，下一轮重试");
            return Ok(PhaseSnapshot {
                retain_champ_select: true,
                ..PhaseSnapshot::default()
            });
        };

        // BAN_PICK 阶段的 2/3 张候选卡来自这个接口；FINALIZATION 阶段则
        // 主要使用 session 中的 benchChampions。接口短暂不可用时保留顶部列表。
        let candidate_ids = client
            .subset_champion_list()
            .await
            .ok()
            .flatten()
            .map(|ids| unique_champion_ids(ids.into_iter()))
            .unwrap_or_default();
        let bench_ids = unique_champion_ids(
            champ_select
                .bench_champions
                .into_iter()
                .map(|champion| champion.champion_id),
        );

        return Ok(PhaseSnapshot {
            active: None,
            champ_select: Some(ChampSelectState {
                game_id: champ_select.game_id.max(session.game_data.game_id),
                timer_phase: champ_select.timer.phase,
                allow_subset_champion_picks: champ_select.allow_subset_champion_picks,
                bench_enabled: champ_select.bench_enabled,
                candidate_ids,
                bench_ids,
            }),
            retain_champ_select: false,
        });
    }

    if !ACTIVE_PHASES.contains(&session.phase.as_str()) || !is_hex_aram(&session) {
        return Ok(PhaseSnapshot::default());
    }

    if current_player.needs_refresh() {
        current_player.mark_refresh_attempt();
        if let Ok(summoner) = client.current_summoner().await {
            current_player.update(&summoner);
        }
    }
    let champion_id = current_champion_id(&session, current_player).unwrap_or_default();
    if champion_id == 0 {
        return Ok(PhaseSnapshot::default());
    }

    Ok(PhaseSnapshot {
        active: Some(ActiveSession {
            game_id: session.game_data.game_id,
            champion_id,
            stats: None,
            next_stats_retry: Instant::now(),
        }),
        champ_select: None,
        retain_champ_select: false,
    })
}

#[cfg(target_os = "windows")]
fn unique_champion_ids(ids: impl IntoIterator<Item = u32>) -> Vec<u32> {
    let mut result = Vec::new();
    for id in ids {
        if id > 0 && !result.contains(&id) {
            result.push(id);
        }
    }
    result
}

#[cfg(target_os = "windows")]
fn current_champion_id(
    session: &GameflowSession,
    current_player: &CurrentPlayerIdentity,
) -> Option<u32> {
    session
        .game_data
        .player_champion_selections
        .iter()
        .find(|player| {
            current_player.matches(
                &player.puuid,
                player.summoner_id,
                &player.summoner_internal_name,
                "",
            )
        })
        .map(|player| player.champion_id)
        .filter(|champion_id| *champion_id > 0)
        .or_else(|| {
            session
                .game_data
                .team_one
                .iter()
                .chain(session.game_data.team_two.iter())
                .find(|player| {
                    current_player.matches(
                        &player.puuid,
                        player.summoner_id,
                        &player.summoner_internal_name,
                        &player.summoner_name,
                    )
                })
                .map(|player| player.champion_id)
                .filter(|champion_id| *champion_id > 0)
        })
}

/// 局内 session 在不同阶段返回的身份字段并不稳定：有时有 PUUID，有时只有
/// `summonerId` 或内部名。因此在拿到当前召唤师后保留多种身份作为匹配兜底。
#[cfg(target_os = "windows")]
#[derive(Debug, Default)]
struct CurrentPlayerIdentity {
    puuid: String,
    summoner_id: u64,
    internal_name: String,
    display_name: String,
    game_name: String,
    refreshed_at: Option<Instant>,
}

#[cfg(target_os = "windows")]
impl CurrentPlayerIdentity {
    fn clear(&mut self) {
        *self = Self::default();
    }

    fn needs_refresh(&self) -> bool {
        self.refreshed_at
            .is_none_or(|at| at.elapsed() >= IDENTITY_REFRESH_INTERVAL)
    }

    fn mark_refresh_attempt(&mut self) {
        self.refreshed_at = Some(Instant::now());
    }

    fn update(&mut self, summoner: &SummonerInfo) {
        self.puuid = summoner.puuid.trim().to_string();
        self.summoner_id = summoner.summoner_id;
        self.internal_name = summoner.internal_name.trim().to_string();
        self.display_name = summoner.display_name.trim().to_string();
        self.game_name = summoner.game_name.trim().to_string();
        self.refreshed_at = Some(Instant::now());
    }

    fn matches(
        &self,
        puuid: &str,
        summoner_id: u64,
        internal_name: &str,
        display_name: &str,
    ) -> bool {
        same_identity_value(puuid, &self.puuid)
            || (self.summoner_id > 0 && summoner_id == self.summoner_id)
            || same_identity_value(internal_name, &self.internal_name)
            || same_identity_value(internal_name, &self.display_name)
            || same_identity_value(internal_name, &self.game_name)
            || same_identity_value(display_name, &self.display_name)
            || same_identity_value(display_name, &self.game_name)
    }
}

#[cfg(target_os = "windows")]
fn same_identity_value(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    !left.is_empty() && !right.is_empty() && left.eq_ignore_ascii_case(right)
}

#[cfg(target_os = "windows")]
fn is_hex_aram(session: &GameflowSession) -> bool {
    session.game_data.queue.id == 2400
        || matches!(
            session
                .game_data
                .queue
                .game_mode
                .to_ascii_uppercase()
                .as_str(),
            "STRAWBERRY" | "KIWI"
        )
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy)]
enum SelectionCardLayout {
    Candidate,
    Bench,
}

#[cfg(target_os = "windows")]
fn selection_layout_name(layout: SelectionCardLayout) -> &'static str {
    match layout {
        SelectionCardLayout::Candidate => "候选卡",
        SelectionCardLayout::Bench => "顶部可用英雄",
    }
}

#[cfg(target_os = "windows")]
fn candidate_screen_order(mut ids: Vec<u32>) -> Vec<u32> {
    // `subset-champion-list` 在三卡时的中间/右侧顺序与画面相反：接口为
    // 1、3、2，屏幕为 1、2、3。两卡则已与屏幕顺序一致。
    if ids.len() == 3 {
        ids.swap(1, 2);
    }
    ids
}

#[cfg(target_os = "windows")]
impl ChampSelectState {
    fn visible_cards(&self) -> Option<(Vec<u32>, SelectionCardLayout)> {
        let is_ban_pick = self.timer_phase.eq_ignore_ascii_case("BAN_PICK");
        if is_ban_pick {
            // 旧客户端偶尔不返回 allowSubsetChampionPicks / benchEnabled，但候选
            // 接口本身的 2/3 个有效英雄已经足够说明当前画面是抽卡界面。
            if matches!(self.candidate_ids.len(), 2 | 3) {
                return Some((
                    candidate_screen_order(self.candidate_ids.clone()),
                    SelectionCardLayout::Candidate,
                ));
            }

            // 候选接口偶发短暂空结果时，不应退回到顶部备战席坐标，否则胜率会
            // 绘制到当前并不存在的英雄格上。下一轮拿到 2/3 张候选后再显示。
            return None;
        }

        if self.bench_enabled && !self.bench_ids.is_empty() {
            return Some((
                self.bench_ids.iter().copied().take(10).collect(),
                SelectionCardLayout::Bench,
            ));
        }

        None
    }
}

#[cfg(target_os = "windows")]
fn selection_label_position(
    layout: SelectionCardLayout,
    index: usize,
    card_count: usize,
) -> Option<(f32, f32)> {
    match layout {
        SelectionCardLayout::Candidate => {
            let x_ratio = match card_count {
                2 => [0.4152, 0.5848].get(index).copied(),
                3 => [0.3333, 0.5, 0.6667].get(index).copied(),
                _ => None,
            }?;
            Some((x_ratio, 0.638))
        }
        // 1280px 客户端中，十个顶部格的中心分别位于 x=379、438、496...906。
        // 坐标随客户区缩放，文字落在图标正下方、两侧状态条正上方。
        SelectionCardLayout::Bench if index < 10 => Some((0.2961 + index as f32 * 0.04575, 0.096)),
        SelectionCardLayout::Bench => None,
    }
}

#[cfg(target_os = "windows")]
fn champ_select_overlay_labels(
    state: &ChampSelectState,
    table: &aramkit::ChampionWinrateTable,
    width: i32,
    height: i32,
) -> Vec<overlay::OverlayLabel> {
    let Some((champion_ids, layout)) = state.visible_cards() else {
        return Vec::new();
    };
    if width <= 0 || height <= 0 {
        return Vec::new();
    }
    let card_count = champion_ids.len();

    champion_ids
        .into_iter()
        .enumerate()
        .filter_map(|(index, champion_id)| {
            let win_rate = table.win_rate(champion_id)?;
            let (x_ratio, y_ratio) = selection_label_position(layout, index, card_count)?;
            Some(overlay::OverlayLabel {
                center_x: (width as f32 * x_ratio).round() as i32,
                y: (height as f32 * y_ratio).round() as i32,
                title: None,
                text: format!("{:.1}%", win_rate * 100.0),
                tone: if win_rate >= 0.5 {
                    overlay::OverlayLabelTone::Positive
                } else {
                    overlay::OverlayLabelTone::Negative
                },
            })
        })
        .collect()
}

#[cfg(target_os = "windows")]
#[derive(Clone, PartialEq, Eq)]
struct SelectionOverlayKey {
    hwnd: isize,
    origin_x: i32,
    origin_y: i32,
    width: i32,
    height: i32,
    labels: Vec<SelectionOverlayLabelKey>,
}

#[cfg(target_os = "windows")]
#[derive(Clone, PartialEq, Eq)]
struct SelectionOverlayLabelKey {
    center_x: i32,
    y: i32,
    text: String,
    tone: u8,
}

#[cfg(target_os = "windows")]
impl SelectionOverlayKey {
    fn new(bounds: client_window::ClientWindowBounds, labels: &[overlay::OverlayLabel]) -> Self {
        Self {
            hwnd: bounds.hwnd as isize,
            origin_x: bounds.origin_x,
            origin_y: bounds.origin_y,
            width: bounds.width,
            height: bounds.height,
            labels: labels
                .iter()
                .map(|label| SelectionOverlayLabelKey {
                    center_x: label.center_x,
                    y: label.y,
                    text: label.text.clone(),
                    tone: overlay_label_tone_key(label.tone),
                })
                .collect(),
        }
    }
}

#[cfg(target_os = "windows")]
fn overlay_label_tone_key(tone: overlay::OverlayLabelTone) -> u8 {
    match tone {
        overlay::OverlayLabelTone::Positive => 0,
        overlay::OverlayLabelTone::Negative => 1,
        overlay::OverlayLabelTone::Unavailable => 2,
    }
}

#[cfg(target_os = "windows")]
fn clip_rect(rect: detector::Rect, width: i32, height: i32) -> detector::Rect {
    let left = rect.left.clamp(0, width);
    let top = rect.top.clamp(0, height);
    let right = rect.right.clamp(left, width);
    let bottom = rect.bottom.clamp(top, height);
    detector::Rect {
        left,
        top,
        right,
        bottom,
    }
}

/// 将固定标题区收紧到文字前景。
///
/// 海克斯标题可能只有两个字，而固定卡片宽度接近 200 像素；PP-OCR 在这种宽输入
/// 上会把绝大部分时间步判成 blank。这里仅利用标题行的亮色文字求边界，并且跳过
/// 区域底部，避免把标题下方的金色横线一起送入 OCR。
#[cfg(target_os = "windows")]
fn tighten_title_rect(
    bgra: &[u8],
    width: i32,
    height: i32,
    rect: detector::Rect,
) -> Option<detector::Rect> {
    if width <= 0 || height <= 0 || bgra.len() < width as usize * height as usize * 4 {
        return None;
    }
    let rect = clip_rect(rect, width, height);
    if rect.width() < 12 || rect.height() < 8 {
        return None;
    }

    let scan_top = rect.top + (rect.height() as f32 * 0.08).round() as i32;
    // 标题下方通常有细横线；最多扫描到 80% 高度，以免横线撑大文字边界。
    let scan_bottom = rect.top + (rect.height() as f32 * 0.80).round() as i32;
    if scan_bottom - scan_top < 6 {
        return None;
    }

    let mut left = rect.right;
    let mut top = scan_bottom;
    let mut right = rect.left;
    let mut bottom = scan_top;
    let mut foreground_count = 0usize;

    for y in scan_top..scan_bottom {
        for x in rect.left..rect.right {
            let index = ((y * width + x) * 4) as usize;
            let blue = bgra[index] as u16;
            let green = bgra[index + 1] as u16;
            let red = bgra[index + 2] as u16;
            // 白色/浅金色文字的亮度远高于深色标题背景。使用整数亮度计算，避免每帧
            // 三张卡 OCR 前再引入浮点和额外分配。
            let luma = (red * 54 + green * 183 + blue * 19) >> 8;
            if luma < 128 {
                continue;
            }

            foreground_count += 1;
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }

    if foreground_count < 8 || right <= left || bottom <= top {
        return None;
    }

    let text_width = right - left;
    let text_height = bottom - top;
    // 避免卡图或异常光效覆盖整个固定标题区域时截掉正常内容。
    if text_width < 8
        || text_height < 6
        || text_width as f32 > rect.width() as f32 * 0.88
        || text_height as f32 > rect.height() as f32 * 0.78
    {
        return None;
    }

    let padding_x = (rect.width() / 38).clamp(4, 8);
    let padding_y = (rect.height() / 14).clamp(2, 4);
    let tightened = detector::Rect {
        left: (left - padding_x).max(rect.left),
        top: (top - padding_y).max(rect.top),
        right: (right + padding_x).min(rect.right),
        bottom: (bottom + padding_y).min(scan_bottom),
    };
    (tightened.width() >= 12 && tightened.height() >= 8).then_some(tightened)
}

#[cfg(target_os = "windows")]
fn crop_bgra_to_rgb(bgra: &[u8], width: i32, height: i32, rect: detector::Rect) -> image::RgbImage {
    let rect = clip_rect(rect, width, height);
    let left = rect.left;
    let top = rect.top;
    let right = rect.right;
    let bottom = rect.bottom;
    let crop_width = (right - left).max(1) as u32;
    let crop_height = (bottom - top).max(1) as u32;
    let mut pixels = Vec::with_capacity(crop_width as usize * crop_height as usize * 3);

    for y in top..bottom {
        for x in left..right {
            let index = ((y * width + x) * 4) as usize;
            pixels.extend_from_slice(&[bgra[index + 2], bgra[index + 1], bgra[index]]);
        }
    }
    image::RgbImage::from_raw(crop_width, crop_height, pixels)
        .unwrap_or_else(|| image::RgbImage::new(crop_width, crop_height))
}

fn format_delta(delta: f64) -> String {
    format!("{:+.1}%", delta * 100.0)
}

#[cfg(target_os = "windows")]
#[derive(Clone, Debug, PartialEq, Eq)]
struct CardResolution {
    augment_id: u32,
    text: String,
    tone: overlay::OverlayLabelTone,
}

#[cfg(target_os = "windows")]
fn format_ocr_confirmation(
    slot: usize,
    ocr_text: &str,
    resolution: &CardResolution,
    stats: &aramkit::ChampionAugmentStats,
) -> String {
    let data_status = match stats.augment_win_rates.get(&resolution.augment_id) {
        Some(win_rate) => format!("命中，胜率={:.2}%", win_rate * 100.0),
        None => "未命中，该英雄统计中没有此ID".to_string(),
    };
    format!(
        "OCR slot={slot} 已确认：识别原文={ocr_text:?} 匹配ID={} 统计=[{data_status}] 显示={:?}",
        resolution.augment_id, resolution.text
    )
}

#[cfg(target_os = "windows")]
#[derive(Default)]
struct TrackedCard {
    // 每次定位成功都更新默认大标题区域；OCR 的紧裁切不写回这里。
    default_title_rect: Option<detector::Rect>,
    label_center: Option<(i32, i32)>,
    resolution: Option<CardResolution>,
    confirmed_ocr_text: String,
    pending_resolution: Option<CardResolution>,
    pending_hits: u8,
    invalid_streak: u8,
    last_ocr_text: String,
    last_ocr_crop: Option<(i32, i32, i32, i32)>,
}

#[cfg(target_os = "windows")]
impl TrackedCard {
    fn update_measured_area(&mut self, title_rect: detector::Rect, label_center: (i32, i32)) {
        self.default_title_rect = Some(title_rect);
        self.label_center = Some(label_center);
    }

    fn record_invalid_result(&mut self) -> bool {
        self.invalid_streak = self.invalid_streak.saturating_add(1);
        self.pending_resolution = None;
        self.pending_hits = 0;
        if self.invalid_streak < OCR_INVALID_RESULT_LIMIT {
            return false;
        }

        *self = Self::default();
        true
    }

    fn accept_resolution(&mut self, candidate: CardResolution, ocr_text: &str) -> bool {
        self.invalid_streak = 0;

        let Some(current) = self.resolution.as_ref() else {
            self.resolution = Some(candidate);
            self.confirmed_ocr_text = ocr_text.to_string();
            self.pending_resolution = None;
            self.pending_hits = 0;
            return true;
        };

        if current.augment_id == candidate.augment_id {
            let changed = current != &candidate;
            if changed {
                self.resolution = Some(candidate);
                self.confirmed_ocr_text = ocr_text.to_string();
            }
            self.pending_resolution = None;
            self.pending_hits = 0;
            return changed;
        }

        if self.pending_resolution.as_ref() == Some(&candidate) {
            self.pending_hits = self.pending_hits.saturating_add(1);
        } else {
            self.pending_resolution = Some(candidate);
            self.pending_hits = 1;
        }

        if self.pending_hits < OCR_REPLACEMENT_CONFIRMATIONS {
            return false;
        }

        self.resolution = self.pending_resolution.take();
        self.confirmed_ocr_text = ocr_text.to_string();
        self.pending_hits = 0;
        true
    }

    fn overlay_label(&self, show_recognition_text: bool) -> Option<overlay::OverlayLabel> {
        let (center_x, y) = self.label_center?;
        let resolution = self.resolution.as_ref()?;
        Some(overlay::OverlayLabel {
            center_x,
            y,
            title: self
                .default_title_rect
                .filter(|_| show_recognition_text && !self.confirmed_ocr_text.is_empty())
                .map(|rect| overlay::OverlayTitle {
                    text: self.confirmed_ocr_text.clone(),
                    max_width: rect.width(),
                }),
            text: resolution.text.clone(),
            tone: resolution.tone,
        })
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, PartialEq, Eq)]
struct FrameSignature {
    source: capture::CaptureSource,
    origin_x: i32,
    origin_y: i32,
    width: i32,
    height: i32,
}

#[cfg(target_os = "windows")]
impl From<&capture::CapturedFrame<'_>> for FrameSignature {
    fn from(value: &capture::CapturedFrame<'_>) -> Self {
        Self {
            source: value.source,
            origin_x: value.origin_x,
            origin_y: value.origin_y,
            width: value.width,
            height: value.height,
        }
    }
}

#[cfg(target_os = "windows")]
fn resolve_augment(
    augment_names: &AugmentNameIndex,
    text: &str,
    stats: &aramkit::ChampionAugmentStats,
) -> Option<CardResolution> {
    if let Some(augment_id) = augment_names.find_id(text, &stats.augment_win_rates) {
        let win_rate = stats.augment_win_rates.get(&augment_id)?;
        let delta = *win_rate - stats.base_win_rate;
        return Some(CardResolution {
            augment_id,
            text: format_delta(delta),
            tone: if delta >= 0.0 {
                overlay::OverlayLabelTone::Positive
            } else {
                overlay::OverlayLabelTone::Negative
            },
        });
    }

    // 名称能在本地词表或当前 LCU 海克斯清单中确认，但 AramKit 没有当前英雄的
    // 统计项时，不将其视为 OCR 失败，而是明确提示样本不足。
    augment_names
        .find_known_id(text)
        .map(|augment_id| CardResolution {
            augment_id,
            text: "选取率过低".to_string(),
            tone: overlay::OverlayLabelTone::Unavailable,
        })
}

#[cfg(target_os = "windows")]
struct ActiveSession {
    game_id: u64,
    champion_id: u32,
    stats: Option<aramkit::ChampionAugmentStats>,
    next_stats_retry: Instant,
}

#[cfg(target_os = "windows")]
impl ActiveSession {
    fn key(&self) -> (u64, u32) {
        (self.game_id, self.champion_id)
    }
}

#[derive(Debug, Deserialize)]
struct AugmentNameEntry {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    file: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "englishName")]
    english_name: String,
}

#[derive(Clone)]
struct AugmentNameIndex {
    by_name: HashMap<String, Vec<u32>>,
    // 旧词表中的英文名/图标文件名，用于把当前客户端 ID 关联回中文名。
    aliases: HashMap<String, Vec<String>>,
}

impl AugmentNameIndex {
    fn load() -> Self {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../src/assets/hextech/augments.zh-CN.json"
        ));
        let entries = serde_json::from_str::<Vec<AugmentNameEntry>>(raw).unwrap_or_default();
        let mut index = Self {
            by_name: HashMap::new(),
            aliases: HashMap::new(),
        };
        for entry in entries {
            if entry.id == 0 || entry.name.trim().is_empty() {
                continue;
            }
            let name = normalize_name(&entry.name);
            if name.is_empty() {
                continue;
            }
            index.add_name_id(&name, entry.id);
            index.add_alias(&entry.english_name, &name);
            index.add_alias(&asset_key(&entry.file), &name);
        }
        index
    }

    fn add_name_id(&mut self, name: &str, id: u32) {
        let ids = self.by_name.entry(name.to_string()).or_default();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }

    fn add_current_name_id(&mut self, name: &str, id: u32) {
        let ids = self.by_name.entry(name.to_string()).or_default();
        if !ids.contains(&id) {
            // 当前 LCU 清单后加载，放在最前面可覆盖本地历史词表中的同名旧 ID。
            ids.insert(0, id);
        }
    }

    fn add_alias(&mut self, alias: &str, name: &str) {
        let alias = normalize_name(alias);
        if alias.is_empty() {
            return;
        }
        let names = self.aliases.entry(alias).or_default();
        if !names.iter().any(|value| value == name) {
            names.push(name.to_string());
        }
    }

    fn extend_assets(&mut self, assets: &[GameAssetEntry]) {
        for asset in assets {
            if asset.id == 0 || asset.name.trim().is_empty() {
                continue;
            }

            let name = normalize_name(&asset.name);
            if name.is_empty() {
                continue;
            }

            self.add_current_name_id(&name, asset.id);

            // 国服 LCU 通常直接返回中文名；如果某个客户端返回英文名，
            // 再通过本地词表里的 englishName/图标文件名补回中文名称。
            for alias in [asset.name.as_str(), asset_key(&asset.icon_path).as_str()] {
                let Some(canonical_names) = self.aliases.get(&normalize_name(alias)).cloned()
                else {
                    continue;
                };
                for canonical_name in canonical_names {
                    self.add_current_name_id(&canonical_name, asset.id);
                }
            }
        }
    }

    fn find_id(&self, text: &str, available: &HashMap<u32, f64>) -> Option<u32> {
        let normalized = normalize_name(text);
        if normalized.is_empty() {
            return None;
        }
        if let Some(ids) = self.by_name.get(&normalized) {
            if let Some(id) = ids.iter().copied().find(|id| available.contains_key(id)) {
                return Some(id);
            }
        }

        // OCR 偶发一个字符误识时，只有高相似度且唯一最佳时才启用兜底，避免错标收益率。
        if normalized.chars().count() < 2 {
            return None;
        }
        let mut best: Option<(f32, u32)> = None;
        let mut second_best = 0.0f32;
        for (name, ids) in &self.by_name {
            let Some(id) = ids.iter().copied().find(|id| available.contains_key(id)) else {
                continue;
            };
            let score = similarity(&normalized, name);
            if best.is_none_or(|(best_score, _)| score > best_score) {
                second_best = best.map(|(best_score, _)| best_score).unwrap_or(0.0);
                best = Some((score, id));
            } else if score > second_best {
                second_best = score;
            }
        }
        best.filter(|(score, _)| *score >= 0.82 && *score - second_best >= 0.08)
            .map(|(_, id)| id)
    }

    fn find_known_id(&self, text: &str) -> Option<u32> {
        let normalized = normalize_name(text);
        if normalized.is_empty() {
            return None;
        }
        if let Some(ids) = self.by_name.get(&normalized) {
            return ids.first().copied();
        }

        // 与有统计数据的匹配保持相同的相似度门槛；这里仅用于判定“名称有效、样本
        // 缺失”，不能因一次弱 OCR 结果错误显示选取率提示。
        if normalized.chars().count() < 2 {
            return None;
        }
        let mut best: Option<(f32, u32)> = None;
        let mut second_best = 0.0f32;
        for (name, ids) in &self.by_name {
            let Some(id) = ids.first().copied() else {
                continue;
            };
            let score = similarity(&normalized, name);
            if best.is_none_or(|(best_score, _)| score > best_score) {
                second_best = best.map(|(best_score, _)| best_score).unwrap_or(0.0);
                best = Some((score, id));
            } else if score > second_best {
                second_best = score;
            }
        }
        best.filter(|(score, _)| *score >= 0.82 && *score - second_best >= 0.08)
            .map(|(_, id)| id)
    }
}

fn asset_key(value: &str) -> String {
    let file_name = value
        .rsplit(|character| character == '/' || character == '\\')
        .next()
        .unwrap_or(value);
    let stem = file_name
        .strip_suffix(".png")
        .or_else(|| file_name.strip_suffix(".webp"))
        .unwrap_or(file_name);
    stem.strip_suffix("_augment").unwrap_or(stem).to_string()
}

fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter_map(|character| {
            let character = match character {
                '\u{3000}' => ' ',
                '\u{FF01}'..='\u{FF5E}' => char::from_u32(character as u32 - 0xfee0)?,
                other => other,
            };
            if character.is_whitespace() || matches!(character, '·' | '・' | '-' | '_') {
                None
            } else {
                Some(character.to_ascii_uppercase())
            }
        })
        .collect()
}

fn similarity(left: &str, right: &str) -> f32 {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (i, left_char) in left.iter().enumerate() {
        let mut current = vec![i + 1; right.len() + 1];
        for (j, right_char) in right.iter().enumerate() {
            current[j + 1] = if left_char == right_char {
                previous[j]
            } else {
                1 + previous[j].min(previous[j + 1]).min(current[j])
            };
        }
        previous = current;
    }
    let distance = previous[right.len()] as f32;
    1.0 - distance / left.len().max(right.len()) as f32
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{format_delta, normalize_name, similarity};

    #[cfg(target_os = "windows")]
    use super::{
        candidate_screen_order, current_champion_id, is_hex_aram, refresh_phase_snapshot,
        resolve_augment, selection_label_position, tighten_title_rect, ActiveSession,
        CardResolution, ChampSelectState, CurrentPlayerIdentity, SelectionCardLayout, TrackedCard,
    };
    #[cfg(target_os = "windows")]
    use crate::services::models::{GameflowGameData, GameflowSession, PlayerChampionSelection};

    #[test]
    fn normalizes_full_width_ascii_and_spaces() {
        assert_eq!(normalize_name(" 回力ｏＫ镖 "), "回力OK镖");
    }

    #[test]
    fn formats_positive_and_negative_delta() {
        assert_eq!(format_delta(0.058), "+5.8%");
        assert_eq!(format_delta(-0.024), "-2.4%");
    }

    #[test]
    fn similarity_is_one_for_equal_names() {
        assert!((similarity("回力OK镖", "回力OK镖") - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn current_lcu_augment_id_overrides_historical_local_id() {
        let mut index = super::AugmentNameIndex::load();
        index.extend_assets(&[super::GameAssetEntry {
            id: 1022,
            name: "灵巧".to_string(),
            ..Default::default()
        }]);

        let mut available = HashMap::new();
        available.insert(1022, 0.57);
        assert_eq!(index.find_id("灵巧", &available), Some(1022));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn known_augment_without_aramkit_stats_shows_low_pick_rate() {
        let mut index = super::AugmentNameIndex::load();
        index.extend_assets(&[super::GameAssetEntry {
            id: 9001,
            name: "灵巧".to_string(),
            ..Default::default()
        }]);
        let stats = super::aramkit::ChampionAugmentStats {
            base_win_rate: 0.5,
            augment_win_rates: HashMap::new(),
        };

        let resolution = resolve_augment(&index, "灵巧", &stats).expect("应确认海克斯名称");
        assert_eq!(resolution.augment_id, 9001);
        assert_eq!(resolution.text, "选取率过低");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn ocr_confirmation_log_includes_raw_text_id_and_stats_status() {
        let stats = super::aramkit::ChampionAugmentStats {
            base_win_rate: 0.5,
            augment_win_rates: HashMap::from([(9002, 0.6)]),
        };
        let mut resolution = CardResolution {
            augment_id: 9001,
            text: "选取率过低".to_string(),
            tone: super::overlay::OverlayLabelTone::Unavailable,
        };
        let log = super::format_ocr_confirmation(1, " 回力ｏＫ镖 ", &resolution, &stats);
        assert!(log.contains("识别原文=\" 回力ｏＫ镖 \""));
        assert!(log.contains("匹配ID=9001"));
        assert!(log.contains("统计=[未命中，该英雄统计中没有此ID]"));
        assert!(log.contains("显示=\"选取率过低\""));

        resolution.augment_id = 9002;
        resolution.text = "+10.0%".to_string();
        resolution.tone = super::overlay::OverlayLabelTone::Positive;
        let log = super::format_ocr_confirmation(2, "暴击飞弹", &resolution, &stats);
        assert!(log.contains("识别原文=\"暴击飞弹\""));
        assert!(log.contains("统计=[命中，胜率=60.00%]"));
        assert!(log.contains("显示=\"+10.0%\""));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tracked_card_updates_measured_area_and_requires_two_failures_to_unlock() {
        use super::detector::Rect;
        use super::overlay::OverlayLabelTone;

        let first_area = Rect {
            left: 100,
            top: 200,
            right: 300,
            bottom: 260,
        };
        let updated_area = Rect {
            left: 120,
            top: 210,
            right: 280,
            bottom: 250,
        };
        let mut card = TrackedCard::default();
        card.update_measured_area(first_area, (200, 430));

        let first = CardResolution {
            augment_id: 1,
            text: "+1.0%".to_string(),
            tone: OverlayLabelTone::Positive,
        };
        let replacement = CardResolution {
            augment_id: 2,
            text: "-1.0%".to_string(),
            tone: OverlayLabelTone::Negative,
        };
        assert!(card.accept_resolution(first.clone(), "暴击飞弹"));
        card.update_measured_area(updated_area, (200, 420));
        assert_eq!(card.default_title_rect, Some(updated_area));
        assert_eq!(card.label_center, Some((200, 420)));
        assert!(!card.accept_resolution(first, " 暴击飞弹 "));
        assert_eq!(card.confirmed_ocr_text, "暴击飞弹");
        assert!(!card.accept_resolution(replacement.clone(), "灵巧"));
        let label = card.overlay_label(true).unwrap();
        assert_eq!(label.text, "+1.0%");
        let title = label.title.unwrap();
        assert_eq!(title.text, "暴击飞弹");
        assert_eq!(title.max_width, updated_area.width());
        let label_without_title = card.overlay_label(false).unwrap();
        assert!(label_without_title.title.is_none());
        assert_eq!(label_without_title.text, "+1.0%");
        assert_eq!(
            card.resolution.as_ref().map(|value| value.augment_id),
            Some(1)
        );
        assert!(card.accept_resolution(replacement, "灵巧"));
        let label = card.overlay_label(true).unwrap();
        assert_eq!(label.title.unwrap().text, "灵巧");
        assert_eq!(label.text, "-1.0%");
        assert_eq!(
            card.resolution.as_ref().map(|value| value.augment_id),
            Some(2)
        );

        assert!(!card.record_invalid_result());
        assert!(card.resolution.is_some());
        card.update_measured_area(updated_area, (200, 420));
        assert!(card.record_invalid_result());
        assert!(card.default_title_rect.is_none());
        assert!(card.resolution.is_none());
        assert!(card.confirmed_ocr_text.is_empty());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tightens_short_title_without_including_the_bottom_rule() {
        use super::detector::Rect;

        let width = 200i32;
        let height = 80i32;
        let mut bgra = vec![0u8; width as usize * height as usize * 4];
        let set_bright = |pixels: &mut [u8], x: i32, y: i32| {
            let index = ((y * width + x) * 4) as usize;
            pixels[index] = 230;
            pixels[index + 1] = 230;
            pixels[index + 2] = 230;
            pixels[index + 3] = 255;
        };

        // 两个短标题字，四周是固定标题区留下的空白。
        for y in 21..34 {
            for x in 84..114 {
                set_bright(&mut bgra, x, y);
            }
        }
        // 这个金色横线应被下方扫描边界排除。
        for x in 20..180 {
            set_bright(&mut bgra, x, 47);
        }

        let original = Rect {
            left: 20,
            top: 10,
            right: 180,
            bottom: 50,
        };
        let tight =
            tighten_title_rect(&bgra, width, height, original).expect("应能定位标题文字前景");

        assert!(tight.left <= 84 && tight.left >= 76);
        assert!(tight.right >= 114 && tight.right <= 122);
        assert!(tight.top <= 21 && tight.top >= 16);
        assert!(tight.bottom >= 34 && tight.bottom < 42);
        assert!(tight.width() < original.width() / 2);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn title_tightening_falls_back_when_no_text_is_visible() {
        use super::detector::Rect;

        let bgra = vec![0u8; 200 * 80 * 4];
        assert!(tighten_title_rect(
            &bgra,
            200,
            80,
            Rect {
                left: 20,
                top: 10,
                right: 180,
                bottom: 50,
            },
        )
        .is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn resolves_current_champion_with_summoner_id_when_puuid_is_missing() {
        let session = GameflowSession {
            game_data: GameflowGameData {
                player_champion_selections: vec![PlayerChampionSelection {
                    champion_id: 67,
                    summoner_id: 42,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let identity = CurrentPlayerIdentity {
            summoner_id: 42,
            ..Default::default()
        };

        assert_eq!(current_champion_id(&session, &identity), Some(67));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn resolves_current_champion_with_internal_name_when_puuid_is_missing() {
        let session = GameflowSession {
            game_data: GameflowGameData {
                player_champion_selections: vec![PlayerChampionSelection {
                    champion_id: 222,
                    summoner_internal_name: "example_internal_name".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        let identity = CurrentPlayerIdentity {
            internal_name: "EXAMPLE_INTERNAL_NAME".to_string(),
            ..Default::default()
        };

        assert_eq!(current_champion_id(&session, &identity), Some(222));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn three_candidate_cards_restore_the_screen_order() {
        assert_eq!(candidate_screen_order(vec![11, 33, 22]), vec![11, 22, 33]);
        assert_eq!(candidate_screen_order(vec![11, 22]), vec![11, 22]);

        let state = ChampSelectState {
            game_id: 1,
            timer_phase: "BAN_PICK".to_string(),
            allow_subset_champion_picks: true,
            bench_enabled: true,
            candidate_ids: vec![11, 33, 22],
            bench_ids: vec![101, 102],
        };
        let (ids, layout) = state.visible_cards().expect("应显示三张候选卡");
        assert_eq!(ids, vec![11, 22, 33]);
        assert!(matches!(layout, SelectionCardLayout::Candidate));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn ban_pick_never_falls_back_to_bench_positions_when_candidates_are_missing() {
        let state = ChampSelectState {
            game_id: 1,
            timer_phase: "BAN_PICK".to_string(),
            allow_subset_champion_picks: false,
            bench_enabled: true,
            candidate_ids: Vec::new(),
            bench_ids: vec![101, 102, 103],
        };

        assert!(state.visible_cards().is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn candidate_cards_render_even_if_older_client_omits_feature_flags() {
        let state = ChampSelectState {
            game_id: 1,
            timer_phase: "BAN_PICK".to_string(),
            candidate_ids: vec![11, 22],
            allow_subset_champion_picks: false,
            bench_enabled: false,
            bench_ids: Vec::new(),
        };

        let (ids, layout) = state.visible_cards().expect("应显示两张候选卡");
        assert_eq!(ids, vec![11, 22]);
        assert!(matches!(layout, SelectionCardLayout::Candidate));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn finalization_prefers_the_top_bench_heroes() {
        let state = ChampSelectState {
            game_id: 1,
            timer_phase: "FINALIZATION".to_string(),
            allow_subset_champion_picks: true,
            bench_enabled: true,
            candidate_ids: vec![11, 33, 22],
            bench_ids: vec![101, 102, 103],
        };
        let (ids, layout) = state.visible_cards().expect("应显示顶部可用英雄");
        assert_eq!(ids, vec![101, 102, 103]);
        assert!(matches!(layout, SelectionCardLayout::Bench));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn selection_label_coordinates_follow_the_client_layout() {
        let (left_x, candidate_y) =
            selection_label_position(SelectionCardLayout::Candidate, 0, 2).unwrap();
        let (right_x, _) = selection_label_position(SelectionCardLayout::Candidate, 1, 2).unwrap();
        assert!((left_x - 0.4152).abs() < f32::EPSILON);
        assert!((right_x - 0.5848).abs() < f32::EPSILON);
        assert!((candidate_y - 0.638).abs() < f32::EPSILON);

        let (first_bench_x, bench_y) =
            selection_label_position(SelectionCardLayout::Bench, 0, 10).unwrap();
        let (last_bench_x, _) =
            selection_label_position(SelectionCardLayout::Bench, 9, 10).unwrap();
        assert!((first_bench_x * 1280.0 - 379.0).abs() < 1.0);
        assert!((last_bench_x * 1280.0 - 906.0).abs() < 1.0);
        assert!((bench_y * 720.0 - 69.0).abs() < 1.0);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    #[ignore = "手动触发：输出当前局内海克斯识别前置条件"]
    async fn manual_debug_live_recognition_prerequisites() {
        let auth = crate::services::client_discovery::discover_primary_client()
            .expect("应能发现英雄联盟客户端");
        match super::client_window::find_client_window(auth.ux_pid) {
            Some(bounds) => println!(
                "hex-winrate prerequisite => ux_pid={} window=0x{:x} client_origin=({}, {}) client_size={}x{} foreground={}",
                auth.ux_pid,
                bounds.hwnd as usize,
                bounds.origin_x,
                bounds.origin_y,
                bounds.width,
                bounds.height,
                super::client_window::is_foreground(&bounds),
            ),
            None => println!(
                "hex-winrate prerequisite => ux_pid={} 未找到可见客户端窗口",
                auth.ux_pid
            ),
        }
        let mut lcu =
            Some(crate::services::lcu::LcuClient::new(&auth).expect("应能创建 LCU 客户端"));
        let client = lcu.as_ref().expect("LCU 客户端应存在");
        let session = client
            .gameflow_session()
            .await
            .expect("应能读取 gameflow session");
        let summoner = client.current_summoner().await.expect("应能读取当前召唤师");

        println!(
            "hex-winrate prerequisite => summoner puuid='{}' summoner_id={} internal='{}' game='{}' display='{}'",
            summoner.puuid,
            summoner.summoner_id,
            summoner.internal_name,
            summoner.game_name,
            summoner.display_name
        );
        if let Some(session) = &session {
            println!(
                "hex-winrate prerequisite => phase='{}' game_id={} queue_id={} queue_type='{}' game_mode='{}' is_hex={} selections={} team_one={} team_two={}",
                session.phase,
                session.game_data.game_id,
                session.game_data.queue.id,
                session.game_data.queue.type_,
                session.game_data.queue.game_mode,
                is_hex_aram(session),
                session.game_data.player_champion_selections.len(),
                session.game_data.team_one.len(),
                session.game_data.team_two.len(),
            );
        } else {
            println!("hex-winrate prerequisite => 当前没有 gameflow session");
        }

        let mut identity = CurrentPlayerIdentity::default();
        let snapshot = refresh_phase_snapshot(&mut lcu, &mut identity)
            .await
            .expect("识别前置检查不应失败");
        print_active_session(snapshot.active.as_ref());
    }

    #[cfg(target_os = "windows")]
    fn print_active_session(active: Option<&ActiveSession>) {
        match active {
            Some(active) => println!(
                "hex-winrate prerequisite => active game_id={} champion_id={}",
                active.game_id, active.champion_id
            ),
            None => println!("hex-winrate prerequisite => 未满足识别前置条件"),
        }
    }
}
