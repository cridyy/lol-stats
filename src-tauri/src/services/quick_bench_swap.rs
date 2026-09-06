//! 顶部备战席英雄的快速交换。
//!
//! 客户端自己的顶部英雄按钮会在一次交换后进入短暂 CD。优先使用覆盖整个顶部备战区
//! 的透明点击层直接调用 LCU，从而不触发客户端按钮自身的视觉冷却；低级鼠标钩子仍
//! 保留作诊断和透明层不可用时的兼容兜底。

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::sync::Mutex;
#[cfg(target_os = "windows")]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(target_os = "windows")]
use crate::services::bench_click_overlay::{self, BenchClickOverlayBounds};
#[cfg(target_os = "windows")]
use crate::services::client_discovery::discover_primary_client;
#[cfg(target_os = "windows")]
use crate::services::hex_winrate::client_window::{self, ClientWindowBounds};
#[cfg(target_os = "windows")]
use crate::services::lcu::LcuClient;

const DISABLED_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[cfg(target_os = "windows")]
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(12);
#[cfg(target_os = "windows")]
const SNAPSHOT_REFRESH_INTERVAL: Duration = Duration::from_millis(125);
#[cfg(target_os = "windows")]
const CLIENT_RETRY_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(target_os = "windows")]
const BENCH_SNAPSHOT_GRACE: Duration = Duration::from_secs(3);
#[cfg(target_os = "windows")]
const SWAP_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(target_os = "windows")]
const MAX_BENCH_CHAMPIONS: usize = 10;
#[cfg(target_os = "windows")]
const NATIVE_SWAP_SETTLE_DELAY: Duration = Duration::from_millis(100);

// 这些位置与海克斯胜率叠层的顶部备战席坐标保持一致。1280x720 客户端中，
// 第一个英雄中心约为 (379, 35)，后续卡位间距约 59px。
#[cfg(target_os = "windows")]
const BENCH_FIRST_CENTER_X_RATIO: f32 = 0.2961;
#[cfg(target_os = "windows")]
const BENCH_CENTER_Y_RATIO: f32 = 0.048;
#[cfg(target_os = "windows")]
const BENCH_STEP_X_RATIO: f32 = 0.04575;
#[cfg(target_os = "windows")]
const BENCH_HIT_HALF_WIDTH_RATIO: f32 = 0.0215;
#[cfg(target_os = "windows")]
const BENCH_HIT_HALF_HEIGHT_RATIO: f32 = 0.04;
// 鼠标状态轮询与 LCU 请求在后台线程中运行。原始点击从不被修改或吞掉。
#[cfg(target_os = "windows")]
const CLICK_DECISION_RECEIVED: u32 = 1;
#[cfg(target_os = "windows")]
const CLICK_DECISION_NO_SNAPSHOT: u32 = 2;
#[cfg(target_os = "windows")]
const CLICK_DECISION_SNAPSHOT_EXPIRED: u32 = 3;
#[cfg(target_os = "windows")]
const CLICK_DECISION_OUTSIDE_BENCH: u32 = 5;
#[cfg(target_os = "windows")]
const CLICK_DECISION_PREVIOUS_PENDING: u32 = 6;
#[cfg(target_os = "windows")]
const CLICK_DECISION_SWAP_QUEUED: u32 = 7;
#[cfg(target_os = "windows")]
const CLICK_DECISION_OVERLAY_SWAP_QUEUED: u32 = 8;

static CONTROLLER: LazyLock<QuickBenchSwapController> =
    LazyLock::new(QuickBenchSwapController::new);

#[cfg(target_os = "windows")]
fn runtime_log(message: impl AsRef<str>) {
    crate::services::runtime_log::quick_bench(message);
}

struct QuickBenchSwapController {
    enabled: AtomicBool,
    worker_started: AtomicBool,
    #[cfg(target_os = "windows")]
    mouse_hook_started: AtomicBool,
    #[cfg(target_os = "windows")]
    swap_pending: AtomicBool,
    #[cfg(target_os = "windows")]
    snapshot: Mutex<Option<BenchSnapshot>>,
    #[cfg(target_os = "windows")]
    click_snapshot_ready: AtomicBool,
    #[cfg(target_os = "windows")]
    click_snapshot_updated_ms: AtomicU64,
    #[cfg(target_os = "windows")]
    click_snapshot_hwnd: AtomicIsize,
    #[cfg(target_os = "windows")]
    click_snapshot_ux_pid: AtomicU32,
    #[cfg(target_os = "windows")]
    click_snapshot_origin_x: AtomicI32,
    #[cfg(target_os = "windows")]
    click_snapshot_origin_y: AtomicI32,
    #[cfg(target_os = "windows")]
    click_snapshot_width: AtomicI32,
    #[cfg(target_os = "windows")]
    click_snapshot_height: AtomicI32,
    #[cfg(target_os = "windows")]
    click_snapshot_champion_count: AtomicU32,
    #[cfg(target_os = "windows")]
    click_snapshot_champion_ids: [AtomicU32; MAX_BENCH_CHAMPIONS],
    #[cfg(target_os = "windows")]
    raw_left_down_count: AtomicU64,
    #[cfg(target_os = "windows")]
    last_click_at_ms: AtomicU64,
    #[cfg(target_os = "windows")]
    last_click_x: AtomicI32,
    #[cfg(target_os = "windows")]
    last_click_y: AtomicI32,
    #[cfg(target_os = "windows")]
    last_click_decision: AtomicU32,
    #[cfg(target_os = "windows")]
    last_click_foreground_hwnd: AtomicIsize,
    #[cfg(target_os = "windows")]
    last_click_foreground_pid: AtomicU32,
    #[cfg(target_os = "windows")]
    last_click_target_hwnd: AtomicIsize,
    #[cfg(target_os = "windows")]
    last_click_target_pid: AtomicU32,
    #[cfg(target_os = "windows")]
    pending_slot: AtomicU32,
    #[cfg(target_os = "windows")]
    pending_champion_id: AtomicU32,
    #[cfg(target_os = "windows")]
    pending_request_sequence: AtomicU64,
    #[cfg(target_os = "windows")]
    pending_not_before_ms: AtomicU64,
    #[cfg(target_os = "windows")]
    pending_from_click_overlay: AtomicBool,
}

impl QuickBenchSwapController {
    fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            worker_started: AtomicBool::new(false),
            #[cfg(target_os = "windows")]
            mouse_hook_started: AtomicBool::new(false),
            #[cfg(target_os = "windows")]
            swap_pending: AtomicBool::new(false),
            #[cfg(target_os = "windows")]
            snapshot: Mutex::new(None),
            #[cfg(target_os = "windows")]
            click_snapshot_ready: AtomicBool::new(false),
            #[cfg(target_os = "windows")]
            click_snapshot_updated_ms: AtomicU64::new(0),
            #[cfg(target_os = "windows")]
            click_snapshot_hwnd: AtomicIsize::new(0),
            #[cfg(target_os = "windows")]
            click_snapshot_ux_pid: AtomicU32::new(0),
            #[cfg(target_os = "windows")]
            click_snapshot_origin_x: AtomicI32::new(0),
            #[cfg(target_os = "windows")]
            click_snapshot_origin_y: AtomicI32::new(0),
            #[cfg(target_os = "windows")]
            click_snapshot_width: AtomicI32::new(0),
            #[cfg(target_os = "windows")]
            click_snapshot_height: AtomicI32::new(0),
            #[cfg(target_os = "windows")]
            click_snapshot_champion_count: AtomicU32::new(0),
            #[cfg(target_os = "windows")]
            click_snapshot_champion_ids: std::array::from_fn(|_| AtomicU32::new(0)),
            #[cfg(target_os = "windows")]
            raw_left_down_count: AtomicU64::new(0),
            #[cfg(target_os = "windows")]
            last_click_at_ms: AtomicU64::new(0),
            #[cfg(target_os = "windows")]
            last_click_x: AtomicI32::new(0),
            #[cfg(target_os = "windows")]
            last_click_y: AtomicI32::new(0),
            #[cfg(target_os = "windows")]
            last_click_decision: AtomicU32::new(0),
            #[cfg(target_os = "windows")]
            last_click_foreground_hwnd: AtomicIsize::new(0),
            #[cfg(target_os = "windows")]
            last_click_foreground_pid: AtomicU32::new(0),
            #[cfg(target_os = "windows")]
            last_click_target_hwnd: AtomicIsize::new(0),
            #[cfg(target_os = "windows")]
            last_click_target_pid: AtomicU32::new(0),
            #[cfg(target_os = "windows")]
            pending_slot: AtomicU32::new(0),
            #[cfg(target_os = "windows")]
            pending_champion_id: AtomicU32::new(0),
            #[cfg(target_os = "windows")]
            pending_request_sequence: AtomicU64::new(0),
            #[cfg(target_os = "windows")]
            pending_not_before_ms: AtomicU64::new(0),
            #[cfg(target_os = "windows")]
            pending_from_click_overlay: AtomicBool::new(false),
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

        #[cfg(target_os = "windows")]
        {
            thread::spawn(run_windows_worker);
            self.ensure_mouse_hook();
        }

        #[cfg(not(target_os = "windows"))]
        thread::spawn(|| loop {
            thread::sleep(DISABLED_POLL_INTERVAL);
        });
    }

    #[cfg(target_os = "windows")]
    fn ensure_mouse_hook(&self) {
        if self
            .mouse_hook_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        thread::spawn(install_mouse_hook);
    }

    #[cfg(target_os = "windows")]
    fn replace_snapshot(&self, snapshot: BenchSnapshot) {
        let message = self.snapshot.lock().ok().and_then(|mut current| {
            let changed = current.as_ref().is_none_or(|previous| {
                previous.hwnd != snapshot.hwnd
                    || previous.ux_pid != snapshot.ux_pid
                    || previous.origin_x != snapshot.origin_x
                    || previous.origin_y != snapshot.origin_y
                    || previous.width != snapshot.width
                    || previous.height != snapshot.height
                    || previous.champion_ids != snapshot.champion_ids
            });
            *current = Some(snapshot.clone());
            changed.then(|| {
                format!(
                    "备战席快照就绪：英雄={}，窗口={}x{}，原点=({}, {})，UX PID={}",
                    snapshot
                        .champion_ids
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                    snapshot.width,
                    snapshot.height,
                    snapshot.origin_x,
                    snapshot.origin_y,
                    snapshot.ux_pid,
                )
            })
        });
        self.publish_click_snapshot(&snapshot);
        if let Some(message) = message {
            runtime_log(message);
        }
    }

    #[cfg(target_os = "windows")]
    fn retain_or_clear_snapshot(&self) {
        let expired = self.snapshot.lock().ok().is_some_and(|mut current| {
            let expired = current
                .as_ref()
                .is_some_and(|snapshot| snapshot.updated_at.elapsed() > BENCH_SNAPSHOT_GRACE);
            if expired {
                *current = None;
            }
            expired
        });
        if expired {
            runtime_log("备战席快照已过期，停止观察顶部点击");
            self.clear_click_snapshot();
        }
    }

    #[cfg(target_os = "windows")]
    fn clear_snapshot(&self) {
        let had_snapshot = self.snapshot.lock().ok().is_some_and(|mut current| {
            let had_snapshot = current.is_some();
            *current = None;
            had_snapshot
        });
        self.clear_click_snapshot();
        if had_snapshot {
            runtime_log("已离开选人或功能关闭，清除备战席快照");
        }
    }

    #[cfg(target_os = "windows")]
    fn queue_swap_at(
        &self,
        screen_x: i32,
        screen_y: i32,
        observed_at_ms: u64,
        click_overlay: bool,
    ) -> bool {
        if !self.enabled.load(Ordering::SeqCst) {
            return false;
        }

        if !self.click_snapshot_ready.load(Ordering::Acquire) {
            self.last_click_decision
                .store(CLICK_DECISION_NO_SNAPSHOT, Ordering::Release);
            return false;
        }

        let updated_at_ms = self.click_snapshot_updated_ms.load(Ordering::Acquire);
        if elapsed_since_ms(updated_at_ms) > BENCH_SNAPSHOT_GRACE {
            self.last_click_decision
                .store(CLICK_DECISION_SNAPSHOT_EXPIRED, Ordering::Release);
            return false;
        }

        let (foreground_hwnd, foreground_pid) = foreground_window_identity();
        let (target_hwnd, target_pid) = window_at_identity(screen_x, screen_y);
        self.last_click_foreground_hwnd
            .store(foreground_hwnd, Ordering::Release);
        self.last_click_foreground_pid
            .store(foreground_pid, Ordering::Release);
        self.last_click_target_hwnd
            .store(target_hwnd, Ordering::Release);
        self.last_click_target_pid
            .store(target_pid, Ordering::Release);
        // WindowFromPoint 与前台窗口在全屏、置顶层和跨完整性级别场景下可能不可靠，
        // 只作为日志诊断。实际命中由已经确认的 ChampSelect 快照和客户区坐标决定。

        let origin_x = self.click_snapshot_origin_x.load(Ordering::Acquire);
        let origin_y = self.click_snapshot_origin_y.load(Ordering::Acquire);
        let width = self.click_snapshot_width.load(Ordering::Acquire);
        let height = self.click_snapshot_height.load(Ordering::Acquire);
        let champion_count = self
            .click_snapshot_champion_count
            .load(Ordering::Acquire)
            .min(MAX_BENCH_CHAMPIONS as u32) as usize;
        let local_x = screen_x - origin_x;
        let local_y = screen_y - origin_y;
        let Some(index) = top_bench_index_at(width, height, local_x, local_y, champion_count)
        else {
            self.last_click_decision
                .store(CLICK_DECISION_OUTSIDE_BENCH, Ordering::Release);
            return false;
        };
        let champion_id = self.click_snapshot_champion_ids[index].load(Ordering::Acquire);
        if champion_id == 0 {
            self.last_click_decision
                .store(CLICK_DECISION_OUTSIDE_BENCH, Ordering::Release);
            return false;
        }

        // 同一时刻只保留一次交换请求。透明层点击不会继续交给客户端；全局钩子
        // 路径仍会给客户端 100ms 自行处理的机会。
        if self
            .swap_pending
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            self.last_click_decision
                .store(CLICK_DECISION_PREVIOUS_PENDING, Ordering::Release);
            return false;
        }

        self.pending_slot
            .store((index + 1) as u32, Ordering::Relaxed);
        self.pending_champion_id
            .store(champion_id, Ordering::Relaxed);
        self.pending_from_click_overlay
            .store(click_overlay, Ordering::Relaxed);
        let not_before_ms = if click_overlay {
            now_epoch_millis()
        } else if observed_at_ms == 0 {
            now_epoch_millis().saturating_add(NATIVE_SWAP_SETTLE_DELAY.as_millis() as u64)
        } else {
            observed_at_ms.saturating_add(NATIVE_SWAP_SETTLE_DELAY.as_millis() as u64)
        };
        self.pending_not_before_ms
            .store(not_before_ms, Ordering::Relaxed);
        self.pending_request_sequence
            .fetch_add(1, Ordering::Release);
        self.last_click_decision.store(
            if click_overlay {
                CLICK_DECISION_OVERLAY_SWAP_QUEUED
            } else {
                CLICK_DECISION_SWAP_QUEUED
            },
            Ordering::Release,
        );
        true
    }

    #[cfg(target_os = "windows")]
    fn record_hook_left_down(&self, screen_x: i32, screen_y: i32) {
        self.last_click_x.store(screen_x, Ordering::Relaxed);
        self.last_click_y.store(screen_y, Ordering::Relaxed);
        self.last_click_at_ms
            .store(now_epoch_millis(), Ordering::Relaxed);
        self.raw_left_down_count.fetch_add(1, Ordering::Release);
    }

    #[cfg(target_os = "windows")]
    fn prepare_observed_click(&self) {
        self.last_click_foreground_hwnd.store(0, Ordering::Relaxed);
        self.last_click_foreground_pid.store(0, Ordering::Relaxed);
        self.last_click_target_hwnd.store(0, Ordering::Relaxed);
        self.last_click_target_pid.store(0, Ordering::Relaxed);
        self.last_click_decision
            .store(CLICK_DECISION_RECEIVED, Ordering::Release);
    }

    #[cfg(target_os = "windows")]
    fn publish_click_snapshot(&self, snapshot: &BenchSnapshot) {
        self.click_snapshot_ready.store(false, Ordering::Release);
        self.click_snapshot_hwnd
            .store(snapshot.hwnd, Ordering::Relaxed);
        self.click_snapshot_ux_pid
            .store(snapshot.ux_pid, Ordering::Relaxed);
        self.click_snapshot_origin_x
            .store(snapshot.origin_x, Ordering::Relaxed);
        self.click_snapshot_origin_y
            .store(snapshot.origin_y, Ordering::Relaxed);
        self.click_snapshot_width
            .store(snapshot.width, Ordering::Relaxed);
        self.click_snapshot_height
            .store(snapshot.height, Ordering::Relaxed);
        self.click_snapshot_champion_count
            .store(snapshot.champion_ids.len() as u32, Ordering::Relaxed);
        for (index, slot) in self.click_snapshot_champion_ids.iter().enumerate() {
            slot.store(
                snapshot
                    .champion_ids
                    .get(index)
                    .copied()
                    .unwrap_or_default(),
                Ordering::Relaxed,
            );
        }
        self.click_snapshot_updated_ms
            .store(now_epoch_millis(), Ordering::Relaxed);
        self.click_snapshot_ready.store(true, Ordering::Release);
        bench_click_overlay::show(click_overlay_bounds(snapshot));
    }

    #[cfg(target_os = "windows")]
    fn clear_click_snapshot(&self) {
        self.click_snapshot_ready.store(false, Ordering::Release);
        self.click_snapshot_champion_count
            .store(0, Ordering::Relaxed);
        self.click_snapshot_updated_ms.store(0, Ordering::Relaxed);
        for slot in &self.click_snapshot_champion_ids {
            slot.store(0, Ordering::Relaxed);
        }
        bench_click_overlay::hide();
    }
}

/// 切换快速交换。后台线程只会启动一次；关闭后不再观察顶部卡位点击。
pub(crate) fn set_enabled(enabled: bool) {
    CONTROLLER.enabled.store(enabled, Ordering::SeqCst);
    #[cfg(target_os = "windows")]
    runtime_log(format!(
        "快速交换备战英雄：{}",
        if enabled { "已开启" } else { "已关闭" }
    ));
    #[cfg(target_os = "windows")]
    if !enabled {
        CONTROLLER.swap_pending.store(false, Ordering::SeqCst);
        CONTROLLER.pending_not_before_ms.store(0, Ordering::SeqCst);
        CONTROLLER
            .pending_from_click_overlay
            .store(false, Ordering::SeqCst);
        CONTROLLER.clear_snapshot();
    }
    if enabled {
        CONTROLLER.ensure_worker();
    }
}

/// 透明备战区点击层收到左键后直接调用此入口。
///
/// 这条路径的原始点击已经被透明窗口截获，因此不再等待客户端自行处理，后台线程会在
/// 下一轮调度中直接调用 LCU。若透明层无法置顶，低级鼠标钩子仍会走旧的 100ms 兜底。
#[cfg(target_os = "windows")]
pub(crate) fn handle_click_overlay_left_down(screen_x: i32, screen_y: i32) {
    if !CONTROLLER.enabled.load(Ordering::SeqCst) {
        return;
    }

    runtime_log(format!(
        "透明备战区点击层收到左键：屏幕坐标=({screen_x}, {screen_y})"
    ));
    if !CONTROLLER.queue_swap_at(screen_x, screen_y, now_epoch_millis(), true) {
        runtime_log("透明备战区点击层未登记交换：点击不在当前可用英雄卡位或已有请求在处理");
    }
}

#[cfg(target_os = "windows")]
#[derive(Clone)]
struct BenchSnapshot {
    hwnd: isize,
    ux_pid: u32,
    origin_x: i32,
    origin_y: i32,
    width: i32,
    height: i32,
    champion_ids: Vec<u32>,
    updated_at: Instant,
}

#[cfg(target_os = "windows")]
impl BenchSnapshot {
    fn new(bounds: ClientWindowBounds, champion_ids: Vec<u32>, ux_pid: u32) -> Self {
        Self {
            hwnd: bounds.hwnd as isize,
            ux_pid,
            origin_x: bounds.origin_x,
            origin_y: bounds.origin_y,
            width: bounds.width,
            height: bounds.height,
            champion_ids,
            updated_at: Instant::now(),
        }
    }
}

#[cfg(target_os = "windows")]
enum SnapshotRefresh {
    Updated(BenchSnapshot),
    /// LCU 在客户端 CD 或页面过渡时会瞬间返回空备战席。保留最近一次可用卡位。
    Retain(String),
    /// 已离开选人，旧的点击区域不能再被截获。
    Clear(String),
}

#[cfg(target_os = "windows")]
fn report_snapshot_state(last_state: &mut Option<String>, state: String) {
    if last_state.as_deref() != Some(state.as_str()) {
        runtime_log(format!("备战席暂不可用：{state}"));
        *last_state = Some(state);
    }
}

#[cfg(target_os = "windows")]
fn run_windows_worker() {
    use windows_sys::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };

    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            runtime_log(format!("无法创建运行时：{error}"));
            return;
        }
    };

    runtime_log("快速交换后台线程已启动");
    runtime.block_on(windows_worker_loop());
}

#[cfg(target_os = "windows")]
fn install_mouse_hook() {
    use std::mem::zeroed;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
        MSG, WH_MOUSE_LL,
    };

    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let module = GetModuleHandleW(null());
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), module, 0);
        if hook.is_null() {
            runtime_log(format!(
                "无法安装低级鼠标钩子：{}；改用 Raw Input 兼容后备",
                std::io::Error::last_os_error()
            ));
            CONTROLLER.mouse_hook_started.store(false, Ordering::SeqCst);
            run_raw_input_monitor();
            return;
        }

        runtime_log("低级鼠标钩子已安装：只观察左键，始终放行原始输入");
        let mut message: MSG = zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result == -1 {
                runtime_log(format!(
                    "低级鼠标钩子消息循环异常退出：{}",
                    std::io::Error::last_os_error()
                ));
                break;
            }
            if result == 0 {
                runtime_log("低级鼠标钩子消息循环已退出");
                break;
            }
            let _ = TranslateMessage(&message);
            let _ = DispatchMessageW(&message);
        }
        let _ = UnhookWindowsHookEx(hook);
        CONTROLLER.mouse_hook_started.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    use std::ptr::null_mut;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, MSLLHOOKSTRUCT, WM_LBUTTONDOWN,
    };

    const HC_ACTION: i32 = 0;

    if code == HC_ACTION
        && wparam as u32 == WM_LBUTTONDOWN
        && lparam != 0
        && CONTROLLER.enabled.load(Ordering::Relaxed)
    {
        // 低级钩子在系统输入路径上运行。这里仅写原子数据，后续处理全部交给后台线程。
        let mouse = &*(lparam as *const MSLLHOOKSTRUCT);
        CONTROLLER.record_hook_left_down(mouse.pt.x, mouse.pt.y);
    }

    // 绝不能在这里返回 1；客户端必须始终收到原始点击。
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

#[cfg(target_os = "windows")]
fn run_raw_input_monitor() {
    use windows_sys::Win32::UI::HiDpi::{
        SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };

    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    if let Err(error) = run_raw_input_message_loop() {
        runtime_log(format!(
            "Raw Input 鼠标监测启动失败，回退到左键状态轮询：{error}"
        ));
        run_polling_mouse_monitor();
    }
}

#[cfg(target_os = "windows")]
fn run_polling_mouse_monitor() {
    runtime_log("左键状态轮询已启动：只作为 Raw Input 不可用时的兼容回退");
    let mut left_button_down = physical_left_button_down();
    loop {
        if CONTROLLER.enabled.load(Ordering::SeqCst) {
            poll_physical_left_button(&mut left_button_down);
        } else {
            // 避免功能关闭期间按住左键、重新开启时被错误识别成一次新点击。
            left_button_down = physical_left_button_down();
        }
        thread::sleep(WORKER_POLL_INTERVAL);
    }
}

#[cfg(target_os = "windows")]
async fn windows_worker_loop() {
    let mut lcu: Option<LcuClient> = None;
    let mut next_snapshot_refresh = Instant::now();
    let mut next_client_retry = Instant::now();
    let mut last_snapshot_state = None::<String>;
    let mut processed_request_sequence =
        CONTROLLER.pending_request_sequence.load(Ordering::Acquire);
    let mut processed_observed_click_count = CONTROLLER.raw_left_down_count.load(Ordering::Acquire);
    let mut reported_observed_click = (
        CONTROLLER.raw_left_down_count.load(Ordering::Acquire),
        CONTROLLER.last_click_decision.load(Ordering::Acquire),
    );

    loop {
        if !CONTROLLER.enabled.load(Ordering::SeqCst) {
            lcu = None;
            CONTROLLER.clear_snapshot();
            CONTROLLER.swap_pending.store(false, Ordering::SeqCst);
            processed_request_sequence =
                CONTROLLER.pending_request_sequence.load(Ordering::Acquire);
            tokio::time::sleep(DISABLED_POLL_INTERVAL).await;
            continue;
        }

        let observed_click_count = CONTROLLER.raw_left_down_count.load(Ordering::Acquire);
        if observed_click_count > processed_observed_click_count {
            // 低级钩子回调只写入原子变量；在此处才做坐标换算、窗口诊断和候选判断。
            processed_observed_click_count = observed_click_count;
            let screen_x = CONTROLLER.last_click_x.load(Ordering::Acquire);
            let screen_y = CONTROLLER.last_click_y.load(Ordering::Acquire);
            let observed_at_ms = CONTROLLER.last_click_at_ms.load(Ordering::Acquire);
            CONTROLLER.prepare_observed_click();
            let _ = CONTROLLER.queue_swap_at(screen_x, screen_y, observed_at_ms, false);
        }
        report_observed_click_diagnostics(&mut reported_observed_click);

        let request_sequence = CONTROLLER.pending_request_sequence.load(Ordering::Acquire);
        let pending_not_before_ms = CONTROLLER.pending_not_before_ms.load(Ordering::Acquire);
        if request_sequence > processed_request_sequence
            && now_epoch_millis() >= pending_not_before_ms
        {
            processed_request_sequence = request_sequence;
            let slot = CONTROLLER.pending_slot.load(Ordering::Acquire);
            let champion_id = CONTROLLER.pending_champion_id.load(Ordering::Acquire);
            let from_click_overlay = CONTROLLER
                .pending_from_click_overlay
                .load(Ordering::Acquire);
            execute_swap_request(&mut lcu, slot, champion_id, from_click_overlay).await;
            CONTROLLER.swap_pending.store(false, Ordering::SeqCst);
            CONTROLLER.pending_not_before_ms.store(0, Ordering::SeqCst);
            CONTROLLER
                .pending_from_click_overlay
                .store(false, Ordering::SeqCst);
            // 成功、失败都立即重新读取。保留已有快照，避免 CD 时间内产生空窗。
            next_snapshot_refresh = Instant::now();
        }

        let now = Instant::now();
        if now >= next_snapshot_refresh {
            match refresh_bench_snapshot(&mut lcu, &mut next_client_retry).await {
                SnapshotRefresh::Updated(snapshot) => {
                    last_snapshot_state = None;
                    CONTROLLER.replace_snapshot(snapshot);
                }
                SnapshotRefresh::Retain(reason) => {
                    report_snapshot_state(&mut last_snapshot_state, reason);
                    CONTROLLER.retain_or_clear_snapshot();
                }
                SnapshotRefresh::Clear(reason) => {
                    report_snapshot_state(&mut last_snapshot_state, reason);
                    CONTROLLER.clear_snapshot();
                }
            }
            next_snapshot_refresh = now + SNAPSHOT_REFRESH_INTERVAL;
        }

        tokio::time::sleep(WORKER_POLL_INTERVAL).await;
    }
}

#[cfg(target_os = "windows")]
async fn execute_swap_request(
    lcu: &mut Option<LcuClient>,
    slot: u32,
    champion_id: u32,
    from_click_overlay: bool,
) {
    if from_click_overlay {
        runtime_log(format!(
            "透明备战区点击层命中顶部第 {slot} 格英雄 {champion_id}，直接调用 LCU 交换"
        ));
        let result = match lcu.as_ref() {
            Some(client) => {
                tokio::time::timeout(SWAP_REQUEST_TIMEOUT, client.bench_swap(champion_id))
                    .await
                    .map_err(|_| "请求超时".to_string())
                    .and_then(|result| result.map_err(|error| error.to_string()))
            }
            None => Err("当前未连接到英雄联盟客户端".to_string()),
        };
        match result {
            Ok(()) => runtime_log(format!("透明点击层交换英雄 {champion_id} 成功")),
            Err(error) => {
                runtime_log(format!("透明点击层交换英雄 {champion_id} 失败：{error}"));
                if error.contains("连接") || error.contains("request") {
                    *lcu = None;
                }
            }
        }
        return;
    }

    let target_still_available = match lcu.as_ref() {
        Some(client) => {
            match tokio::time::timeout(SWAP_REQUEST_TIMEOUT, client.champ_select_session()).await {
                Ok(Ok(Some(session))) if session.bench_enabled => session
                    .bench_champions
                    .iter()
                    .any(|champion| champion.champion_id == champion_id),
                Ok(Ok(Some(_))) => false,
                Ok(Ok(None)) => {
                    runtime_log("100ms 后选人 session 已不可用，取消本次兜底交换");
                    false
                }
                Ok(Err(error)) => {
                    runtime_log(format!("100ms 后确认备战席失败，取消本次兜底交换：{error}"));
                    if error.to_string().contains("连接") || error.to_string().contains("request")
                    {
                        *lcu = None;
                    }
                    false
                }
                Err(_) => {
                    runtime_log("100ms 后确认备战席超时，取消本次兜底交换");
                    false
                }
            }
        }
        None => {
            runtime_log("100ms 后尚未连接英雄联盟客户端，取消本次兜底交换");
            false
        }
    };

    if !target_still_available {
        runtime_log(format!(
            "顶部第 {slot} 格英雄 {champion_id} 已被客户端处理或不再可用，保留原点击，不再重复交换"
        ));
        return;
    }

    runtime_log(format!(
        "顶部第 {slot} 格英雄 {champion_id} 在 100ms 后仍在备战席；客户端可能处于 CD，调用 LCU 兜底交换"
    ));
    let result = match lcu.as_ref() {
        Some(client) => tokio::time::timeout(SWAP_REQUEST_TIMEOUT, client.bench_swap(champion_id))
            .await
            .map_err(|_| "请求超时".to_string())
            .and_then(|result| result.map_err(|error| error.to_string())),
        None => Err("当前未连接到英雄联盟客户端".to_string()),
    };
    match result {
        Ok(()) => runtime_log(format!("兜底交换英雄 {champion_id} 成功")),
        Err(error) => {
            tracing::warn!(target: "lol_stats::quick_bench", champion_id, %error, "兜底交换英雄失败");
            if error.contains("连接") || error.contains("request") {
                *lcu = None;
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn report_observed_click_diagnostics(reported_raw_click: &mut (u64, u32)) {
    if !crate::services::runtime_log::detailed_enabled() {
        return;
    }
    let raw_left_down_count = CONTROLLER.raw_left_down_count.load(Ordering::Acquire);
    let decision = CONTROLLER.last_click_decision.load(Ordering::Acquire);
    if raw_left_down_count == reported_raw_click.0 && decision == reported_raw_click.1 {
        return;
    }
    *reported_raw_click = (raw_left_down_count, decision);
    if raw_left_down_count == 0 {
        return;
    }

    let screen_x = CONTROLLER.last_click_x.load(Ordering::Acquire);
    let screen_y = CONTROLLER.last_click_y.load(Ordering::Acquire);
    let foreground_hwnd = CONTROLLER
        .last_click_foreground_hwnd
        .load(Ordering::Acquire);
    let foreground_pid = CONTROLLER.last_click_foreground_pid.load(Ordering::Acquire);
    let target_hwnd = CONTROLLER.last_click_target_hwnd.load(Ordering::Acquire);
    let target_pid = CONTROLLER.last_click_target_pid.load(Ordering::Acquire);
    let expected_hwnd = CONTROLLER.click_snapshot_hwnd.load(Ordering::Acquire);
    let expected_pid = CONTROLLER.click_snapshot_ux_pid.load(Ordering::Acquire);
    tracing::debug!(target: "lol_stats::quick_bench",
        "低级鼠标钩子收到点击 #{raw_left_down_count}：屏幕坐标=({screen_x}, {screen_y})；{}；前台窗口=0x{foreground_hwnd:x}/PID {foreground_pid}；坐标命中窗口=0x{target_hwnd:x}/PID {target_pid}；客户端窗口=0x{expected_hwnd:x}/PID {expected_pid}",
        click_decision_text(decision),
    );
}

#[cfg(target_os = "windows")]
fn click_decision_text(decision: u32) -> &'static str {
    match decision {
        CLICK_DECISION_RECEIVED => "已收到原始点击，尚未完成命中判定",
        CLICK_DECISION_NO_SNAPSHOT => "未处理：当前没有有效备战席快照",
        CLICK_DECISION_SNAPSHOT_EXPIRED => "未处理：备战席快照已过期",
        CLICK_DECISION_OUTSIDE_BENCH => "未处理：坐标未命中顶部备战席",
        CLICK_DECISION_PREVIOUS_PENDING => "未处理：上一点击仍在等待 100ms 结果",
        CLICK_DECISION_SWAP_QUEUED => "已登记：等待客户端自行处理，100ms 后再检查",
        CLICK_DECISION_OVERLAY_SWAP_QUEUED => "已登记：透明备战区点击层将直接调用 LCU 交换",
        _ => "已收到点击，状态未知",
    }
}

#[cfg(target_os = "windows")]
async fn refresh_bench_snapshot(
    lcu: &mut Option<LcuClient>,
    next_client_retry: &mut Instant,
) -> SnapshotRefresh {
    let now = Instant::now();
    if lcu.is_none() {
        if now < *next_client_retry {
            return SnapshotRefresh::Retain("等待英雄联盟客户端连接重试".to_string());
        }
        *next_client_retry = now + CLIENT_RETRY_INTERVAL;

        let auth = match discover_primary_client() {
            Ok(auth) => auth,
            Err(error) => {
                return SnapshotRefresh::Clear(format!("未发现可用的英雄联盟客户端：{error}"));
            }
        };
        let client = match LcuClient::new(&auth) {
            Ok(client) => client,
            Err(error) => {
                return SnapshotRefresh::Clear(format!("无法建立 LCU 连接：{error}"));
            }
        };
        *lcu = Some(client);
    }

    let Some(client) = lcu.as_ref() else {
        return SnapshotRefresh::Clear("LCU 客户端尚未就绪".to_string());
    };
    let gameflow = match client.gameflow_session().await {
        Ok(Some(session)) => session,
        Ok(None) => return SnapshotRefresh::Clear("游戏流程 session 尚未创建".to_string()),
        Err(error) => {
            *lcu = None;
            return SnapshotRefresh::Retain(format!("读取游戏流程失败：{error}"));
        }
    };
    if !gameflow.phase.eq_ignore_ascii_case("ChampSelect") {
        return SnapshotRefresh::Clear(format!("当前游戏阶段为 {}，尚未进入选人", gameflow.phase));
    }

    let session = match client.champ_select_session().await {
        Ok(Some(session)) => session,
        // 选人页面切换或客户端 UI CD 时，这个接口可能短暂不可用或返回空值。
        // 此时后台仍握有 LCU 连接，可继续用最近一次有效英雄列表直接交换。
        Ok(None) => return SnapshotRefresh::Retain("选人 session 暂未创建".to_string()),
        Err(error) => return SnapshotRefresh::Retain(format!("读取选人 session 失败：{error}")),
    };
    if !session.bench_enabled {
        return SnapshotRefresh::Retain("当前选人未启用备战席".to_string());
    }

    let champion_ids = session
        .bench_champions
        .into_iter()
        .map(|champion| champion.champion_id)
        .filter(|champion_id| *champion_id > 0)
        .take(MAX_BENCH_CHAMPIONS)
        .collect::<Vec<_>>();
    if champion_ids.is_empty() {
        return SnapshotRefresh::Retain("备战席当前没有可交换英雄".to_string());
    }

    let Some(bounds) = client_window::find_client_window(client.ux_pid()) else {
        return SnapshotRefresh::Retain("未找到 LeagueClientUx 客户端窗口".to_string());
    };
    SnapshotRefresh::Updated(BenchSnapshot::new(bounds, champion_ids, client.ux_pid()))
}

#[cfg(target_os = "windows")]
fn run_raw_input_message_loop() -> Result<(), String> {
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DispatchMessageW, GetMessageW, RegisterClassW, TranslateMessage,
        HWND_MESSAGE, MSG, WNDCLASSW,
    };

    const CLASS_NAME: &[u16] = &[
        b'l' as u16,
        b'o' as u16,
        b'l' as u16,
        b'S' as u16,
        b't' as u16,
        b'a' as u16,
        b't' as u16,
        b's' as u16,
        b'R' as u16,
        b'a' as u16,
        b'w' as u16,
        b'I' as u16,
        b'n' as u16,
        b'p' as u16,
        b'u' as u16,
        b't' as u16,
        0,
    ];

    unsafe {
        let instance = GetModuleHandleW(null());
        if instance.is_null() {
            return Err(format!(
                "读取模块句柄失败：{}",
                std::io::Error::last_os_error()
            ));
        }

        let window_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(raw_input_window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: CLASS_NAME.as_ptr(),
        };
        let class_atom = RegisterClassW(&window_class);
        if class_atom == 0 && std::io::Error::last_os_error().raw_os_error() != Some(1410) {
            return Err(format!(
                "注册 Raw Input 窗口类失败：{}",
                std::io::Error::last_os_error()
            ));
        }

        let hwnd = CreateWindowExW(
            0,
            CLASS_NAME.as_ptr(),
            CLASS_NAME.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return Err(format!(
                "创建 Raw Input 消息窗口失败：{}",
                std::io::Error::last_os_error()
            ));
        }

        let device = RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x02,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        };
        if RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) == 0 {
            return Err(format!(
                "订阅 Raw Input 鼠标事件失败：{}",
                std::io::Error::last_os_error()
            ));
        }

        runtime_log("Raw Input 鼠标监测已启动：只订阅硬件左键，不拦截客户端输入");
        let mut message: MSG = zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result == -1 {
                return Err(format!(
                    "Raw Input 消息循环异常退出：{}",
                    std::io::Error::last_os_error()
                ));
            }
            if result == 0 {
                return Ok(());
            }
            let _ = TranslateMessage(&message);
            let _ = DispatchMessageW(&message);
        }
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn raw_input_window_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{DefWindowProcW, WM_INPUT};

    if message == WM_INPUT && CONTROLLER.enabled.load(Ordering::SeqCst) {
        handle_raw_mouse_input(lparam);
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

#[cfg(target_os = "windows")]
unsafe fn handle_raw_mouse_input(lparam: isize) {
    use std::mem::{size_of, MaybeUninit};
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::Input::{
        GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEMOUSE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, RI_MOUSE_LEFT_BUTTON_DOWN};

    if lparam == 0 {
        return;
    }

    let mut input = MaybeUninit::<RAWINPUT>::zeroed();
    let mut size = size_of::<RAWINPUT>() as u32;
    let read = GetRawInputData(
        lparam as HRAWINPUT,
        RID_INPUT,
        input.as_mut_ptr().cast(),
        &mut size,
        size_of::<RAWINPUTHEADER>() as u32,
    );
    if read == u32::MAX || read < size {
        tracing::warn!(target: "lol_stats::quick_bench",
            "读取 Raw Input 鼠标数据失败：{}",
            std::io::Error::last_os_error()
        );
        return;
    }

    let input = input.assume_init();
    if input.header.dwType != RIM_TYPEMOUSE {
        return;
    }
    let button_flags = input.data.mouse.Anonymous.Anonymous.usButtonFlags as u32;
    if button_flags & RI_MOUSE_LEFT_BUTTON_DOWN == 0 {
        return;
    }

    let mut point = POINT { x: 0, y: 0 };
    if GetCursorPos(&mut point) == 0 {
        tracing::warn!(target: "lol_stats::quick_bench",
            "Raw Input 收到左键但读取鼠标坐标失败：{}",
            std::io::Error::last_os_error()
        );
        return;
    }
    // Raw Input 仅在低级钩子无法安装时作为后备；同样只生产点击记录。
    CONTROLLER.record_hook_left_down(point.x, point.y);
}

#[cfg(target_os = "windows")]
fn poll_physical_left_button(was_down: &mut bool) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let is_down = physical_left_button_down();
    if is_down && !*was_down {
        let mut point = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut point) } != 0 {
            CONTROLLER.record_hook_left_down(point.x, point.y);
        } else {
            tracing::warn!(target: "lol_stats::quick_bench",
                "读取物理鼠标坐标失败：{}",
                std::io::Error::last_os_error()
            );
        }
    }
    *was_down = is_down;
}

#[cfg(target_os = "windows")]
fn physical_left_button_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};

    unsafe { (GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000) != 0 }
}

#[cfg(target_os = "windows")]
fn now_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(target_os = "windows")]
fn elapsed_since_ms(timestamp_ms: u64) -> Duration {
    Duration::from_millis(now_epoch_millis().saturating_sub(timestamp_ms))
}

#[cfg(target_os = "windows")]
fn foreground_window_identity() -> (isize, u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    unsafe { window_identity(GetForegroundWindow()) }
}

#[cfg(target_os = "windows")]
fn window_at_identity(screen_x: i32, screen_y: i32) -> (isize, u32) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::WindowFromPoint;

    unsafe {
        window_identity(WindowFromPoint(POINT {
            x: screen_x,
            y: screen_y,
        }))
    }
}

#[cfg(target_os = "windows")]
fn window_identity(hwnd: windows_sys::Win32::Foundation::HWND) -> (isize, u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    if hwnd.is_null() {
        return (0, 0);
    }
    let mut process_id = 0u32;
    unsafe {
        let _ = GetWindowThreadProcessId(hwnd, &mut process_id);
    }
    (hwnd as isize, process_id)
}

#[cfg(target_os = "windows")]
fn top_bench_index_at(
    width: i32,
    height: i32,
    local_x: i32,
    local_y: i32,
    champion_count: usize,
) -> Option<usize> {
    if width <= 0 || height <= 0 || champion_count == 0 {
        return None;
    }

    let half_width = width as f32 * BENCH_HIT_HALF_WIDTH_RATIO;
    let half_height = height as f32 * BENCH_HIT_HALF_HEIGHT_RATIO;
    let center_y = height as f32 * BENCH_CENTER_Y_RATIO;
    if (local_y as f32 - center_y).abs() > half_height {
        return None;
    }

    (0..champion_count.min(MAX_BENCH_CHAMPIONS)).find(|index| {
        let center_x =
            width as f32 * (BENCH_FIRST_CENTER_X_RATIO + *index as f32 * BENCH_STEP_X_RATIO);
        (local_x as f32 - center_x).abs() <= half_width
    })
}

#[cfg(target_os = "windows")]
fn click_overlay_bounds(snapshot: &BenchSnapshot) -> BenchClickOverlayBounds {
    let champion_count = snapshot.champion_ids.len().min(MAX_BENCH_CHAMPIONS);
    let half_width = snapshot.width as f32 * BENCH_HIT_HALF_WIDTH_RATIO;
    let half_height = snapshot.height as f32 * BENCH_HIT_HALF_HEIGHT_RATIO;
    let first_center_x = snapshot.width as f32 * BENCH_FIRST_CENTER_X_RATIO;
    let last_center_x = first_center_x
        + snapshot.width as f32 * BENCH_STEP_X_RATIO * champion_count.saturating_sub(1) as f32;
    let center_y = snapshot.height as f32 * BENCH_CENTER_Y_RATIO;
    let left = (snapshot.origin_x as f32 + first_center_x - half_width).floor() as i32;
    let top = (snapshot.origin_y as f32 + center_y - half_height).floor() as i32;
    let right = (snapshot.origin_x as f32 + last_center_x + half_width).ceil() as i32;
    let bottom = (snapshot.origin_y as f32 + center_y + half_height).ceil() as i32;

    BenchClickOverlayBounds {
        left,
        top,
        width: right.saturating_sub(left).max(1),
        height: bottom.saturating_sub(top).max(1),
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use std::time::Instant;

    use super::{click_overlay_bounds, top_bench_index_at, BenchSnapshot};

    #[test]
    fn maps_top_bench_card_centers_to_their_index() {
        assert_eq!(top_bench_index_at(1280, 720, 379, 35, 10), Some(0));
        assert_eq!(top_bench_index_at(1280, 720, 438, 35, 10), Some(1));
        assert_eq!(top_bench_index_at(1280, 720, 906, 35, 10), Some(9));
    }

    #[test]
    fn ignores_clicks_outside_the_top_bench_cards() {
        assert_eq!(top_bench_index_at(1280, 720, 408, 35, 10), None);
        assert_eq!(top_bench_index_at(1280, 720, 379, 100, 10), None);
        assert_eq!(top_bench_index_at(1280, 720, 965, 35, 10), None);
    }

    #[test]
    fn one_click_overlay_spans_every_visible_bench_card() {
        let snapshot = BenchSnapshot {
            hwnd: 0,
            ux_pid: 0,
            origin_x: 100,
            origin_y: 200,
            width: 1280,
            height: 720,
            champion_ids: (1..=10).collect(),
            updated_at: Instant::now(),
        };

        let bounds = click_overlay_bounds(&snapshot);
        assert!(bounds.left <= 100 + 379);
        assert!(bounds.left + bounds.width >= 100 + 906);
        assert!(bounds.top <= 200 + 35);
        assert!(bounds.top + bounds.height >= 200 + 35);
    }
}
