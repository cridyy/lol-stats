//! League Client 窗口定位。
//!
//! 选人阶段的画面属于 LeagueClientUx.exe，而不是游戏进程。这里仅返回顶层窗口的
//! 客户区坐标，叠层绘制仍由共享的透明窗口完成。

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible,
};

const MIN_CLIENT_WIDTH: i32 = 800;
const MIN_CLIENT_HEIGHT: i32 = 450;

#[derive(Clone, Copy)]
pub(crate) struct ClientWindowBounds {
    pub(crate) hwnd: HWND,
    pub(crate) origin_x: i32,
    pub(crate) origin_y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

struct WindowSearch {
    pid: u32,
    best: Option<(HWND, i64)>,
}

/// 查找由指定 LCU 进程拥有的可见顶层窗口，并返回其客户区在屏幕上的位置。
pub(crate) fn find_client_window(pid: u32) -> Option<ClientWindowBounds> {
    if pid == 0 {
        return None;
    }

    let mut search = WindowSearch { pid, best: None };
    unsafe {
        let _ = EnumWindows(
            Some(find_client_window_callback),
            &mut search as *mut WindowSearch as LPARAM,
        );
    }

    let hwnd = search.best?.0;
    client_bounds(hwnd)
}

pub(crate) fn is_foreground(bounds: &ClientWindowBounds) -> bool {
    unsafe { GetForegroundWindow() == bounds.hwnd }
}

fn client_bounds(hwnd: HWND) -> Option<ClientWindowBounds> {
    unsafe {
        let mut client: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut client) == 0 {
            return None;
        }

        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if width < MIN_CLIENT_WIDTH || height < MIN_CLIENT_HEIGHT {
            return None;
        }

        let mut origin = POINT { x: 0, y: 0 };
        if ClientToScreen(hwnd, &mut origin) == 0 {
            return None;
        }

        Some(ClientWindowBounds {
            hwnd,
            origin_x: origin.x,
            origin_y: origin.y,
            width,
            height,
        })
    }
}

unsafe extern "system" fn find_client_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam as *mut WindowSearch);
    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
        return 1;
    }

    let mut process_id = 0u32;
    GetWindowThreadProcessId(hwnd, &mut process_id);
    if process_id != search.pid {
        return 1;
    }

    let Some(bounds) = client_bounds(hwnd) else {
        return 1;
    };
    let area = i64::from(bounds.width) * i64::from(bounds.height);
    if search
        .best
        .as_ref()
        .is_none_or(|(_, best_area)| area > *best_area)
    {
        search.best = Some((hwnd, area));
    }
    1
}
