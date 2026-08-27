use anyhow::{anyhow, Result};
use windows::core::Interface;
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;

pub(crate) fn create_d3d11_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    unsafe {
        let feature_levels: [D3D_FEATURE_LEVEL; 2] =
            [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];

        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let mut chosen_level = D3D_FEATURE_LEVEL_11_0;
        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;

        let hardware_result = D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE(std::ptr::null_mut()),
            flags,
            Some(&feature_levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut chosen_level),
            Some(&mut context),
        );
        if hardware_result.is_err() {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE(std::ptr::null_mut()),
                flags,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut chosen_level),
                Some(&mut context),
            )
            .map_err(|error| anyhow!("D3D11CreateDevice(WARP) 失败: {error}"))?;
        }

        let device = device.ok_or_else(|| anyhow!("D3D11CreateDevice 返回空 device"))?;
        let context = context.ok_or_else(|| anyhow!("D3D11CreateDevice 返回空 context"))?;
        Ok((device, context))
    }
}

pub(crate) fn create_direct3d_device(d3d_device: &ID3D11Device) -> Result<IDirect3DDevice> {
    let dxgi_device: IDXGIDevice = d3d_device.cast().map_err(|error| anyhow!("{error}"))?;
    let inspectable = unsafe {
        CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device).map_err(|error| anyhow!("{error}"))?
    };
    inspectable.cast().map_err(|error| anyhow!("{error}"))
}

pub(crate) struct StagingTexture {
    texture: ID3D11Texture2D,
    desc: D3D11_TEXTURE2D_DESC,
}

impl StagingTexture {
    pub(crate) fn new(
        device: &ID3D11Device,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
    ) -> Result<Self> {
        unsafe {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut texture: Option<ID3D11Texture2D> = None;
            device
                .CreateTexture2D(&desc, None, Some(&mut texture))
                .map_err(|error| anyhow!("创建 staging 纹理失败: {error}"))?;
            let texture = texture.ok_or_else(|| anyhow!("CreateTexture2D 返回空 texture"))?;
            Ok(Self { texture, desc })
        }
    }

    pub(crate) fn desc(&self) -> &D3D11_TEXTURE2D_DESC {
        &self.desc
    }

    pub(crate) fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }
}
