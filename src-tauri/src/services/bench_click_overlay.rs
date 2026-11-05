//! 顶部备战区的透明点击层。
//!
//! 客户端与本程序属于不同进程，普通 Win32 窗口无法把 hover 事件可靠地转交给客户端。
//! 因此这个窗口只在快速交换开启、备战席可用且客户端位于前台时覆盖顶部英雄区：
//! 它不绘制内容、显示手型光标，并直接接管点击以绕开客户端按钮的视觉冷却。

#![cfg(target_os = "windows")]

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::thread;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, IsWindowVisible, KillTimer,
    LoadCursorW, PostMessageW, RegisterClassW, SetCursor, SetLayeredWindowAttributes, SetTimer,
    SetWindowPos, ShowWindow, TranslateMessage, EVENT_SYSTEM_FOREGROUND, HTTRANSPARENT, HWND_TOPMOST,
    IDC_HAND, LWA_ALPHA, MA_NOACTIVATE, MSG, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_SHOWWINDOW,
    SW_HIDE, SW_SHOWNOACTIVATE, WINEVENT_OUTOFCONTEXT, WM_APP, WM_LBUTTONDOWN, WM_MOUSEACTIVATE,
    WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

const UPDATE_MESSAGE: u32 = WM_APP + 0x57;
const VISIBILITY_TIMER: usize = 1;

/// 一个覆盖整个备战区的屏幕坐标矩形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BenchClickOverlayBounds {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

impl BenchClickOverlayBounds {
    pub(crate) fn is_valid(self) -> bool {
        self.width > 0 && self.height > 0
    }
}

struct ClickOverlayController {
    bounds: Mutex<Option<BenchClickOverlayBounds>>,
    started: AtomicBool,
    hwnd: AtomicIsize,
}

impl ClickOverlayController {
    fn new() -> Self {
        Self {
            bounds: Mutex::new(None),
            started: AtomicBool::new(false),
            hwnd: AtomicIsize::new(0),
        }
    }

    fn show(&self, bounds: BenchClickOverlayBounds) {
        if !bounds.is_valid() {
            self.hide();
            return;
        }
        if let Ok(mut current) = self.bounds.lock() {
            *current = Some(bounds);
        }
        self.ensure_started();
        self.request_update();
    }

    fn hide(&self) {
        if let Ok(mut current) = self.bounds.lock() {
            *current = None;
        }
        self.request_update();
    }

    fn bounds(&self) -> Option<BenchClickOverlayBounds> {
        self.bounds.lock().ok().and_then(|bounds| *bounds)
    }

    fn ensure_started(&self) {
        if self
            .started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        thread::spawn(click_overlay_thread);
    }

    fn request_update(&self) {
        let raw = self.hwnd.load(Ordering::Acquire);
        if raw <= 0 {
            return;
        }
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw as *mut _)),
                UPDATE_MESSAGE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

static CLICK_OVERLAY: LazyLock<ClickOverlayController> = LazyLock::new(ClickOverlayController::new);

pub(crate) fn show(bounds: BenchClickOverlayBounds) {
    CLICK_OVERLAY.show(bounds);
}

pub(crate) fn hide() {
    CLICK_OVERLAY.hide();
}

fn click_overlay_thread() {
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class_name = w!("LolStatsQuickBenchClickOverlay");
        let instance = match GetModuleHandleW(None) {
            Ok(instance) => instance,
            Err(error) => {
                tracing::error!(target: "lol_stats::quick_bench", %error, "无法创建透明备战区点击层");
                CLICK_OVERLAY.hwnd.store(-1, Ordering::Release);
                return;
            }
        };
        let hand_cursor = LoadCursorW(None, IDC_HAND).ok().unwrap_or_default();
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            hCursor: hand_cursor,
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassW(&class);

        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class_name,
            class_name,
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance.into()),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(error) => {
                tracing::error!(target: "lol_stats::quick_bench", %error, "创建透明备战区点击层失败");
                CLICK_OVERLAY.hwnd.store(-1, Ordering::Release);
                return;
            }
        };

        // 使用 1/255 的不透明度，视觉上完全不可见，但仍能稳定接收鼠标消息。
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA);
        CLICK_OVERLAY.hwnd.store(hwnd.0 as isize, Ordering::Release);
        apply_bounds(hwnd);

        // 在窗口消息线程监听前台切换，不依赖后台 LCU 请求是否完成。
        let foreground_hook = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        if foreground_hook.is_invalid() {
            tracing::warn!(target: "lol_stats::quick_bench", "前台窗口监听不可用，使用可见性定时检查");
        }
        // 同时处理快照过期，以及系统未送达前台事件的情况。
        let _ = SetTimer(Some(hwnd), VISIBILITY_TIMER, 100, None);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if message.message == UPDATE_MESSAGE {
                apply_bounds(hwnd);
            } else {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let _ = KillTimer(Some(hwnd), VISIBILITY_TIMER);
        if !foreground_hook.is_invalid() {
            let _ = UnhookWinEvent(foreground_hook);
        }
        CLICK_OVERLAY.hwnd.store(0, Ordering::Release);
    }
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _object_id: i32,
    _child_id: i32,
    _thread_id: u32,
    _time: u32,
) {
    let raw = CLICK_OVERLAY.hwnd.load(Ordering::Acquire);
    if raw > 0 {
        apply_bounds(HWND(raw as *mut _));
    }
}

fn should_show_overlay() -> bool {
    CLICK_OVERLAY.bounds().is_some_and(|bounds| bounds.is_valid())
        && crate::services::quick_bench_swap::can_handle_click_overlay()
}

unsafe fn apply_bounds(hwnd: HWND) {
    let Some(bounds) = CLICK_OVERLAY.bounds() else {
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
    };
    if !bounds.is_valid() || !crate::services::quick_bench_swap::can_handle_click_overlay() {
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
    }

    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        bounds.left,
        bounds.top,
        bounds.width,
        bounds.height,
        SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
    );
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_TIMER if wparam.0 == VISIBILITY_TIMER => {
                if IsWindowVisible(hwnd).as_bool() != should_show_overlay() {
                    apply_bounds(hwnd);
                }
                LRESULT(0)
            }
            WM_NCHITTEST if !should_show_overlay() => {
                let _ = ShowWindow(hwnd, SW_HIDE);
                LRESULT(HTTRANSPARENT as isize)
            }
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            WM_SETCURSOR => {
                if let Ok(cursor) = LoadCursorW(None, IDC_HAND) {
                    let _ = SetCursor(Some(cursor));
                }
                LRESULT(1)
            }
            WM_LBUTTONDOWN => {
                if !should_show_overlay() {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    return LRESULT(0);
                }
                let local_x = signed_low_word(lparam.0);
                let local_y = signed_high_word(lparam.0);
                if let Some(bounds) = CLICK_OVERLAY.bounds() {
                    crate::services::quick_bench_swap::handle_click_overlay_left_down(
                        bounds.left.saturating_add(local_x),
                        bounds.top.saturating_add(local_y),
                    );
                }
                LRESULT(0)
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut paint);
                let _ = EndPaint(hwnd, &paint);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

fn signed_low_word(value: isize) -> i32 {
    value as i16 as i32
}

fn signed_high_word(value: isize) -> i32 {
    ((value >> 16) as i16) as i32
}
