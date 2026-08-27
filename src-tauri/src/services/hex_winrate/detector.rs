use std::cmp::{max, min};

const DARK_LUMA_LIMIT: f32 = 58.0;
const DARK_RED_LIMIT: u8 = 90;
const DARK_GREEN_LIMIT: u8 = 100;
const DARK_BLUE_LIMIT: u8 = 112;
const CENTER_SCAN_Y_START: f32 = 0.55;
const CENTER_SCAN_Y_END: f32 = 0.64;
const COLUMN_DARK_RATIO_LIMIT: f32 = 0.78;
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

/// 根据三张卡片下半部稳定存在的深色内容区反推整张卡的位置。
///
/// 金色和棱彩海克斯的黑色核心宽度会收窄，因此坐标只使用三个核心的中心点与间距，
/// 不直接把黑色核心宽度当作卡片宽度。
pub(crate) fn detect_cards(bgra: &[u8], width: i32, height: i32) -> Option<CardDetection> {
    if width < 960 || height < 540 || bgra.len() < width as usize * height as usize * 4 {
        return None;
    }

    let scan_top = (height as f32 * CENTER_SCAN_Y_START).round() as i32;
    let scan_bottom = (height as f32 * CENTER_SCAN_Y_END).round() as i32;
    if scan_bottom <= scan_top {
        return None;
    }

    let mut dark_columns = vec![false; width as usize];
    for x in 0..width {
        let mut dark_count = 0i32;
        for y in scan_top..scan_bottom {
            if is_dark(pixel(bgra, width, x, y)) {
                dark_count += 1;
            }
        }
        dark_columns[x as usize] =
            dark_count as f32 / (scan_bottom - scan_top) as f32 >= COLUMN_DARK_RATIO_LIMIT;
    }

    close_short_gaps(&mut dark_columns, 4);
    let runs = candidate_dark_runs(&dark_columns, width);
    let selected =
        select_card_runs(&runs, width).or_else(|| fallback_card_runs(&dark_columns, width))?;
    let core_rects = selected.map(|run| Rect {
        left: run.left,
        top: scan_top,
        right: run.right,
        bottom: scan_bottom,
    });
    if dark_ratio(bgra, width, height, core_rects[1]) < 0.78 {
        return None;
    }

    let left_gap = selected[1].center() - selected[0].center();
    let right_gap = selected[2].center() - selected[1].center();
    let average_spacing = (left_gap + right_gap) / 2.0;
    let card_width = (average_spacing * CONTENT_WIDTH_TO_CENTER_SPACING).round() as i32;
    let card_height = (card_width as f32 * CARD_HEIGHT_TO_WIDTH).round() as i32;
    if card_width < width / 12 || card_height <= card_width {
        return None;
    }

    let center_x = selected[1].center().round() as i32;
    let (bottom_edge, _) = find_bottom_edge(bgra, width, height, center_x, card_width)?;
    let card_top = bottom_edge - card_height;
    if card_top < 0 || bottom_edge > height {
        return None;
    }

    let mut title_rects = [Rect::default(); 3];
    let mut label_centers = [(0, 0); 3];
    let mut minimum_lower_dark_ratio = 1.0f32;
    for (slot, core) in selected.into_iter().enumerate() {
        let center_x = core.center().round() as i32;
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

        let lower_black = Rect {
            left: card.left + (card.width() as f32 * 0.15).round() as i32,
            top: card.top + (card.height() as f32 * 0.69).round() as i32,
            right: card.right - (card.width() as f32 * 0.15).round() as i32,
            bottom: card.top + (card.height() as f32 * 0.91).round() as i32,
        }
        .clip(width, height);
        minimum_lower_dark_ratio =
            minimum_lower_dark_ratio.min(dark_ratio(bgra, width, height, lower_black));

        title_rects[slot] = title;
        label_centers[slot] = (
            center_x,
            card.top + (card.height() as f32 * 0.735).round() as i32,
        );
    }

    if minimum_lower_dark_ratio < 0.62 {
        return None;
    }

    Some(CardDetection {
        title_rects,
        label_centers,
    })
}

fn pixel(bgra: &[u8], width: i32, x: i32, y: i32) -> (u8, u8, u8) {
    let index = ((y * width + x) * 4) as usize;
    (bgra[index + 2], bgra[index + 1], bgra[index])
}

fn luma((red, green, blue): (u8, u8, u8)) -> f32 {
    red as f32 * 0.2126 + green as f32 * 0.7152 + blue as f32 * 0.0722
}

fn is_dark(rgb: (u8, u8, u8)) -> bool {
    let (red, green, blue) = rgb;
    luma(rgb) < DARK_LUMA_LIMIT
        && red < DARK_RED_LIMIT
        && green < DARK_GREEN_LIMIT
        && blue < DARK_BLUE_LIMIT
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
    let minimum_width = (image_width as f32 * 0.10).round() as i32;
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

fn select_card_runs(runs: &[DarkRun], image_width: i32) -> Option<[DarkRun; 3]> {
    let mut best: Option<(f32, [DarkRun; 3])> = None;
    for left_index in 0..runs.len() {
        for middle_index in left_index + 1..runs.len() {
            for right_index in middle_index + 1..runs.len() {
                let candidate = [runs[left_index], runs[middle_index], runs[right_index]];
                let left_gap = candidate[1].center() - candidate[0].center();
                let right_gap = candidate[2].center() - candidate[1].center();
                let average_gap = (left_gap + right_gap) / 2.0;
                if average_gap < image_width as f32 * 0.14
                    || average_gap > image_width as f32 * 0.24
                {
                    continue;
                }

                let symmetry_error = (left_gap - right_gap).abs() / image_width as f32;
                let middle_offset =
                    (candidate[1].center() - image_width as f32 / 2.0).abs() / image_width as f32;
                let spacing_error =
                    (average_gap - image_width as f32 * 0.192).abs() / image_width as f32;
                let widths = [
                    candidate[0].width() as f32,
                    candidate[1].width() as f32,
                    candidate[2].width() as f32,
                ];
                let mean_width = (widths[0] + widths[1] + widths[2]) / 3.0;
                let width_variation = ((widths[0] - mean_width).powi(2)
                    + (widths[1] - mean_width).powi(2)
                    + (widths[2] - mean_width).powi(2))
                .sqrt()
                    / mean_width.max(1.0);
                let score = symmetry_error * 5.0
                    + middle_offset * 3.0
                    + spacing_error * 0.8
                    + width_variation * 0.2;

                if best
                    .as_ref()
                    .is_none_or(|(best_score, _)| score < *best_score)
                {
                    best = Some((score, candidate));
                }
            }
        }
    }
    best.map(|(_, candidate)| candidate)
}

/// 光效或局内提示偶尔会遮住左右卡片的黑色核心。此时仍以屏幕中心的卡片为锚点，
/// 按固定布局推导另外两张卡；后续深色校验和 OCR 会继续过滤误判。
fn fallback_card_runs(columns: &[bool], image_width: i32) -> Option<[DarkRun; 3]> {
    if image_width <= 0 {
        return None;
    }

    let center_index = (image_width as usize / 2).min(columns.len().saturating_sub(1));
    let center = run_containing(columns, center_index).unwrap_or_else(|| {
        let half_width = (image_width as f32 * 0.071).round() as i32;
        DarkRun {
            left: image_width / 2 - half_width,
            right: image_width / 2 + half_width,
        }
    });
    let half_width = center.width() / 2;
    let spacing = (image_width as f32 * 0.192).round() as i32;
    let center_x = center.center().round() as i32;

    Some([
        DarkRun {
            left: center_x - spacing - half_width,
            right: center_x - spacing + half_width,
        },
        center,
        DarkRun {
            left: center_x + spacing - half_width,
            right: center_x + spacing + half_width,
        },
    ])
}

fn run_containing(columns: &[bool], target: usize) -> Option<DarkRun> {
    let target = if columns.get(target).copied().unwrap_or(false) {
        target
    } else {
        columns
            .iter()
            .enumerate()
            .filter_map(|(index, value)| value.then_some(index))
            .min_by_key(|index| index.abs_diff(target))?
    };

    let mut left = target;
    let mut right = target + 1;
    while left > 0 && columns[left - 1] {
        left -= 1;
    }
    while right < columns.len() && columns[right] {
        right += 1;
    }
    Some(DarkRun {
        left: left as i32,
        right: right as i32,
    })
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

fn dark_ratio(bgra: &[u8], width: i32, height: i32, rect: Rect) -> f32 {
    let rect = rect.clip(width, height);
    if rect.width() <= 0 || rect.height() <= 0 {
        return 0.0;
    }

    let mut dark_count = 0i32;
    for y in rect.top..rect.bottom {
        for x in rect.left..rect.right {
            if is_dark(pixel(bgra, width, x, y)) {
                dark_count += 1;
            }
        }
    }
    dark_count as f32 / (rect.width() * rect.height()) as f32
}

#[cfg(test)]
mod tests {
    use super::{fallback_card_runs, select_card_runs, DarkRun};

    #[test]
    fn prefers_centered_equally_spaced_card_cores() {
        let result = select_card_runs(
            &[
                DarkRun {
                    left: 440,
                    right: 660,
                },
                DarkRun {
                    left: 830,
                    right: 1050,
                },
                DarkRun {
                    left: 1220,
                    right: 1440,
                },
            ],
            1920,
        );
        assert!(result.is_some());
    }

    #[test]
    fn fallback_keeps_card_centers_symmetric_around_the_screen_center() {
        let mut columns = vec![false; 1920];
        columns[825..1105].fill(true);
        let result = fallback_card_runs(&columns, 1920).expect("应能从中间卡片推导布局");

        assert_eq!(result[1].center(), 965.0);
        assert!((result[1].center() - result[0].center() - 369.0).abs() <= 1.0);
        assert!((result[2].center() - result[1].center() - 369.0).abs() <= 1.0);
    }
}
