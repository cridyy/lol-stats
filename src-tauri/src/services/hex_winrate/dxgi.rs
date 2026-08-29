use anyhow::{anyhow, Result};
use windows::core::Interface;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_ACCESS_LOST,
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_DESC, DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};

use super::d3d11::{create_d3d11_device, StagingTexture};

#[derive(Debug, Clone, Copy)]
pub(crate) struct DxgiFrame {
    pub(crate) origin_x: i32,
    pub(crate) origin_y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

/// DXGI Desktop Duplication 状态。
///
/// Desktop Duplication 必须绑定某一块显示器；游戏窗口移动到另一块显示器时，调用方
/// 会重建此状态。像素数据由调用方持有，以便零拷贝地借给后续的三卡检测和 OCR。
pub(crate) struct DxgiState {
    monitor: HMONITOR,
    monitor_rect: RECT,
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    duplication_desc: DXGI_OUTDUPL_DESC,
    staging: Option<StagingTexture>,
    last_frame: Option<DxgiFrame>,
}

impl DxgiState {
    pub(crate) fn new(hwnd_value: isize) -> Result<Self> {
        let hwnd = HWND(hwnd_value as _);
        let monitor = monitor_for_window(hwnd)?;
        let monitor_rect = monitor_rect(monitor)?;
        let (d3d_device, d3d_context) = create_d3d11_device()?;
        let (duplication, duplication_desc) = create_duplication_for_monitor(&d3d_device, monitor)?;

        Ok(Self {
            monitor,
            monitor_rect,
            d3d_device,
            d3d_context,
            duplication,
            duplication_desc,
            staging: None,
            last_frame: None,
        })
    }

    pub(crate) fn matches_window(&self, hwnd_value: isize) -> Result<bool> {
        let monitor = monitor_for_window(HWND(hwnd_value as _))?;
        Ok(monitor.0 == self.monitor.0)
    }

    fn recreate_duplication(&mut self) -> Result<()> {
        let (duplication, duplication_desc) =
            create_duplication_for_monitor(&self.d3d_device, self.monitor)?;
        self.monitor_rect = monitor_rect(self.monitor)?;
        self.duplication = duplication;
        self.duplication_desc = duplication_desc;
        self.staging = None;
        self.last_frame = None;
        Ok(())
    }

    /// 将游戏客户区从桌面复制到调用方复用的 BGRA 缓冲区中。
    ///
    /// Desktop Duplication 在画面没有变化时不会产生新帧。此处返回最近一次成功帧，
    /// 让静态海克斯卡片仍可继续被 OCR 和叠层逻辑处理。
    pub(crate) fn capture_into(
        &mut self,
        requested_origin_x: i32,
        requested_origin_y: i32,
        requested_width: i32,
        requested_height: i32,
        timeout_ms: u32,
        out: &mut Vec<u8>,
    ) -> Result<Option<DxgiFrame>> {
        struct FrameGuard {
            duplication: IDXGIOutputDuplication,
        }

        impl Drop for FrameGuard {
            fn drop(&mut self) {
                unsafe {
                    let _ = self.duplication.ReleaseFrame();
                }
            }
        }

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;
        let acquire = unsafe {
            self.duplication
                .AcquireNextFrame(timeout_ms, &mut frame_info, &mut resource)
        };
        match acquire {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(self.last_frame),
            Err(error) if error.code() == DXGI_ERROR_ACCESS_LOST => {
                self.recreate_duplication()?;
                resource = None;
                match unsafe {
                    self.duplication
                        .AcquireNextFrame(timeout_ms, &mut frame_info, &mut resource)
                } {
                    Ok(()) => {}
                    Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                        return Ok(self.last_frame)
                    }
                    Err(error) => {
                        return Err(anyhow!("DXGI 重建后 AcquireNextFrame 失败: {error}"))
                    }
                }
            }
            Err(error) => return Err(anyhow!("DXGI AcquireNextFrame 失败: {error}")),
        }

        let _guard = FrameGuard {
            duplication: self.duplication.clone(),
        };

        // 仅光标变化时可能得到没有新桌面像素的帧，直接复用上一帧即可。
        if frame_info.LastPresentTime == 0 && frame_info.AccumulatedFrames == 0 {
            return Ok(self.last_frame);
        }

        let Some(resource) = resource else {
            return Err(anyhow!("DXGI AcquireNextFrame 返回空资源"));
        };
        let texture: ID3D11Texture2D = resource.cast().map_err(|error| anyhow!("{error}"))?;

        let frame_width = self.duplication_desc.ModeDesc.Width;
        let frame_height = self.duplication_desc.ModeDesc.Height;
        let format = self.duplication_desc.ModeDesc.Format;
        if !matches!(
            format,
            DXGI_FORMAT_B8G8R8A8_UNORM
                | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
                | DXGI_FORMAT_R8G8B8A8_UNORM
                | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
        ) {
            return Err(anyhow!("不支持的 DXGI 格式: {format:?}"));
        }

        let capture_x1 = i64::from(self.monitor_rect.left);
        let capture_y1 = i64::from(self.monitor_rect.top);
        let capture_x2 = capture_x1 + i64::from(frame_width);
        let capture_y2 = capture_y1 + i64::from(frame_height);
        let requested_x1 = i64::from(requested_origin_x);
        let requested_y1 = i64::from(requested_origin_y);
        let requested_x2 = requested_x1 + i64::from(requested_width);
        let requested_y2 = requested_y1 + i64::from(requested_height);

        let x1 = requested_x1.max(capture_x1);
        let y1 = requested_y1.max(capture_y1);
        let x2 = requested_x2.min(capture_x2);
        let y2 = requested_y2.min(capture_y2);
        if x2 <= x1 || y2 <= y1 {
            return Err(anyhow!("DXGI 截图区域不在游戏窗口所在显示器中"));
        }

        let width = (x2 - x1) as u32;
        let height = (y2 - y1) as u32;
        let frame = DxgiFrame {
            origin_x: x1 as i32,
            origin_y: y1 as i32,
            width: width as i32,
            height: height as i32,
        };
        let local_x = (x1 - capture_x1) as u32;
        let local_y = (y1 - capture_y1) as u32;

        let need_staging = self.staging.as_ref().is_none_or(|staging| {
            let description = staging.desc();
            description.Format != format || description.Width < width || description.Height < height
        });
        if need_staging {
            self.staging = Some(StagingTexture::new(
                &self.d3d_device,
                width,
                height,
                format,
            )?);
        }
        let staging = self
            .staging
            .as_ref()
            .ok_or_else(|| anyhow!("DXGI staging 纹理未初始化"))?;
        let source_box = D3D11_BOX {
            left: local_x,
            top: local_y,
            front: 0,
            right: local_x + width,
            bottom: local_y + height,
            back: 1,
        };
        unsafe {
            self.d3d_context.CopySubresourceRegion(
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
            self.d3d_context
                .Map(staging.texture(), 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|error| anyhow!("DXGI Map staging 失败: {error}"))?;
        }

        let row_pitch = mapped.RowPitch as usize;
        let row_bytes = width as usize * 4;
        if row_pitch < row_bytes {
            unsafe {
                self.d3d_context.Unmap(staging.texture(), 0);
            }
            return Err(anyhow!("DXGI row pitch 小于实际行宽"));
        }

        let expected_length = height as usize * row_bytes;
        out.resize(expected_length, 0);
        let source = unsafe {
            std::slice::from_raw_parts(mapped.pData.cast::<u8>(), height as usize * row_pitch)
        };
        for row in 0..height as usize {
            let source_row = &source[row * row_pitch..row * row_pitch + row_bytes];
            let destination_row = &mut out[row * row_bytes..(row + 1) * row_bytes];
            match format {
                DXGI_FORMAT_B8G8R8A8_UNORM | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB => {
                    destination_row.copy_from_slice(source_row);
                }
                DXGI_FORMAT_R8G8B8A8_UNORM | DXGI_FORMAT_R8G8B8A8_UNORM_SRGB => {
                    for (source_pixel, destination_pixel) in source_row
                        .chunks_exact(4)
                        .zip(destination_row.chunks_exact_mut(4))
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

        unsafe {
            self.d3d_context.Unmap(staging.texture(), 0);
        }
        self.last_frame = Some(frame);
        Ok(Some(frame))
    }
}

fn monitor_for_window(hwnd: HWND) -> Result<HMONITOR> {
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.0.is_null() {
        return Err(anyhow!("无法定位游戏窗口所在显示器"));
    }
    Ok(monitor)
}

fn monitor_rect(monitor: HMONITOR) -> Result<RECT> {
    unsafe {
        let mut info = MONITORINFO::default();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err(anyhow!("GetMonitorInfoW 失败"));
        }
        Ok(info.rcMonitor)
    }
}

fn create_duplication_for_monitor(
    device: &ID3D11Device,
    monitor: HMONITOR,
) -> Result<(IDXGIOutputDuplication, DXGI_OUTDUPL_DESC)> {
    let dxgi_device: IDXGIDevice = device.cast().map_err(|error| anyhow!("{error}"))?;
    let adapter = unsafe {
        dxgi_device
            .GetAdapter()
            .map_err(|error| anyhow!("{error}"))?
    };

    let mut index = 0u32;
    loop {
        let output = unsafe { adapter.EnumOutputs(index) };
        let output = match output {
            Ok(output) => output,
            Err(error) => return Err(anyhow!("未找到对应 HMONITOR 的 DXGI Output: {error}")),
        };
        let description = unsafe { output.GetDesc().map_err(|error| anyhow!("{error}"))? };
        if description.Monitor == monitor {
            let output: IDXGIOutput1 = output.cast().map_err(|error| anyhow!("{error}"))?;
            let duplication = unsafe {
                output
                    .DuplicateOutput(device)
                    .map_err(|error| anyhow!("{error}"))?
            };
            let description = unsafe { duplication.GetDesc() };
            return Ok((duplication, description));
        }
        index = index.saturating_add(1);
    }
}
