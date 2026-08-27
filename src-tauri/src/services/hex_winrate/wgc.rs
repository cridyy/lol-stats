use super::d3d11::{create_d3d11_device, create_direct3d_device, StagingTexture};
use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use windows::core::{Interface, HRESULT};
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DSurface;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HWND, POINT, RECT, S_FALSE};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_BOX, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_TEXTURE2D_DESC,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::CoIncrementMTAUsage;
use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::{Graphics::Dwm::*, UI::WindowsAndMessaging::*};

const HRESULT_E_PENDING: HRESULT = HRESULT(0x8000000A_u32 as _);
const HRESULT_OK: HRESULT = HRESULT(0);

#[derive(Debug, Clone, Copy)]
pub(crate) struct WgcFrame {
    pub(crate) origin_x: i32,
    pub(crate) origin_y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

struct SharedState {
    seq: u64,
    frame: Option<WgcFrame>,
    bgra: Vec<u8>,
    closed: bool,
    error: Option<String>,
}

struct Shared {
    state: Mutex<SharedState>,
    changed: Condvar,
}

pub(crate) struct WgcState {
    shared: Shared,
    stop: AtomicBool,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Drop for WgcState {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut state) = self.shared.state.lock() {
            state.closed = true;
        }
        self.shared.changed.notify_all();
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(handle) = worker.take() {
                let _ = handle.join();
            }
        }
    }
}

/// 创建一个绑定到指定 HWND 的 Windows Graphics Capture 会话。
///
/// 初始化在后台线程完成，避免识别循环因为 D3D/WGC 首帧等待而阻塞；调用方
/// 通过 `capture_wgc_into` 等待首帧或读取最近一帧。
pub(crate) fn init_wgc_state(hwnd: isize) -> Result<Arc<WgcState>> {
    if hwnd == 0 {
        return Err(anyhow!("目标 HWND 为空"));
    }

    let state = Arc::new(WgcState {
        shared: Shared {
            state: Mutex::new(SharedState {
                seq: 0,
                frame: None,
                bgra: Vec::new(),
                closed: false,
                error: None,
            }),
            changed: Condvar::new(),
        },
        stop: AtomicBool::new(false),
        worker: Mutex::new(None),
    });

    let state_for_thread = Arc::clone(&state);
    let worker = thread::spawn(move || {
        if let Err(error) = wgc_worker_loop(Arc::clone(&state_for_thread), hwnd) {
            close_with_error(&state_for_thread, error.to_string());
        }
    });
    *state
        .worker
        .lock()
        .map_err(|_| anyhow!("WGC worker mutex poisoned"))? = Some(worker);

    Ok(state)
}

/// 读取一帧 BGRA 数据。WGC 在画面静止时不一定产生新帧，因此超时后会复用最近一帧。
pub(crate) fn capture_wgc_into(
    state: &Arc<WgcState>,
    last_seq: u64,
    timeout_ms: u32,
    out: &mut Vec<u8>,
) -> Result<Option<(WgcFrame, u64)>> {
    let timeout = Duration::from_millis(timeout_ms as u64);
    let start = Instant::now();
    let mut guard = state
        .shared
        .state
        .lock()
        .map_err(|_| anyhow!("WGC shared mutex poisoned"))?;

    while guard.seq <= last_seq && guard.seq == 0 && !guard.closed {
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            return Ok(None);
        }
        let remaining = timeout.saturating_sub(elapsed);
        let (next, wait_result) = state
            .shared
            .changed
            .wait_timeout(guard, remaining)
            .map_err(|_| anyhow!("WGC shared mutex poisoned"))?;
        guard = next;
        if wait_result.timed_out() && guard.seq == 0 {
            return Ok(None);
        }
    }

    if guard.closed && guard.seq == 0 {
        return Err(anyhow!(
            "WGC 捕获线程已结束：{}",
            guard.error.as_deref().unwrap_or("没有收到有效帧")
        ));
    }

    let Some(frame) = guard.frame else {
        return Ok(None);
    };
    if out.len() != guard.bgra.len() {
        out.resize(guard.bgra.len(), 0);
    }
    out.copy_from_slice(&guard.bgra);
    Ok(Some((frame, guard.seq)))
}

fn wgc_worker_loop(state: Arc<WgcState>, hwnd_value: isize) -> Result<()> {
    init_winrt_mta()?;
    if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
        return Err(anyhow!("GraphicsCaptureSession::IsSupported == false"));
    }

    let (d3d_device, d3d_context) = create_d3d11_device()?;
    let direct3d_device = create_direct3d_device(&d3d_device)?;
    let hwnd = HWND(hwnd_value as _);
    let interop: IGraphicsCaptureItemInterop =
        windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(|error| anyhow!("创建 GraphicsCaptureItem 工厂失败：{error}"))?;
    let item: GraphicsCaptureItem = unsafe {
        interop
            .CreateForWindow(hwnd)
            .map_err(|error| anyhow!("按 HWND 创建 GraphicsCaptureItem 失败：{error}"))?
    };

    let pixel_format = DirectXPixelFormat::B8G8R8A8UIntNormalized;
    let mut last_size = item
        .Size()
        .map_err(|error| anyhow!("读取 WGC 窗口尺寸失败：{error}"))?;
    if last_size.Width <= 0 || last_size.Height <= 0 {
        return Err(anyhow!("WGC 窗口尺寸无效"));
    }

    let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &direct3d_device,
        pixel_format,
        3,
        last_size,
    )
    .map_err(|error| anyhow!("创建 WGC frame pool 失败：{error}"))?;
    let session = frame_pool
        .CreateCaptureSession(&item)
        .map_err(|error| anyhow!("创建 WGC capture session 失败：{error}"))?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    let _ = session.SetIsBorderRequired(false);
    session
        .StartCapture()
        .map_err(|error| anyhow!("启动 WGC 捕获失败：{error}"))?;

    let mut staging: Option<StagingTexture> = None;
    while !state.stop.load(Ordering::Relaxed) {
        let frame = match frame_pool.TryGetNextFrame() {
            Ok(frame) => frame,
            Err(error) if error.code() == HRESULT_E_PENDING || error.code() == HRESULT_OK => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(error) => return Err(anyhow!("TryGetNextFrame 失败：{error}")),
        };

        let content_size = frame
            .ContentSize()
            .map_err(|error| anyhow!("读取 WGC frame 尺寸失败：{error}"))?;
        if content_size.Width <= 0 || content_size.Height <= 0 {
            continue;
        }
        if content_size.Width != last_size.Width || content_size.Height != last_size.Height {
            frame_pool
                .Recreate(&direct3d_device, pixel_format, 3, content_size)
                .map_err(|error| anyhow!("重建 WGC frame pool 失败：{error}"))?;
            last_size = content_size;
        }

        let frame_width = content_size.Width as u32;
        let frame_height = content_size.Height as u32;
        let surface: IDirect3DSurface = frame
            .Surface()
            .map_err(|error| anyhow!("读取 WGC surface 失败：{error}"))?;
        let dxgi_access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|error| anyhow!("获取 WGC DXGI surface 接口失败：{error}"))?;
        let texture: ID3D11Texture2D = unsafe {
            dxgi_access
                .GetInterface()
                .map_err(|error| anyhow!("获取 WGC D3D11 texture 失败：{error}"))?
        };

        let mut description = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut description) };
        let format = description.Format;
        if !matches!(
            format,
            DXGI_FORMAT_B8G8R8A8_UNORM
                | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
                | DXGI_FORMAT_R8G8B8A8_UNORM
                | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        ) {
            return Err(anyhow!("WGC 纹理格式不支持：{:?}", format));
        }

        let (origin_x, origin_y) =
            best_origin_for_capture(hwnd_value, frame_width, frame_height).unwrap_or((0, 0));
        let width = frame_width as usize;
        let height = frame_height as usize;
        let row_bytes = width * 4;

        let need_staging = staging.as_ref().is_none_or(|value| {
            let desc = value.desc();
            desc.Format != format || desc.Width < frame_width || desc.Height < frame_height
        });
        if need_staging {
            staging = Some(StagingTexture::new(
                &d3d_device,
                frame_width,
                frame_height,
                format,
            )?);
        }
        let staging = staging
            .as_ref()
            .ok_or_else(|| anyhow!("WGC staging texture 未初始化"))?;
        let source_box = D3D11_BOX {
            left: 0,
            top: 0,
            front: 0,
            right: frame_width,
            bottom: frame_height,
            back: 1,
        };
        unsafe {
            d3d_context.CopySubresourceRegion(
                staging.texture(),
                0,
                0,
                0,
                0,
                &texture,
                0,
                Some(&source_box),
            );
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            d3d_context
                .Map(staging.texture(), 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|error| anyhow!("WGC Map staging 失败：{error}"))?;
        }
        let row_pitch = mapped.RowPitch as usize;
        if row_pitch < row_bytes {
            unsafe { d3d_context.Unmap(staging.texture(), 0) };
            return Err(anyhow!("WGC row pitch 小于实际行宽"));
        }
        let source =
            unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), height * row_pitch) };
        let mut pixels = vec![0u8; height * row_bytes];
        for row in 0..height {
            let src = &source[row * row_pitch..row * row_pitch + row_bytes];
            let dst = &mut pixels[row * row_bytes..(row + 1) * row_bytes];
            match format {
                DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => {
                    dst.copy_from_slice(src);
                }
                DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => {
                    for (source_pixel, destination_pixel) in
                        src.chunks_exact(4).zip(dst.chunks_exact_mut(4))
                    {
                        destination_pixel[0] = source_pixel[2];
                        destination_pixel[1] = source_pixel[1];
                        destination_pixel[2] = source_pixel[0];
                        destination_pixel[3] = source_pixel[3];
                    }
                }
                _ => unreachable!(),
            }
        }
        unsafe { d3d_context.Unmap(staging.texture(), 0) };

        let mut shared = state
            .shared
            .state
            .lock()
            .map_err(|_| anyhow!("WGC shared mutex poisoned"))?;
        shared.seq = shared.seq.wrapping_add(1);
        shared.frame = Some(WgcFrame {
            origin_x,
            origin_y,
            width: frame_width as i32,
            height: frame_height as i32,
        });
        shared.bgra = pixels;
        drop(shared);
        state.shared.changed.notify_all();
    }

    close_cleanly(&state);
    Ok(())
}

fn close_cleanly(state: &Arc<WgcState>) {
    if let Ok(mut shared) = state.shared.state.lock() {
        shared.closed = true;
    }
    state.shared.changed.notify_all();
}

fn close_with_error(state: &Arc<WgcState>, error: String) {
    if let Ok(mut shared) = state.shared.state.lock() {
        shared.closed = true;
        if shared.error.is_none() {
            shared.error = Some(error);
        }
    }
    state.shared.changed.notify_all();
}

fn init_winrt_mta() -> Result<()> {
    static MTA_READY: OnceLock<()> = OnceLock::new();
    MTA_READY.get_or_init(|| {
        let _ = unsafe { CoIncrementMTAUsage() };
    });
    match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        Ok(_) => Ok(()),
        Err(error) if error.code() == S_FALSE => Ok(()),
        Err(error) => Err(anyhow!("RoInitialize 失败：{error}")),
    }
}

fn best_origin_for_capture(
    hwnd_value: isize,
    frame_width: u32,
    frame_height: u32,
) -> Option<(i32, i32)> {
    let hwnd = HWND(hwnd_value as _);
    let mut candidates = Vec::new();
    if let Some(value) = client_rect_on_screen(hwnd) {
        candidates.push(value);
    }
    if let Some(value) = window_rect(hwnd) {
        candidates.push(value);
    }
    if let Some(value) = extended_frame_rect(hwnd) {
        candidates.push(value);
    }
    if candidates.is_empty() {
        return None;
    }

    let target_width = frame_width as i32;
    let target_height = frame_height as i32;
    candidates
        .into_iter()
        .min_by_key(|(_, (width, height))| {
            (width - target_width).abs() + (height - target_height).abs()
        })
        .map(|(origin, _)| origin)
}

fn client_rect_on_screen(hwnd: HWND) -> Option<((i32, i32), (i32, i32))> {
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).ok()?;
        let mut point = POINT { x: 0, y: 0 };
        ClientToScreen(hwnd, &mut point).as_bool().then_some((
            (point.x, point.y),
            (rect.right - rect.left, rect.bottom - rect.top),
        ))
    }
}

fn window_rect(hwnd: HWND) -> Option<((i32, i32), (i32, i32))> {
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        Some((
            (rect.left, rect.top),
            (rect.right - rect.left, rect.bottom - rect.top),
        ))
    }
}

fn extended_frame_rect(hwnd: HWND) -> Option<((i32, i32), (i32, i32))> {
    unsafe {
        let mut rect = RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            std::ptr::addr_of_mut!(rect).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
        Some((
            (rect.left, rect.top),
            (rect.right - rect.left, rect.bottom - rect.top),
        ))
    }
}
