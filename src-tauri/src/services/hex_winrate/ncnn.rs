#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_void, CStr, CString};

pub type ncnn_allocator_t = *mut c_void;
pub type ncnn_option_t = *mut c_void;
pub type ncnn_mat_t = *mut c_void;
pub type ncnn_net_t = *mut c_void;
pub type ncnn_extractor_t = *mut c_void;

pub const NCNN_MAT_PIXEL_BGR: i32 = 2;

#[link(name = "ncnn", kind = "static")]
extern "C" {
    fn ncnn_option_create() -> ncnn_option_t;
    fn ncnn_option_destroy(opt: ncnn_option_t);
    fn ncnn_option_set_num_threads(opt: ncnn_option_t, num_threads: i32);
    fn ncnn_option_set_use_vulkan_compute(opt: ncnn_option_t, use_vulkan_compute: i32);

    fn ncnn_net_create() -> ncnn_net_t;
    fn ncnn_net_destroy(net: ncnn_net_t);
    fn ncnn_net_set_option(net: ncnn_net_t, opt: ncnn_option_t);
    fn ncnn_net_load_param_memory(net: ncnn_net_t, mem: *const c_char) -> i32;
    fn ncnn_net_load_model_memory(net: ncnn_net_t, mem: *const u8) -> i32;

    fn ncnn_extractor_create(net: ncnn_net_t) -> ncnn_extractor_t;
    fn ncnn_extractor_destroy(ex: ncnn_extractor_t);
    fn ncnn_extractor_set_option(ex: ncnn_extractor_t, opt: ncnn_option_t);
    fn ncnn_extractor_input(ex: ncnn_extractor_t, name: *const c_char, mat: ncnn_mat_t) -> i32;
    fn ncnn_extractor_extract(
        ex: ncnn_extractor_t,
        name: *const c_char,
        mat: *mut ncnn_mat_t,
    ) -> i32;

    fn ncnn_mat_destroy(mat: ncnn_mat_t);
    fn ncnn_mat_get_dims(mat: ncnn_mat_t) -> i32;
    fn ncnn_mat_get_w(mat: ncnn_mat_t) -> i32;
    fn ncnn_mat_get_h(mat: ncnn_mat_t) -> i32;
    fn ncnn_mat_get_d(mat: ncnn_mat_t) -> i32;
    fn ncnn_mat_get_c(mat: ncnn_mat_t) -> i32;
    fn ncnn_mat_get_elemsize(mat: ncnn_mat_t) -> usize;
    fn ncnn_mat_get_elempack(mat: ncnn_mat_t) -> i32;
    fn ncnn_mat_get_data(mat: ncnn_mat_t) -> *mut c_void;
    fn ncnn_mat_get_channel_data(mat: ncnn_mat_t, c: i32) -> *mut c_void;

    fn ncnn_mat_from_pixels(
        pixels: *const u8,
        type_: i32,
        w: i32,
        h: i32,
        stride: i32,
        allocator: ncnn_allocator_t,
    ) -> ncnn_mat_t;
    fn ncnn_mat_substract_mean_normalize(
        mat: ncnn_mat_t,
        mean_vals: *const f32,
        norm_vals: *const f32,
    );
}

fn check(code: i32, context: &str) -> anyhow::Result<()> {
    if code >= 0 {
        Ok(())
    } else {
        anyhow::bail!("ncnn {context} 失败，错误码 {code}");
    }
}

pub struct Net {
    net: ncnn_net_t,
    option: ncnn_option_t,
}

impl Net {
    pub fn new(num_threads: i32) -> anyhow::Result<Self> {
        unsafe {
            let net = ncnn_net_create();
            if net.is_null() {
                anyhow::bail!("ncnn_net_create 返回空指针");
            }

            let option = ncnn_option_create();
            if option.is_null() {
                ncnn_net_destroy(net);
                anyhow::bail!("ncnn_option_create 返回空指针");
            }

            ncnn_option_set_num_threads(option, num_threads.max(1));
            ncnn_option_set_use_vulkan_compute(option, 0);
            ncnn_net_set_option(net, option);
            Ok(Self { net, option })
        }
    }

    pub fn load_from_memory(&self, param_cstr: &[u8], model: &[u8]) -> anyhow::Result<()> {
        if !param_cstr.ends_with(&[0]) {
            anyhow::bail!("ncnn param 必须以 NUL 结尾");
        }

        unsafe {
            check(
                ncnn_net_load_param_memory(self.net, param_cstr.as_ptr() as *const c_char),
                "load_param_memory",
            )?;
            check(
                ncnn_net_load_model_memory(self.net, model.as_ptr()),
                "load_model_memory",
            )?;
        }
        Ok(())
    }

    pub fn create_extractor(&self) -> anyhow::Result<Extractor> {
        unsafe {
            let extractor = ncnn_extractor_create(self.net);
            if extractor.is_null() {
                anyhow::bail!("ncnn_extractor_create 返回空指针");
            }
            ncnn_extractor_set_option(extractor, self.option);
            Ok(Extractor { extractor })
        }
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        unsafe {
            if !self.net.is_null() {
                ncnn_net_destroy(self.net);
            }
            if !self.option.is_null() {
                ncnn_option_destroy(self.option);
            }
        }
    }
}

pub struct Extractor {
    extractor: ncnn_extractor_t,
}

impl Extractor {
    pub fn input(&self, name: &CStr, mat: &Mat) -> anyhow::Result<()> {
        unsafe {
            check(
                ncnn_extractor_input(self.extractor, name.as_ptr(), mat.mat),
                "extractor_input",
            )
        }
    }

    pub fn extract(&self, name: &CStr) -> anyhow::Result<Mat> {
        unsafe {
            let mut output: ncnn_mat_t = std::ptr::null_mut();
            check(
                ncnn_extractor_extract(self.extractor, name.as_ptr(), &mut output),
                "extractor_extract",
            )?;
            if output.is_null() {
                anyhow::bail!("ncnn extractor 返回空 Mat");
            }
            Ok(Mat { mat: output })
        }
    }
}

impl Drop for Extractor {
    fn drop(&mut self) {
        unsafe {
            if !self.extractor.is_null() {
                ncnn_extractor_destroy(self.extractor);
            }
        }
    }
}

pub struct Mat {
    mat: ncnn_mat_t,
}

impl Mat {
    pub fn from_pixels_bgr(
        pixels: &[u8],
        width: i32,
        height: i32,
        stride: i32,
    ) -> anyhow::Result<Self> {
        unsafe {
            let mat = ncnn_mat_from_pixels(
                pixels.as_ptr(),
                NCNN_MAT_PIXEL_BGR,
                width,
                height,
                stride,
                std::ptr::null_mut(),
            );
            if mat.is_null() {
                anyhow::bail!("ncnn_mat_from_pixels 返回空指针");
            }
            Ok(Self { mat })
        }
    }

    pub fn normalize_inplace(&mut self, mean: [f32; 3], norm: [f32; 3]) {
        unsafe {
            ncnn_mat_substract_mean_normalize(self.mat, mean.as_ptr(), norm.as_ptr());
        }
    }

    pub fn shape(&self) -> MatShape {
        unsafe {
            MatShape {
                dims: ncnn_mat_get_dims(self.mat),
                w: ncnn_mat_get_w(self.mat),
                h: ncnn_mat_get_h(self.mat),
                d: ncnn_mat_get_d(self.mat),
                c: ncnn_mat_get_c(self.mat),
                elemsize: ncnn_mat_get_elemsize(self.mat),
                elempack: ncnn_mat_get_elempack(self.mat),
            }
        }
    }

    pub unsafe fn data_ptr(&self) -> *const f32 {
        ncnn_mat_get_data(self.mat) as *const f32
    }

    pub unsafe fn channel_ptr(&self, channel: i32) -> *const f32 {
        ncnn_mat_get_channel_data(self.mat, channel) as *const f32
    }
}

impl Drop for Mat {
    fn drop(&mut self) {
        unsafe {
            if !self.mat.is_null() {
                ncnn_mat_destroy(self.mat);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatShape {
    pub dims: i32,
    pub w: i32,
    pub h: i32,
    pub d: i32,
    pub c: i32,
    pub elemsize: usize,
    pub elempack: i32,
}

pub fn cstr(value: &str) -> CString {
    CString::new(value).expect("NCNN 节点名不能包含 NUL")
}
