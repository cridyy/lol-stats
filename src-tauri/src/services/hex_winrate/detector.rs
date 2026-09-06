use std::cmp::{max, min};

use super::DetectionMode;

// 高精度方案限定卡片底色，R 允许到 3，兼容部分截图的轻微颜色偏移。
const CORE_DARK_LUMA_LIMIT: f32 = 24.0;
const CORE_DARK_RED_LIMIT: u8 = 3;
const CORE_DARK_GREEN_LIMIT: u8 = 24;
const CORE_DARK_BLUE_LIMIT: u8 = 24;
const TOLERANT_DARK_LUMA_LIMIT: f32 = 32.0;
const TOLERANT_DARK_RED_LIMIT: u8 = 5;
const TOLERANT_DARK_GREEN_LIMIT: u8 = 30;
const TOLERANT_DARK_BLUE_LIMIT: u8 = 30;
const COMPATIBLE_DARK_LUMA_LIMIT: f32 = 58.0;
const CENTER_SCAN_Y_START: f32 = 0.55;
const CENTER_SCAN_Y_END: f32 = 0.64;
const COLUMN_DARK_RATIO_LIMIT: f32 = 0.45;
const CENTER_SEARCH_HALF_WIDTH: f32 = 0.10;
const CARD_SUPPORT_HALF_WIDTH_TO_SPACING: f32 = 0.46;
const CARD_SUPPORT_TOLERANT_RATIO: f32 = 0.30;
const CARD_SUPPORT_CORE_RATIO: f32 = 0.08;
const CARD_SUPPORT_CONTIGUOUS_RATIO: f32 = 0.42;
const CARD_SPACING_MIN_RATIO: f32 = 0.14;
const CARD_SPACING_MAX_RATIO: f32 = 0.24;
const CARD_SPACING_ASYMMETRY_LIMIT: f32 = 0.12;
const CARD_HEIGHT_TO_WIDTH: f32 = 1.65;
const CONTENT_WIDTH_TO_CENTER_SPACING: f32 = 0.75;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Rect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

impl Rect {
    pub(crate) fn width(self) -> i32 {
        self.right - self.left
    }

    pub(crate) fn height(self) -> i32 {
        self.bottom - self.top
    }

    fn clip(self, width: i32, height: i32) -> Self {
        Self {
            left: self.left.clamp(0, width),
            top: self.top.clamp(0, height),
            right: self.right.clamp(0, width),
            bottom: self.bottom.clamp(0, height),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CardDetection {
    pub(crate) card_rects: [Rect; 3],
    pub(crate) title_rects: [Rect; 3],
    pub(crate) label_centers: [(i32, i32); 3],
}

#[derive(Debug, Clone, Copy)]
struct DarkRun {
    left: i32,
    right: i32,
}

impl DarkRun {
    fn width(self) -> i32 {
        self.right - self.left
    }

    fn center(self) -> f32 {
        (self.left + self.right) as f32 / 2.0
    }
}

/// 根据中心卡片和至少一侧卡片的深色核心反推三张卡的位置。
///
/// 左右卡片的深色核心可能被光效遮住，因此允许只验证到一侧；未验证到的一侧仍使用
/// 本帧实测间距对称推算坐标，最终是否显示由 OCR 和海克斯数据匹配结果决定。
pub(crate) fn detect_cards(
    bgra: &[u8],
    width: i32,
    height: i32,
    mode: DetectionMode,
) -> Option<CardDetection> {
    if width < 960 || height < 540 || bgra.len() < width as usize * height as usize * 4 {
        return None;
    }

    let scan_top = (height as f32 * CENTER_SCAN_Y_START).round() as i32;
    let scan_bottom = (height as f32 * CENTER_SCAN_Y_END).round() as i32;
    if scan_bottom <= scan_top {
        return None;
    }

    let mut dark_columns = build_dark_columns(bgra, width, scan_top, scan_bottom, mode);
    close_short_gaps(&mut dark_columns, 4);
    let runs = candidate_dark_runs(&dark_columns, width);
    let center = select_center_run(&runs, width)?;
    let card_centers = measure_card_centers(&runs, center, bgra, width, height, mode)?;
    let average_spacing = (card_centers[2] - card_centers[0]) / 2.0;
    let card_width = (average_spacing * CONTENT_WIDTH_TO_CENTER_SPACING).round() as i32;
    let card_height = (card_width as f32 * CARD_HEIGHT_TO_WIDTH).round() as i32;
    if card_width < width / 12 || card_height <= card_width {
        return None;
    }

    let center_x = center.center().round() as i32;
    let (bottom_edge, _) = find_bottom_edge(bgra, width, height, center_x, card_width)?;
    let card_top = bottom_edge - card_height;
    if card_top < 0 || bottom_edge > height {
        return None;
    }

    let mut card_rects = [Rect::default(); 3];
    let mut title_rects = [Rect::default(); 3];
    let mut label_centers = [(0, 0); 3];
    for (slot, center_x) in card_centers.into_iter().enumerate() {
        let center_x = center_x.round() as i32;
        let card = Rect {
            left: center_x - card_width / 2,
            top: card_top,
            right: center_x + card_width / 2,
            bottom: card_top + card_height,
        }
        .clip(width, height);
        if card.width() <= 0 || card.height() <= 0 {
            return None;
        }

        let title = Rect {
            left: card.left + (card.width() as f32 * 0.12).round() as i32,
            top: card.top + (card.height() as f32 * 0.395).round() as i32,
            right: card.right - (card.width() as f32 * 0.12).round() as i32,
            bottom: card.top + (card.height() as f32 * 0.485).round() as i32,
        }
        .clip(width, height);
        if title.width() < 12 || title.height() < 8 {
            return None;
        }

        card_rects[slot] = card;
        title_rects[slot] = title;
        label_centers[slot] = (
            center_x,
            card.top + (card.height() as f32 * 0.735).round() as i32,
        );
    }

    Some(CardDetection {
        card_rects,
        title_rects,
        label_centers,
    })
}

fn build_dark_columns(
    bgra: &[u8],
    width: i32,
    scan_top: i32,
    scan_bottom: i32,
    mode: DetectionMode,
) -> Vec<bool> {
    let scan_height = (scan_bottom - scan_top).max(1) as f32;
    let mut columns = vec![false; width.max(0) as usize];
    for x in 0..width {
        let dark_count = (scan_top..scan_bottom)
            .filter(|&y| is_tolerant_dark(pixel(bgra, width, x, y), mode))
            .count();
        columns[x as usize] = dark_count as f32 / scan_height >= COLUMN_DARK_RATIO_LIMIT;
    }
    columns
}

fn select_center_run(runs: &[DarkRun], image_width: i32) -> Option<DarkRun> {
    let center = image_width as f32 / 2.0;
    let maximum_offset = image_width as f32 * CENTER_SEARCH_HALF_WIDTH;
    runs.iter()
        .copied()
        .filter(|run| (run.center() - center).abs() <= maximum_offset)
        .min_by(|left, right| {
            let left_score = center_run_score(*left, center, image_width);
            let right_score = center_run_score(*right, center, image_width);
            left_score
                .partial_cmp(&right_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn center_run_score(run: DarkRun, image_center: f32, image_width: i32) -> f32 {
    let offset = (run.center() - image_center).abs() / image_width as f32;
    let expected_width = image_width as f32 * 0.115;
    let width_error = (run.width() as f32 - expected_width).abs() / image_width as f32;
    offset * 4.0 + width_error * 0.35
}

fn measure_card_centers(
    runs: &[DarkRun],
    center: DarkRun,
    bgra: &[u8],
    width: i32,
    height: i32,
    mode: DetectionMode,
) -> Option<[f32; 3]> {
    let center_x = center.center();
    let minimum_spacing = width as f32 * CARD_SPACING_MIN_RATIO;
    let maximum_spacing = width as f32 * CARD_SPACING_MAX_RATIO;
    let sides: Vec<DarkRun> = runs
        .iter()
        .copied()
        .filter(|side| {
            let spacing = (side.center() - center_x).abs();
            (minimum_spacing..=maximum_spacing).contains(&spacing)
                && has_card_support(bgra, width, height, center_x, spacing, mode)
                && has_card_support(bgra, width, height, side.center(), spacing, mode)
        })
        .collect();

    // 优先使用两侧实测位置；屏幕比例仅限制候选范围，不再决定最终间距。
    let mut best_pair: Option<([f32; 3], f32)> = None;
    for left in sides.iter().filter(|side| side.center() < center_x) {
        for right in sides.iter().filter(|side| side.center() > center_x) {
            let left_spacing = center_x - left.center();
            let right_spacing = right.center() - center_x;
            let average_spacing = (left_spacing + right_spacing) / 2.0;
            let asymmetry = (left_spacing - right_spacing).abs() / average_spacing;
            if asymmetry > CARD_SPACING_ASYMMETRY_LIMIT {
                continue;
            }
            let width_error = ((left.width() - center.width()).abs()
                + (right.width() - center.width()).abs()) as f32
                / (center.width() * 2) as f32;
            let score = asymmetry * 5.0 + width_error * 0.2;
            if best_pair.as_ref().is_none_or(|(_, best)| score < *best) {
                best_pair = Some(([left.center(), center_x, right.center()], score));
            }
        }
    }
    if let Some((centers, _)) = best_pair {
        return Some(centers);
    }

    // 一侧被光效遮挡时，以另一侧到中心的实测距离镜像补齐，不用固定值兜底。
    let side = sides
        .iter()
        .min_by_key(|side| (side.width() - center.width()).abs())?;
    let spacing = (side.center() - center_x).abs();
    Some([center_x - spacing, center_x, center_x + spacing])
}

fn has_card_support(
    bgra: &[u8],
    width: i32,
    height: i32,
    center_x: f32,
    spacing: f32,
    mode: DetectionMode,
) -> bool {
    let scan_top = (height as f32 * CENTER_SCAN_Y_START).round() as i32;
    let scan_bottom = (height as f32 * CENTER_SCAN_Y_END).round() as i32;
    let half_width = (spacing * CARD_SUPPORT_HALF_WIDTH_TO_SPACING).round() as i32;
    let left = (center_x.round() as i32 - half_width).clamp(0, width);
    let right = (center_x.round() as i32 + half_width).clamp(0, width);
    if right <= left || scan_bottom <= scan_top {
        return false;
    }

    let scan_height = (scan_bottom - scan_top) as f32;
    let mut tolerant_pixels = 0usize;
    let mut core_pixels = 0usize;
    let mut longest_contiguous = 0usize;
    let mut current_contiguous = 0usize;
    for x in left..right {
        let mut tolerant_count = 0usize;
        for y in scan_top..scan_bottom {
            let rgb = pixel(bgra, width, x, y);
            if is_tolerant_dark(rgb, mode) {
                tolerant_count += 1;
                tolerant_pixels += 1;
            }
            if mode == DetectionMode::HighPrecision && is_core_dark(rgb) {
                core_pixels += 1;
            }
        }
        if tolerant_count as f32 / scan_height >= COLUMN_DARK_RATIO_LIMIT {
            current_contiguous += 1;
            longest_contiguous = longest_contiguous.max(current_contiguous);
        } else {
            current_contiguous = 0;
        }
    }

    let total_pixels = ((right - left) * (scan_bottom - scan_top)) as f32;
    tolerant_pixels as f32 / total_pixels >= CARD_SUPPORT_TOLERANT_RATIO
        && (mode == DetectionMode::Compatibility
            || core_pixels as f32 / total_pixels >= CARD_SUPPORT_CORE_RATIO)
        && longest_contiguous as f32 / (right - left) as f32 >= CARD_SUPPORT_CONTIGUOUS_RATIO
}

fn pixel(bgra: &[u8], width: i32, x: i32, y: i32) -> (u8, u8, u8) {
    let index = ((y * width + x) * 4) as usize;
    (bgra[index + 2], bgra[index + 1], bgra[index])
}

fn luma((red, green, blue): (u8, u8, u8)) -> f32 {
    red as f32 * 0.2126 + green as f32 * 0.7152 + blue as f32 * 0.0722
}

fn close_short_gaps(values: &mut [bool], maximum_gap: usize) {
    let mut index = 0usize;
    while index < values.len() {
        if values[index] {
            index += 1;
            continue;
        }

        let start = index;
        while index < values.len() && !values[index] {
            index += 1;
        }
        let end = index;
        if start > 0
            && end < values.len()
            && values[start - 1]
            && values[end]
            && end - start <= maximum_gap
        {
            values[start..end].fill(true);
        }
    }
}

fn candidate_dark_runs(columns: &[bool], image_width: i32) -> Vec<DarkRun> {
    let minimum_width = (image_width as f32 * 0.045).round() as i32;
    let maximum_width = (image_width as f32 * 0.20).round() as i32;
    let mut result = Vec::new();
    let mut index = 0usize;

    while index < columns.len() {
        if !columns[index] {
            index += 1;
            continue;
        }
        let left = index;
        while index < columns.len() && columns[index] {
            index += 1;
        }
        let run = DarkRun {
            left: left as i32,
            right: index as i32,
        };
        if run.width() >= minimum_width && run.width() <= maximum_width {
            result.push(run);
        }
    }
    result
}

fn find_bottom_edge(
    bgra: &[u8],
    width: i32,
    height: i32,
    center_x: i32,
    card_width: i32,
) -> Option<(i32, f32)> {
    let left = max(
        0,
        (center_x as f32 - card_width as f32 * 0.34).round() as i32,
    );
    let right = min(
        width,
        (center_x as f32 + card_width as f32 * 0.34).round() as i32,
    );
    let start = (height as f32 * 0.61).round() as i32;
    let end = (height as f32 * 0.73).round() as i32;
    if right <= left || end <= start {
        return None;
    }

    let expected_y = height as f32 * 0.667;
    let mut previous_mean = None;
    let mut best = None;
    for y in start..end {
        let mut bright = 0i32;
        let mut total_luma = 0.0f32;
        for x in left..right {
            let current_luma = luma(pixel(bgra, width, x, y));
            total_luma += current_luma;
            if current_luma > 125.0 {
                bright += 1;
            }
        }
        let samples = (right - left) as f32;
        let mean_luma = total_luma / samples;
        let gradient = previous_mean
            .map(|previous: f32| (mean_luma - previous).abs() / 255.0)
            .unwrap_or(0.0);
        previous_mean = Some(mean_luma);
        let proximity = 1.0 - ((y as f32 - expected_y).abs() / (height as f32 * 0.08)).min(1.0);
        let score = bright as f32 / samples * 0.78 + gradient * 0.16 + proximity * 0.06;
        if best
            .as_ref()
            .is_none_or(|(_, best_score)| score > *best_score)
        {
            best = Some((y, score));
        }
    }
    best
}

fn is_core_dark((red, green, blue): (u8, u8, u8)) -> bool {
    luma((red, green, blue)) <= CORE_DARK_LUMA_LIMIT
        && red <= CORE_DARK_RED_LIMIT
        && green <= CORE_DARK_GREEN_LIMIT
        && blue <= CORE_DARK_BLUE_LIMIT
}

fn is_tolerant_dark(rgb: (u8, u8, u8), mode: DetectionMode) -> bool {
    let (red, green, blue) = rgb;
    match mode {
        DetectionMode::HighPrecision => {
            luma(rgb) <= TOLERANT_DARK_LUMA_LIMIT
                && red <= TOLERANT_DARK_RED_LIMIT
                && green <= TOLERANT_DARK_GREEN_LIMIT
                && blue <= TOLERANT_DARK_BLUE_LIMIT
        }
        // 沿用低亮度方案的宽松通道上限，不再要求高精度方案的深色核心。
        DetectionMode::Compatibility => {
            luma(rgb) <= COMPATIBLE_DARK_LUMA_LIMIT && red < 90 && green < 100 && blue < 112
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_cards, is_core_dark, is_tolerant_dark, DetectionMode};

    const MODES: [DetectionMode; 2] = [DetectionMode::HighPrecision, DetectionMode::Compatibility];

    #[test]
    fn uses_the_strict_core_threshold_before_tolerance() {
        assert!(is_core_dark((2, 24, 24)));
        assert!(is_core_dark((3, 24, 24)));
        assert!(!is_core_dark((4, 24, 24)));
        assert!(!is_core_dark((2, 25, 24)));
        assert!(!is_core_dark((2, 24, 25)));
        assert!(is_tolerant_dark((5, 30, 30), DetectionMode::HighPrecision));
        assert!(!is_tolerant_dark((6, 30, 30), DetectionMode::HighPrecision));
    }

    #[test]
    fn compatibility_uses_low_luma_instead_of_strict_rgb() {
        assert!(is_tolerant_dark((40, 40, 40), DetectionMode::Compatibility));
        assert!(is_tolerant_dark((57, 57, 57), DetectionMode::Compatibility));
        assert!(!is_tolerant_dark(
            (59, 59, 59),
            DetectionMode::Compatibility
        ));
        assert!(!is_tolerant_dark((90, 0, 0), DetectionMode::Compatibility));
        assert!(!is_tolerant_dark((0, 0, 112), DetectionMode::Compatibility));
        assert!(!is_tolerant_dark(
            (40, 40, 40),
            DetectionMode::HighPrecision
        ));
    }

    #[test]
    fn center_and_one_side_are_enough_to_confirm_the_layout() {
        for mode in MODES {
            for (left, right) in [(true, false), (false, true), (true, true)] {
                let (bgra, width, height) = synthetic_cards(true, left, right, (3, 16, 19));
                assert!(
                    detect_cards(&bgra, width, height, mode).is_some(),
                    "{mode:?}"
                );
            }
        }
    }

    #[test]
    fn measures_actual_spacing_for_different_aspect_ratios() {
        for (width, height, spacing) in [(1920, 1080, 368), (2560, 1600, 546)] {
            let (bgra, _, _) = synthetic_layout(width, height, spacing, [true; 3], (3, 16, 19));
            for mode in MODES {
                let result = detect_cards(&bgra, width, height, mode).unwrap();
                assert_eq!(result.label_centers[0].0, width / 2 - spacing);
                assert_eq!(result.label_centers[1].0, width / 2);
                assert_eq!(result.label_centers[2].0, width / 2 + spacing);
                let expected_width = (spacing as f32 * 0.75).round() as i32;
                let expected_height = (expected_width as f32 * 1.65).round() as i32;
                assert_eq!(result.card_rects[1].width(), expected_width / 2 * 2);
                assert_eq!(result.card_rects[1].height(), expected_height);
            }
        }
    }

    #[test]
    fn a_single_visible_side_also_supplies_the_measured_spacing() {
        for included in [[true, true, false], [false, true, true]] {
            let (bgra, width, height) = synthetic_layout(1600, 1000, 341, included, (3, 16, 19));
            for mode in MODES {
                let result = detect_cards(&bgra, width, height, mode).unwrap();
                assert_eq!(result.label_centers[0].0, width / 2 - 341);
                assert_eq!(result.label_centers[2].0, width / 2 + 341);
            }
        }
    }

    #[test]
    fn spacing_changes_are_remeasured_at_the_same_resolution() {
        let (first, width, height) = synthetic_layout(1600, 1000, 308, [true; 3], (3, 16, 19));
        let (second, _, _) = synthetic_layout(width, height, 342, [true; 3], (3, 16, 19));
        for mode in MODES {
            let initial = detect_cards(&first, width, height, mode).unwrap();
            let changed = detect_cards(&second, width, height, mode).unwrap();
            let restored = detect_cards(&first, width, height, mode).unwrap();
            assert_eq!(initial.label_centers[0].0, width / 2 - 308);
            assert_eq!(changed.label_centers[0].0, width / 2 - 342);
            assert!(changed.card_rects[1].height() > initial.card_rects[1].height());
            assert_eq!(restored.card_rects, initial.card_rects);
            assert_eq!(restored.title_rects, initial.title_rects);
        }
    }

    #[test]
    fn center_alone_does_not_confirm_the_layout() {
        let (bgra, width, height) = synthetic_cards(true, false, false, (3, 16, 19));
        for mode in MODES {
            assert!(
                detect_cards(&bgra, width, height, mode).is_none(),
                "{mode:?}"
            );
        }
    }

    #[test]
    fn side_cards_without_a_center_do_not_confirm_the_layout() {
        let (bgra, width, height) = synthetic_cards(false, true, true, (3, 16, 19));
        for mode in MODES {
            assert!(
                detect_cards(&bgra, width, height, mode).is_none(),
                "{mode:?}"
            );
        }
    }

    #[test]
    fn high_precision_does_not_fall_back_to_compatibility() {
        for rgb in [(4, 16, 19), (40, 40, 40)] {
            let (bgra, width, height) = synthetic_cards(true, true, true, rgb);
            assert!(detect_cards(&bgra, width, height, DetectionMode::HighPrecision).is_none());
            assert!(detect_cards(&bgra, width, height, DetectionMode::Compatibility).is_some());
        }
    }

    #[test]
    fn uniform_backgrounds_are_not_card_layouts() {
        for shade in [0, 20, 80, 255] {
            let bgra = vec![shade; 1000 * 600 * 4];
            for mode in MODES {
                assert!(detect_cards(&bgra, 1000, 600, mode).is_none(), "{mode:?}");
            }
        }
    }

    #[test]
    fn invalid_or_small_frames_are_rejected() {
        for mode in MODES {
            assert!(detect_cards(&[], 1000, 600, mode).is_none());
            assert!(detect_cards(&[], -1, 600, mode).is_none());
            let bgra = vec![0; 640 * 480 * 4];
            assert!(detect_cards(&bgra, 640, 480, mode).is_none());
        }
    }

    #[test]
    #[ignore = "set HEX_WINRATE_TEST_IMAGE to a full game screenshot"]
    fn detects_cards_in_supplied_screenshot() {
        let path = std::env::var_os("HEX_WINRATE_TEST_IMAGE").expect("set HEX_WINRATE_TEST_IMAGE");
        let image = image::open(path).expect("read screenshot").to_rgba8();
        let (width, height) = image.dimensions();
        let mut bgra = image.into_raw();
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        for mode in MODES {
            let detection = detect_cards(&bgra, width as i32, height as i32, mode)
                .unwrap_or_else(|| panic!("{mode:?} failed to detect the supplied screenshot"));
            eprintln!("{mode:?}: {detection:?}");
        }
    }

    fn synthetic_cards(
        include_center: bool,
        include_left: bool,
        include_right: bool,
        rgb: (u8, u8, u8),
    ) -> (Vec<u8>, i32, i32) {
        synthetic_layout(
            1000,
            600,
            192,
            [include_left, include_center, include_right],
            rgb,
        )
    }

    fn synthetic_layout(
        width: i32,
        height: i32,
        spacing: i32,
        included: [bool; 3],
        rgb: (u8, u8, u8),
    ) -> (Vec<u8>, i32, i32) {
        let mut bgra = vec![80u8; (width * height * 4) as usize];
        let center = width / 2;
        let core_half_width = (spacing as f32 * 0.417).round() as i32;
        let top = (height as f32 * 0.25).round() as i32;
        let bottom = (height as f32 * (2.0 / 3.0)).round() as i32;
        for (slot, card_center) in [center - spacing, center, center + spacing]
            .into_iter()
            .enumerate()
        {
            if !included[slot] {
                continue;
            }
            for y in top..bottom {
                for x in (card_center - core_half_width).max(0)
                    ..(card_center + core_half_width).min(width)
                {
                    let index = ((y * width + x) * 4) as usize;
                    bgra[index] = rgb.2;
                    bgra[index + 1] = rgb.1;
                    bgra[index + 2] = rgb.0;
                    bgra[index + 3] = 255;
                }
            }
        }
        for card_center in [center - spacing, center, center + spacing] {
            for x in
                (card_center - core_half_width).max(0)..(card_center + core_half_width).min(width)
            {
                let index = ((bottom * width + x) * 4) as usize;
                bgra[index] = 220;
                bgra[index + 1] = 220;
                bgra[index + 2] = 220;
                bgra[index + 3] = 255;
            }
        }
        (bgra, width, height)
    }
}
