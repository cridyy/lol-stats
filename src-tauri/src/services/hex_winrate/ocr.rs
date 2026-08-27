use image::{imageops::FilterType, RgbImage};

use super::ncnn;

const MODEL_PARAM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/ocr/PP-OCRv5_mobile_rec.param"
));
const MODEL_BIN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/ocr/PP-OCRv5_mobile_rec.bin"
));
const VOCAB: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/ocr/PP-OCRv5_vocab.txt"
));

pub(crate) struct PpOcrRec {
    net: ncnn::Net,
    input_name: std::ffi::CString,
    output_name: std::ffi::CString,
    _param_cstr: Vec<u8>,
    _model_bin: Vec<u8>,
    keys: Vec<String>,
    expected_class_count: usize,
}

impl PpOcrRec {
    pub(crate) fn from_embedded_model() -> anyhow::Result<Self> {
        let mut param_cstr = Vec::with_capacity(MODEL_PARAM.len() + 1);
        param_cstr.extend_from_slice(MODEL_PARAM);
        param_cstr.push(0);

        let model_bin = MODEL_BIN.to_vec();
        let vocab = split_character_lines(VOCAB.strip_prefix('\u{feff}').unwrap_or(VOCAB));
        if vocab.is_empty() {
            anyhow::bail!("PP-OCRv5 字符表为空");
        }

        let expected_class_count = vocab.len() + 2;
        let keys = ensure_keys_len(vocab, expected_class_count);

        let net = ncnn::Net::new(1)?;
        net.load_from_memory(&param_cstr, &model_bin)?;

        Ok(Self {
            net,
            input_name: ncnn::cstr("in0"),
            output_name: ncnn::cstr("out0"),
            _param_cstr: param_cstr,
            _model_bin: model_bin,
            expected_class_count,
            keys,
        })
    }

    pub(crate) fn recognize_rgb(&self, image: &RgbImage) -> anyhow::Result<String> {
        let (mut input, _) = preprocess_bgr(image, 48)?;
        input.normalize_inplace([127.5, 127.5, 127.5], [1.0 / 127.5; 3]);

        let extractor = self.net.create_extractor()?;
        extractor.input(self.input_name.as_c_str(), &input)?;
        let output = extractor.extract(self.output_name.as_c_str())?;
        let shape = output.shape();
        let (time_steps, class_count, layout) = infer_layout(&shape, self.expected_class_count);
        if class_count != self.expected_class_count {
            anyhow::bail!(
                "PP-OCRv5 模型类别数不匹配：输出 {class_count}，词表期望 {}",
                self.expected_class_count
            );
        }

        let text = decode_ctc(&output, &self.keys, time_steps, class_count, layout, 0.5)?;
        Ok(text.to_ascii_uppercase())
    }
}

#[derive(Debug, Clone, Copy)]
enum RecLayout {
    WhRowMajor,
    HwRowMajor,
    ChannelMajor,
}

fn infer_layout(shape: &ncnn::MatShape, expected_class_count: usize) -> (usize, usize, RecLayout) {
    let width = shape.w.max(0) as usize;
    let height = shape.h.max(0) as usize;
    let depth = shape.d.max(0) as usize;
    let channels = shape.c.max(0) as usize;
    let class_count = [
        expected_class_count,
        expected_class_count.saturating_sub(1),
        expected_class_count.saturating_sub(2),
        438,
    ]
    .into_iter()
    .find(|candidate| {
        *candidate > 0
            && (width == *candidate
                || height == *candidate
                || depth == *candidate
                || channels == *candidate)
    })
    .unwrap_or_else(|| width.max(height).max(depth).max(channels).max(1));

    match shape.dims {
        2 => {
            if width == class_count {
                (height.max(1), class_count, RecLayout::WhRowMajor)
            } else {
                (width.max(1), class_count, RecLayout::HwRowMajor)
            }
        }
        3 => {
            if width == class_count && channels == 1 {
                (height.max(1), class_count, RecLayout::WhRowMajor)
            } else if width == class_count && height == 1 {
                (channels.max(1), class_count, RecLayout::ChannelMajor)
            } else if height == class_count && channels == 1 {
                (width.max(1), class_count, RecLayout::HwRowMajor)
            } else {
                (height.max(1), class_count, RecLayout::WhRowMajor)
            }
        }
        _ => (height.max(1), class_count, RecLayout::WhRowMajor),
    }
}

fn decode_ctc(
    output: &ncnn::Mat,
    keys: &[String],
    time_steps: usize,
    class_count: usize,
    layout: RecLayout,
    threshold: f32,
) -> anyhow::Result<String> {
    let shape = output.shape();
    if shape.elemsize != 4 || shape.elempack != 1 {
        anyhow::bail!("不支持的 OCR 输出格式");
    }

    let mut result = String::new();
    let mut last_index = 0usize;

    match layout {
        RecLayout::WhRowMajor => unsafe {
            let width = shape.w.max(0) as usize;
            let height = shape.h.max(0) as usize;
            let rows = time_steps.min(height.max(1));
            let classes = class_count.min(width.max(1));
            let data = output.data_ptr();

            for time in 0..rows {
                let row = std::slice::from_raw_parts(data.add(time * width), width);
                let (index, probability) = argmax(row, classes);
                push_ctc_token(
                    &mut result,
                    keys,
                    &mut last_index,
                    index,
                    probability,
                    threshold,
                );
            }
        },
        RecLayout::HwRowMajor => unsafe {
            let width = shape.w.max(0) as usize;
            let height = shape.h.max(0) as usize;
            let steps = time_steps.min(width.max(1));
            let classes = class_count.min(height.max(1));
            let data = output.data_ptr();

            for time in 0..steps {
                let mut best_index = 0usize;
                let mut best_probability = f32::NEG_INFINITY;
                for class in 0..classes {
                    let probability = *data.add(class * width + time);
                    if probability > best_probability {
                        best_probability = probability;
                        best_index = class;
                    }
                }
                push_ctc_token(
                    &mut result,
                    keys,
                    &mut last_index,
                    best_index,
                    best_probability,
                    threshold,
                );
            }
        },
        RecLayout::ChannelMajor => unsafe {
            let width = shape.w.max(0) as usize;
            let channels = shape.c.max(0) as usize;
            let steps = time_steps.min(channels.max(1));
            let classes = class_count.min(width.max(1));

            for time in 0..steps {
                let row = std::slice::from_raw_parts(output.channel_ptr(time as i32), width);
                let (index, probability) = argmax(row, classes);
                push_ctc_token(
                    &mut result,
                    keys,
                    &mut last_index,
                    index,
                    probability,
                    threshold,
                );
            }
        },
    }

    Ok(result)
}

fn push_ctc_token(
    result: &mut String,
    keys: &[String],
    last_index: &mut usize,
    index: usize,
    probability: f32,
    threshold: f32,
) {
    if index > 0 && index != *last_index && probability > threshold {
        if let Some(token) = keys.get(index) {
            result.push_str(token);
        }
    }
    *last_index = index;
}

fn argmax(row: &[f32], class_count: usize) -> (usize, f32) {
    let mut index = 0usize;
    let mut value = f32::NEG_INFINITY;
    for (candidate, probability) in row.iter().take(class_count).copied().enumerate() {
        if probability > value {
            value = probability;
            index = candidate;
        }
    }
    (index, value)
}

fn split_character_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

fn ensure_keys_len(mut keys: Vec<String>, class_count: usize) -> Vec<String> {
    if keys.len() < class_count {
        keys.insert(0, "blank".to_string());
        while keys.len() < class_count {
            keys.push(" ".to_string());
        }
    } else if keys.len() > class_count {
        keys.truncate(class_count);
    }
    keys
}

fn preprocess_bgr(image: &RgbImage, input_height: u32) -> anyhow::Result<(ncnn::Mat, u32)> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        anyhow::bail!("OCR 输入图像尺寸非法：{width}x{height}");
    }

    let resized_width = ((input_height as f32 * width as f32 / height as f32).ceil() as u32).max(1);
    let resized = image::imageops::resize(image, resized_width, input_height, FilterType::Lanczos3);
    let mut pixels = resized.into_raw();
    for pixel in pixels.chunks_exact_mut(3) {
        pixel.swap(0, 2);
    }

    let mat = ncnn::Mat::from_pixels_bgr(
        &pixels,
        resized_width as i32,
        input_height as i32,
        resized_width as i32 * 3,
    )?;
    Ok((mat, resized_width))
}
