use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::Duration;

use windows::core::{w, Result};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EndPaint, GetDC,
    ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassW,
    SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow, HTTRANSPARENT, HWND_TOPMOST,
    MA_NOACTIVATE, MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_SHOWWINDOW,
    SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_APP, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};
use windows_numerics::Vector2;

const UPDATE_MESSAGE: u32 = WM_APP + 0x51;
const LABEL_LAYOUT_WIDTH: f32 = 220.0;
const LABEL_LAYOUT_HEIGHT: f32 = 72.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayLabelTone {
    Positive,
    Negative,
    Unavailable,
}

#[derive(Clone)]
pub(crate) struct OverlayTitle {
    pub(crate) text: String,
    pub(crate) max_width: i32,
}

#[derive(Clone)]
pub(crate) struct OverlayLabel {
    pub(crate) center_x: i32,
    pub(crate) y: i32,
    pub(crate) title: Option<OverlayTitle>,
    pub(crate) text: String,
    pub(crate) tone: OverlayLabelTone,
}

#[derive(Clone, Default)]
struct OverlaySnapshot {
    origin_x: i32,
    origin_y: i32,
    width: i32,
    height: i32,
    labels: Vec<OverlayLabel>,
}

struct OverlayController {
    snapshot: Mutex<OverlaySnapshot>,
    started: AtomicBool,
    hwnd: AtomicIsize,
}

impl OverlayController {
    fn new() -> Self {
        Self {
            snapshot: Mutex::new(OverlaySnapshot::default()),
            started: AtomicBool::new(false),
            hwnd: AtomicIsize::new(0),
        }
    }

    fn update(
        &self,
        origin_x: i32,
        origin_y: i32,
        width: i32,
        height: i32,
        labels: Vec<OverlayLabel>,
    ) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.origin_x = origin_x;
            snapshot.origin_y = origin_y;
            snapshot.width = width;
            snapshot.height = height;
            snapshot.labels = labels;
        }
        self.ensure_started();
        self.request_redraw();
    }

    fn hide(&self) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            snapshot.labels.clear();
        }
        self.request_redraw();
    }

    fn snapshot(&self) -> OverlaySnapshot {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }

    fn ensure_started(&self) {
        if self
            .started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        thread::spawn(overlay_thread);
        for _ in 0..100 {
            if self.hwnd.load(Ordering::SeqCst) != 0 {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn request_redraw(&self) {
        let raw = self.hwnd.load(Ordering::SeqCst);
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

static OVERLAY: LazyLock<OverlayController> = LazyLock::new(OverlayController::new);

pub(crate) fn show(
    origin_x: i32,
    origin_y: i32,
    width: i32,
    height: i32,
    labels: Vec<OverlayLabel>,
) {
    OVERLAY.update(origin_x, origin_y, width, height, labels);
}

pub(crate) fn hide() {
    OVERLAY.hide();
}

struct RenderResources {
    render_target: ID2D1DCRenderTarget,
    dwrite_factory: IDWriteFactory,
    text_format: IDWriteTextFormat,
    title_format: IDWriteTextFormat,
    outline_brush: ID2D1SolidColorBrush,
    title_brush: ID2D1SolidColorBrush,
    positive_brush: ID2D1SolidColorBrush,
    negative_brush: ID2D1SolidColorBrush,
    unavailable_brush: ID2D1SolidColorBrush,
    memory_dc: HDC,
    memory_bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    width: i32,
    height: i32,
    font_size: i32,
}

impl RenderResources {
    unsafe fn new(width: i32, height: i32, font_size: i32) -> Result<Self> {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let screen_dc = GetDC(None);
        let memory_dc = CreateCompatibleDC(Some(screen_dc));
        let _ = ReleaseDC(None, screen_dc);

        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = std::ptr::null_mut();
        let memory_bitmap = CreateDIBSection(
            Some(memory_dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut pixels,
            None,
            0,
        )?;
        let old_bitmap = SelectObject(memory_dc, memory_bitmap.into());

        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let render_target = factory.CreateDCRenderTarget(&properties)?;
        let target_rect = windows::Win32::Foundation::RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        render_target.BindDC(memory_dc, &target_rect)?;

        let outline_brush = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.01,
                g: 0.02,
                b: 0.03,
                a: 0.95,
            },
            None,
        )?;
        let title_brush = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.95,
                g: 0.95,
                b: 0.95,
                a: 1.0,
            },
            None,
        )?;
        let positive_brush = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 0.27,
                g: 0.91,
                b: 0.55,
                a: 1.0,
            },
            None,
        )?;
        let negative_brush = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 0.36,
                b: 0.36,
                a: 1.0,
            },
            None,
        )?;
        let unavailable_brush = render_target.CreateSolidColorBrush(
            &D2D1_COLOR_F {
                r: 1.0,
                g: 0.78,
                b: 0.22,
                a: 1.0,
            },
            None,
        )?;

        let dwrite_factory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let text_format = create_text_format(&dwrite_factory, font_size)?;
        let title_format = create_text_format(&dwrite_factory, (font_size - 6).clamp(14, 22))?;

        Ok(Self {
            render_target,
            dwrite_factory,
            text_format,
            title_format,
            outline_brush,
            title_brush,
            positive_brush,
            negative_brush,
            unavailable_brush,
            memory_dc,
            memory_bitmap,
            old_bitmap,
            width,
            height,
            font_size,
        })
    }

    unsafe fn resize_if_needed(&mut self, width: i32, height: i32, font_size: i32) -> Result<()> {
        if self.width == width && self.height == height && self.font_size == font_size {
            return Ok(());
        }
        let replacement = Self::new(width, height, font_size)?;
        let previous = std::mem::replace(self, replacement);
        previous.destroy();
        Ok(())
    }

    unsafe fn destroy(self) {
        let _ = SelectObject(self.memory_dc, self.old_bitmap);
        let _ = DeleteObject(self.memory_bitmap.into());
        let _ = DeleteDC(self.memory_dc);
    }
}

unsafe fn create_text_format(
    dwrite_factory: &IDWriteFactory,
    font_size: i32,
) -> Result<IDWriteTextFormat> {
    dwrite_factory.CreateTextFormat(
        w!("Microsoft YaHei UI"),
        None,
        DWRITE_FONT_WEIGHT_BOLD,
        DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_STRETCH_NORMAL,
        font_size as f32,
        w!("zh-CN"),
    )
}

fn overlay_thread() {
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let class_name = w!("LolStatsHexWinrateOverlay");
        let instance = match GetModuleHandleW(None) {
            Ok(instance) => instance,
            Err(_) => {
                OVERLAY.hwnd.store(-1, Ordering::SeqCst);
                return;
            }
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassW(&class);

        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
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
            Err(_) => {
                OVERLAY.hwnd.store(-1, Ordering::SeqCst);
                return;
            }
        };
        OVERLAY.hwnd.store(hwnd.0 as isize, Ordering::SeqCst);

        let mut resources: Option<RenderResources> = None;
        apply_snapshot(hwnd, &mut resources);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if message.message == UPDATE_MESSAGE {
                apply_snapshot(hwnd, &mut resources);
            } else {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        if let Some(resources) = resources {
            resources.destroy();
        }
    }
}

unsafe fn apply_snapshot(hwnd: HWND, resources: &mut Option<RenderResources>) {
    let snapshot = OVERLAY.snapshot();
    if snapshot.labels.is_empty() || snapshot.width <= 0 || snapshot.height <= 0 {
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
    }

    let font_size = (snapshot.width as f32 * 0.014).round().clamp(20.0, 34.0) as i32;
    let prepared = match resources.as_mut() {
        Some(existing) => existing.resize_if_needed(snapshot.width, snapshot.height, font_size),
        None => {
            *resources = RenderResources::new(snapshot.width, snapshot.height, font_size).ok();
            if resources.is_none() {
                return;
            }
            Ok(())
        }
    };
    if prepared.is_err() {
        return;
    }

    let Some(resources) = resources.as_mut() else {
        return;
    };
    if render_snapshot(resources, &snapshot).is_err() {
        return;
    }

    let destination = POINT {
        x: snapshot.origin_x,
        y: snapshot.origin_y,
    };
    let size = SIZE {
        cx: snapshot.width,
        cy: snapshot.height,
    };
    let source = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        None,
        Some(&destination),
        Some(&size),
        Some(resources.memory_dc),
        Some(&source),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
    // League 在全屏优化或无边框全屏切换时可能会重新占据顶层。创建窗口时的
    // WS_EX_TOPMOST 不足以保证后续层级，因此每次叠层刷新后都重新置顶。
    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        0,
        0,
        0,
        0,
        SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
    );
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
}

unsafe fn render_snapshot(
    resources: &mut RenderResources,
    snapshot: &OverlaySnapshot,
) -> Result<()> {
    resources.render_target.BeginDraw();
    resources.render_target.Clear(Some(&D2D1_COLOR_F {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    }));

    for label in &snapshot.labels {
        draw_label(resources, label)?;
    }

    resources.render_target.EndDraw(None, None)?;
    Ok(())
}

unsafe fn draw_label(resources: &RenderResources, label: &OverlayLabel) -> Result<()> {
    if label.text.is_empty() {
        return Ok(());
    }
    let text = label.text.encode_utf16().collect::<Vec<_>>();
    let layout: IDWriteTextLayout = resources.dwrite_factory.CreateTextLayout(
        &text,
        &resources.text_format,
        LABEL_LAYOUT_WIDTH,
        LABEL_LAYOUT_HEIGHT,
    )?;
    let mut metrics = DWRITE_TEXT_METRICS::default();
    layout.GetMetrics(&mut metrics)?;
    let origin = Vector2 {
        X: label.center_x as f32 - metrics.width / 2.0,
        Y: label.y as f32 - metrics.height / 2.0,
    };

    if let Some(title) = label
        .title
        .as_ref()
        .filter(|title| !title.text.trim().is_empty() && title.max_width > 0)
    {
        let (title_layout, title_metrics) =
            create_title_layout(&resources.dwrite_factory, &resources.title_format, title)?;
        draw_outlined_layout(
            resources,
            &title_layout,
            Vector2 {
                X: label.center_x as f32 - title_metrics.width / 2.0,
                Y: origin.Y - title_metrics.height - 3.0,
            },
            &resources.title_brush,
        );
    }

    let brush = match label.tone {
        OverlayLabelTone::Positive => &resources.positive_brush,
        OverlayLabelTone::Negative => &resources.negative_brush,
        OverlayLabelTone::Unavailable => &resources.unavailable_brush,
    };
    draw_outlined_layout(resources, &layout, origin, brush);
    Ok(())
}

unsafe fn create_title_layout(
    factory: &IDWriteFactory,
    format: &IDWriteTextFormat,
    title: &OverlayTitle,
) -> Result<(IDWriteTextLayout, DWRITE_TEXT_METRICS)> {
    let text = title.text.trim().encode_utf16().collect::<Vec<_>>();
    let max_width = title.max_width.max(1) as f32;
    let layout = factory.CreateTextLayout(&text, format, max_width, LABEL_LAYOUT_HEIGHT)?;
    layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
    let mut metrics = DWRITE_TEXT_METRICS::default();
    layout.GetMetrics(&mut metrics)?;
    // 长名称限制在本张卡的标题宽度内，收益率仍保留原来的字号和位置。
    if metrics.width > max_width {
        layout.SetFontSize(
            format.GetFontSize() * max_width / metrics.width,
            DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: text.len() as u32,
            },
        )?;
        layout.GetMetrics(&mut metrics)?;
    }
    Ok((layout, metrics))
}

unsafe fn draw_outlined_layout(
    resources: &RenderResources,
    layout: &IDWriteTextLayout,
    origin: Vector2,
    brush: &ID2D1SolidColorBrush,
) {
    for (offset_x, offset_y) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        resources.render_target.DrawTextLayout(
            Vector2 {
                X: origin.X + offset_x,
                Y: origin.Y + offset_y,
            },
            layout,
            &resources.outline_brush,
            Default::default(),
        );
    }
    resources
        .render_target
        .DrawTextLayout(origin, layout, brush, Default::default());
}

extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_titles_fit_the_card_width_on_one_line() {
        unsafe {
            let factory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).unwrap();
            let format = create_text_format(&factory, 22).unwrap();
            for max_width in [96, 210, 280] {
                for text in ["暴击飞弹", "灵巧", "任务：沃格勒特的巫师帽", "回力OK镖"]
                {
                    let (_, metrics) = create_title_layout(
                        &factory,
                        &format,
                        &OverlayTitle {
                            text: text.to_string(),
                            max_width,
                        },
                    )
                    .unwrap();
                    assert_eq!(metrics.lineCount, 1, "{text}");
                    assert!(metrics.width > 0.0 && metrics.height > 0.0);
                    assert!(metrics.width <= max_width as f32 + 0.1, "{text}");
                }
            }
        }
    }

    #[test]
    #[ignore = "手动生成原生叠层预览，不显示窗口或操作游戏"]
    fn manual_render_ocr_overlay_preview() {
        use windows::Win32::Graphics::Gdi::{GetObjectW, BITMAP};

        unsafe {
            let snapshot = OverlaySnapshot {
                width: 960,
                height: 180,
                labels: [
                    ("暴击飞弹", "+2.0%", OverlayLabelTone::Positive),
                    ("灵巧", "-1.5%", OverlayLabelTone::Negative),
                    ("升级：耀光", "选取率过低", OverlayLabelTone::Unavailable),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (title, text, tone))| OverlayLabel {
                    center_x: 160 + index as i32 * 320,
                    y: 120,
                    title: Some(OverlayTitle {
                        text: title.to_string(),
                        max_width: 210,
                    }),
                    text: text.to_string(),
                    tone,
                })
                .collect(),
                ..Default::default()
            };
            let mut resources = RenderResources::new(snapshot.width, snapshot.height, 30).unwrap();
            render_snapshot(&mut resources, &snapshot).unwrap();

            let mut bitmap = BITMAP::default();
            assert_ne!(
                GetObjectW(
                    resources.memory_bitmap.into(),
                    std::mem::size_of::<BITMAP>() as i32,
                    Some((&mut bitmap as *mut BITMAP).cast()),
                ),
                0
            );
            assert!(!bitmap.bmBits.is_null());
            let pixels = std::slice::from_raw_parts(
                bitmap.bmBits.cast::<u8>(),
                (snapshot.width * snapshot.height * 4) as usize,
            );
            for left in [0, 320, 640] {
                for (top, bottom) in [(55, 98), (99, 145)] {
                    assert!((top..bottom).any(|y| (left..left + 320)
                        .any(|x| pixels[((y * snapshot.width + x) * 4 + 3) as usize] > 0)));
                }
            }
            let mut preview = image::RgbImage::new(snapshot.width as u32, snapshot.height as u32);
            for (pixel, bgra) in preview.pixels_mut().zip(pixels.chunks_exact(4)) {
                let background = 31 * (255 - bgra[3] as u16) / 255;
                *pixel = image::Rgb([
                    (bgra[2] as u16 + background).min(255) as u8,
                    (bgra[1] as u16 + background).min(255) as u8,
                    (bgra[0] as u16 + background).min(255) as u8,
                ]);
            }
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/ocr-overlay-preview.png");
            preview.save(&path).unwrap();
            resources.destroy();
            println!("叠层渲染预览：{}", path.display());
        }
    }
}
