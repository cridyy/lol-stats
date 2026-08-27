use std::collections::HashSet;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sysinfo::System;
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, ClientToScreen, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC,
    ReleaseDC, SelectObject, BITMAPINFO, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible,
};

use super::wgc;

pub(crate) struct CapturedFrame<'a> {
    pub(crate) source: CaptureSource,
    pub(crate) origin_x: i32,
    pub(crate) origin_y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
    pub(crate) bgra: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureSource {
    Wgc,
    DesktopDc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureMode {
    /// WGC -> 桌面 DC（仅游戏前台）
    Auto,
    /// Windows Graphics Capture 窗口捕获
    Wgc,
    /// 旧式桌面 DC，只能拿到屏幕最上层的实际画面
    DesktopDc,
}

#[derive(Clone, Copy)]
struct ClientBounds {
    origin_x: i32,
    origin_y: i32,
    width: i32,
    height: i32,
}

/// 旧式桌面 DC 截图，仅用于强制兼容模式，或自动模式中游戏位于前台时的最后回退。
struct DesktopDcCapture {
    source_dc: HDC,
    memory_dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    pixels: *mut u8,
    width: i32,
    height: i32,
}

impl DesktopDcCapture {
    fn new() -> Self {
        Self {
            source_dc: null_mut(),
            memory_dc: null_mut(),
            bitmap: null_mut(),
            old_bitmap: null_mut(),
            pixels: null_mut(),
            width: 0,
            height: 0,
        }
    }

    fn capture(&mut self, bounds: ClientBounds) -> Result<bool, String> {
        self.ensure_buffer(bounds.width, bounds.height)?;
        let copied = unsafe {
            BitBlt(
                self.memory_dc,
                0,
                0,
                bounds.width,
                bounds.height,
                self.source_dc,
                bounds.origin_x,
                bounds.origin_y,
                SRCCOPY,
            ) != 0
        };
        Ok(copied)
    }

    fn ensure_buffer(&mut self, width: i32, height: i32) -> Result<(), String> {
        if !self.source_dc.is_null()
            && !self.memory_dc.is_null()
            && self.width == width
            && self.height == height
        {
            return Ok(());
        }

        self.release();
        unsafe {
            let source_dc = GetDC(null_mut());
            if source_dc.is_null() {
                return Err("无法取得桌面 DC".to_string());
            }
            let memory_dc = CreateCompatibleDC(source_dc);
            if memory_dc.is_null() {
                let _ = ReleaseDC(null_mut(), source_dc);
                return Err("无法创建桌面截图内存 DC".to_string());
            }

            let mut bitmap_info: BITMAPINFO = std::mem::zeroed();
            bitmap_info.bmiHeader.biSize = std::mem::size_of_val(&bitmap_info.bmiHeader) as u32;
            bitmap_info.bmiHeader.biWidth = width;
            bitmap_info.bmiHeader.biHeight = -height;
            bitmap_info.bmiHeader.biPlanes = 1;
            bitmap_info.bmiHeader.biBitCount = 32;
            bitmap_info.bmiHeader.biCompression = BI_RGB;

            let mut pixels: *mut c_void = null_mut();
            let bitmap = CreateDIBSection(
                memory_dc,
                &bitmap_info,
                DIB_RGB_COLORS,
                &mut pixels,
                null_mut(),
                0,
            );
            if bitmap.is_null() || pixels.is_null() {
                let _ = DeleteDC(memory_dc);
                let _ = ReleaseDC(null_mut(), source_dc);
                return Err("无法创建桌面截图位图缓冲区".to_string());
            }

            let old_bitmap = SelectObject(memory_dc, bitmap as HGDIOBJ);
            if old_bitmap.is_null() {
                let _ = DeleteObject(bitmap as HGDIOBJ);
                let _ = DeleteDC(memory_dc);
                let _ = ReleaseDC(null_mut(), source_dc);
                return Err("无法选择桌面截图位图".to_string());
            }

            self.source_dc = source_dc;
            self.memory_dc = memory_dc;
            self.bitmap = bitmap;
            self.old_bitmap = old_bitmap;
            self.pixels = pixels.cast::<u8>();
            self.width = width;
            self.height = height;
        }
        Ok(())
    }

    fn pixels(&self) -> Option<&[u8]> {
        if self.pixels.is_null() || self.width <= 0 || self.height <= 0 {
            return None;
        }
        Some(unsafe {
            std::slice::from_raw_parts(self.pixels, self.width as usize * self.height as usize * 4)
        })
    }

    fn release(&mut self) {
        unsafe {
            if !self.memory_dc.is_null() {
                if !self.old_bitmap.is_null() {
                    let _ = SelectObject(self.memory_dc, self.old_bitmap);
                }
                if !self.bitmap.is_null() {
                    let _ = DeleteObject(self.bitmap as HGDIOBJ);
                }
                let _ = DeleteDC(self.memory_dc);
            }
            if !self.source_dc.is_null() {
                let _ = ReleaseDC(null_mut(), self.source_dc);
            }
        }
        self.source_dc = null_mut();
        self.memory_dc = null_mut();
        self.bitmap = null_mut();
        self.old_bitmap = null_mut();
        self.pixels = null_mut();
        self.width = 0;
        self.height = 0;
    }
}

impl Drop for DesktopDcCapture {
    fn drop(&mut self) {
        self.release();
    }
}

pub(crate) struct GameWindowCapture {
    hwnd: HWND,
    wgc: Option<Arc<wgc::WgcState>>,
    wgc_last_seq: u64,
    next_wgc_retry: Instant,
    wgc_pixels: Vec<u8>,
    desktop: DesktopDcCapture,
    last_search: Option<Instant>,
}

impl GameWindowCapture {
    pub(crate) fn new() -> Self {
        Self {
            hwnd: null_mut(),
            wgc: None,
            wgc_last_seq: 0,
            next_wgc_retry: Instant::now(),
            wgc_pixels: Vec::new(),
            desktop: DesktopDcCapture::new(),
            last_search: None,
        }
    }

    pub(crate) fn capture(
        &mut self,
        mode: CaptureMode,
    ) -> Result<Option<CapturedFrame<'_>>, String> {
        let Some(hwnd) = self.resolve_game_window() else {
            return Ok(None);
        };
        let Some(bounds) = client_bounds(hwnd) else {
            self.clear_target();
            return Ok(None);
        };
        if bounds.width < 960 || bounds.height < 540 {
            return Ok(None);
        }

        if self.hwnd != hwnd {
            self.clear_target();
            self.hwnd = hwnd;
        }

        let can_use_desktop = mode == CaptureMode::DesktopDc
            || (mode == CaptureMode::Auto && is_foreground_window(hwnd));

        if matches!(mode, CaptureMode::Auto | CaptureMode::Wgc)
            && self.wgc.is_none()
            && Instant::now() >= self.next_wgc_retry
        {
            match wgc::init_wgc_state(hwnd as isize) {
                Ok(state) => {
                    self.wgc = Some(state);
                    self.wgc_last_seq = 0;
                }
                Err(_) => {
                    // WGC 下一秒重试，自动模式期间可回退桌面 DC，避免识别线程忙等。
                    self.next_wgc_retry = Instant::now() + Duration::from_secs(2);
                }
            }
        }

        // 克隆 Arc，避免在返回借用帧时同时持有 self.wgc 的字段借用。
        if matches!(mode, CaptureMode::Auto | CaptureMode::Wgc) {
            if let Some(state) = self.wgc.clone() {
                match wgc::capture_wgc_into(&state, self.wgc_last_seq, 80, &mut self.wgc_pixels) {
                    Ok(Some((frame, sequence))) if frame.width >= 960 && frame.height >= 540 => {
                        self.wgc_last_seq = sequence;
                        let length = frame.width as usize * frame.height as usize * 4;
                        if self.wgc_pixels.len() >= length {
                            return Ok(Some(CapturedFrame {
                                source: CaptureSource::Wgc,
                                origin_x: frame.origin_x,
                                origin_y: frame.origin_y,
                                width: frame.width,
                                height: frame.height,
                                bgra: &self.wgc_pixels[..length],
                            }));
                        }
                    }
                    Ok(Some(_)) | Ok(None) => {}
                    Err(_) => {
                        self.wgc = None;
                        self.wgc_last_seq = 0;
                        self.next_wgc_retry = Instant::now() + Duration::from_secs(2);
                    }
                }
            }
        }

        if can_use_desktop && self.desktop.capture(bounds)? {
            let bgra = self
                .desktop
                .pixels()
                .ok_or_else(|| "桌面截图位图为空".to_string())?;
            return Ok(Some(CapturedFrame {
                source: CaptureSource::DesktopDc,
                origin_x: bounds.origin_x,
                origin_y: bounds.origin_y,
                width: bounds.width,
                height: bounds.height,
                bgra,
            }));
        }
        Ok(None)
    }

    fn resolve_game_window(&mut self) -> Option<HWND> {
        if !self.hwnd.is_null() {
            let valid = unsafe {
                IsWindow(self.hwnd) != 0
                    && IsWindowVisible(self.hwnd) != 0
                    && IsIconic(self.hwnd) == 0
            };
            if valid {
                return Some(self.hwnd);
            }
            self.clear_target();
        }

        if self
            .last_search
            .is_some_and(|search| search.elapsed() < Duration::from_millis(500))
        {
            return None;
        }
        self.last_search = Some(Instant::now());
        self.hwnd = find_game_window().unwrap_or(null_mut());
        (!self.hwnd.is_null()).then_some(self.hwnd)
    }

    fn clear_target(&mut self) {
        self.wgc = None;
        self.wgc_last_seq = 0;
        self.wgc_pixels.clear();
        self.next_wgc_retry = Instant::now();
        self.desktop.release();
        self.hwnd = null_mut();
    }
}

fn client_bounds(hwnd: HWND) -> Option<ClientBounds> {
    unsafe {
        let mut client: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut client) == 0 {
            return None;
        }
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if width <= 0 || height <= 0 {
            return None;
        }
        let mut origin = POINT { x: 0, y: 0 };
        if ClientToScreen(hwnd, &mut origin) == 0 {
            return None;
        }
        Some(ClientBounds {
            origin_x: origin.x,
            origin_y: origin.y,
            width,
            height,
        })
    }
}

fn is_foreground_window(hwnd: HWND) -> bool {
    unsafe { GetForegroundWindow() == hwnd }
}

struct WindowSearch {
    game_pids: HashSet<u32>,
    best: Option<(HWND, i64)>,
}

fn find_game_window() -> Option<HWND> {
    let system = System::new_all();
    let game_pids = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            is_game_process(&process.name().to_string_lossy()).then_some(pid.as_u32())
        })
        .collect::<HashSet<_>>();
    if game_pids.is_empty() {
        return None;
    }

    let mut search = WindowSearch {
        game_pids,
        best: None,
    };
    unsafe {
        let _ = EnumWindows(
            Some(find_game_window_callback),
            &mut search as *mut WindowSearch as LPARAM,
        );
    }
    search.best.map(|(hwnd, _)| hwnd)
}

fn is_game_process(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("league of legends") && !name.contains("leagueclient")
}

unsafe extern "system" fn find_game_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam as *mut WindowSearch);
    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
        return 1;
    }

    let mut process_id = 0u32;
    GetWindowThreadProcessId(hwnd, &mut process_id);
    if !search.game_pids.contains(&process_id) {
        return 1;
    }

    let mut client: RECT = std::mem::zeroed();
    if GetClientRect(hwnd, &mut client) == 0 {
        return 1;
    }
    let width = client.right - client.left;
    let height = client.bottom - client.top;
    if width < 960 || height < 540 {
        return 1;
    }
    let area = i64::from(width) * i64::from(height);
    if search
        .best
        .as_ref()
        .is_none_or(|(_, best_area)| area > *best_area)
    {
        search.best = Some((hwnd, area));
    }
    1
}

#[cfg(test)]
mod tests {
    use std::fs::create_dir_all;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{CaptureMode, GameWindowCapture};

    fn save_bgra_png(bgra: &[u8], width: i32, height: i32, file_name: &str) -> PathBuf {
        let expected_length = width as usize * height as usize * 4;
        assert!(
            width > 0 && height > 0 && bgra.len() >= expected_length,
            "截图缓冲区尺寸异常：{}x{}，{} bytes",
            width,
            height,
            bgra.len()
        );

        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for pixel in bgra[..expected_length].chunks_exact(4) {
            rgb.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
        let image =
            image::RgbImage::from_raw(width as u32, height as u32, rgb).expect("无法构造 PNG 图像");

        let mut output = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        output.push("lol-stats");
        output.push("diagnostics");
        create_dir_all(&output).expect("无法创建截图诊断目录");
        output.push(file_name);
        image.save(&output).expect("无法写入截图");
        output
    }

    #[test]
    #[ignore = "手动触发：只验证英雄联盟 WGC 截图并导出 PNG"]
    fn manual_capture_wgc_to_png() {
        let mut capture = GameWindowCapture::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        let frame = loop {
            match capture.capture(CaptureMode::Wgc).expect("WGC 截图调用失败") {
                Some(frame) => break frame,
                None if Instant::now() < deadline => thread::sleep(Duration::from_millis(80)),
                None => panic!("WGC 在 3 秒内没有返回有效游戏窗口画面"),
            }
        };
        let output = save_bgra_png(
            frame.bgra,
            frame.width,
            frame.height,
            "wgc-window-capture.png",
        );

        println!(
            "wgc capture saved: source={:?} origin=({}, {}) size={}x{} path={}",
            frame.source,
            frame.origin_x,
            frame.origin_y,
            frame.width,
            frame.height,
            output.display()
        );
    }

    #[test]
    #[ignore = "手动触发：验证自动截图模式的实际回退来源并导出 PNG"]
    fn manual_capture_auto_to_png() {
        let mut capture = GameWindowCapture::new();
        let deadline = Instant::now() + Duration::from_secs(3);
        let frame = loop {
            match capture
                .capture(CaptureMode::Auto)
                .expect("自动截图调用失败")
            {
                Some(frame) => break frame,
                None if Instant::now() < deadline => thread::sleep(Duration::from_millis(80)),
                None => panic!("自动截图模式在 3 秒内没有返回有效游戏窗口画面"),
            }
        };
        let output = save_bgra_png(
            frame.bgra,
            frame.width,
            frame.height,
            "auto-window-capture.png",
        );

        println!(
            "auto capture saved: source={:?} origin=({}, {}) size={}x{} path={}",
            frame.source,
            frame.origin_x,
            frame.origin_y,
            frame.width,
            frame.height,
            output.display()
        );
    }
}
