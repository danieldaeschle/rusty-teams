use std::time::Duration;

use crate::blur::{MASK_HEIGHT, MASK_WIDTH, axis};

const DARK_LUMA: f32 = 0.35;
const VERY_DARK_LUMA: f32 = 0.05;
const GAMMA_AT_DARK: f32 = 0.75;
const GAMMA_AT_VERY_DARK: f32 = 0.6;
const GAMMA_TIME_CONSTANT: f32 = 1.0;
const GAMMA_SETTLED: f32 = 0.005;
const LUMA_SAMPLE_STEP: usize = 4;
const SOFT_FOCUS_MAX_ALPHA: f32 = 0.7;
const CELL: usize = 4;
const FILTER_RADIUS: usize = 1;
const FILTER_EPSILON: f32 = 100.;
const NEAR_WEIGHTS: [f32; CELL] = [0.625, 0.875, 0.875, 0.625];
const INSIDE_START: f32 = 0.5;
const MAX_BANDS: usize = 4;

pub fn mean_luma(plane: &[u8], stride: usize, width: usize, height: usize) -> f32 {
    let (mut sum, mut count) = (0u64, 0u64);
    for row in (0..height).step_by(LUMA_SAMPLE_STEP) {
        for &value in plane[row * stride..][..width].iter().step_by(LUMA_SAMPLE_STEP) {
            sum += u64::from(value);
            count += 1;
        }
    }
    if count == 0 { 0. } else { sum as f32 / (count as f32 * 255.) }
}

pub fn target_gamma(mean_luma: f32) -> f32 {
    if mean_luma >= DARK_LUMA {
        return 1.;
    }
    let darkness = ((DARK_LUMA - mean_luma) / (DARK_LUMA - VERY_DARK_LUMA)).clamp(0., 1.);
    GAMMA_AT_DARK + (GAMMA_AT_VERY_DARK - GAMMA_AT_DARK) * darkness
}

pub fn smooth_gamma(current: f32, target: f32, elapsed: Duration) -> f32 {
    let next = current + (target - current) * (1. - (-elapsed.as_secs_f32() / GAMMA_TIME_CONSTANT).exp());
    if (next - target).abs() < GAMMA_SETTLED { target } else { next }
}

pub fn gamma_table(gamma: f32) -> [u8; 256] {
    std::array::from_fn(|value| (255. * (value as f32 / 255.).powf(gamma)).round() as u8)
}

pub fn apply_table(plane: &mut [u8], stride: usize, width: usize, height: usize, table: &[u8; 256]) {
    for row in plane.chunks_mut(stride).take(height) {
        for value in &mut row[..width] {
            *value = table[usize::from(*value)];
        }
    }
}

pub fn inside_person(mask_value: f32) -> f32 {
    ((mask_value - INSIDE_START) / (1. - INSIDE_START)).clamp(0., 1.)
}

pub fn soft_focus_alpha(strength_percent: u8, inside: f32) -> f32 {
    f32::from(strength_percent.min(100)) / 100. * SOFT_FOCUS_MAX_ALPHA * inside
}

pub fn blend_coefficients(alpha: f32, slope: f32, offset: f32) -> (f32, f32) {
    (1. + alpha * (slope - 1.), alpha * offset)
}

pub fn apply_coefficients(value: u8, gain: f32, bias: f32) -> u8 {
    (f32::from(value) * gain + bias + 0.5) as u8
}

fn box_mean(input: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut across = vec![0f32; input.len()];
    for (line, mean) in input.chunks_exact(width).zip(across.chunks_exact_mut(width)) {
        for (index, slot) in mean.iter_mut().enumerate() {
            let (low, high) = (index.saturating_sub(FILTER_RADIUS), (index + FILTER_RADIUS).min(width - 1));
            *slot = line[low..=high].iter().sum::<f32>() / (high + 1 - low) as f32;
        }
    }
    let mut output = vec![0f32; input.len()];
    for row in 0..height {
        let (low, high) = (row.saturating_sub(FILTER_RADIUS), (row + FILTER_RADIUS).min(height - 1));
        let slot = &mut output[row * width..][..width];
        for line in low..=high {
            slot.iter_mut().zip(&across[line * width..][..width]).for_each(|(sum, value)| *sum += value);
        }
        let count = (high + 1 - low) as f32;
        slot.iter_mut().for_each(|sum| *sum /= count);
    }
    output
}

struct Cells {
    gain: Vec<f32>,
    bias: Vec<f32>,
    alpha: Vec<f32>,
    width: usize,
    height: usize,
}

fn cell_means(plane: &[u8], stride: usize, cell_width: usize, cell_height: usize) -> Vec<f32> {
    let mut means = vec![0f32; cell_width * cell_height];
    let mut column_sums = vec![0u16; cell_width * CELL];
    for (row, mean) in means.chunks_exact_mut(cell_width).enumerate() {
        column_sums.fill(0);
        for line in 0..CELL {
            let pixels = &plane[(row * CELL + line) * stride..][..cell_width * CELL];
            column_sums.iter_mut().zip(pixels).for_each(|(sum, &value)| *sum += u16::from(value));
        }
        for (slot, group) in mean.iter_mut().zip(column_sums.as_chunks::<CELL>().0) {
            *slot = group.iter().map(|&sum| u32::from(sum)).sum::<u32>() as f32 / (CELL * CELL) as f32;
        }
    }
    means
}

fn smoothing_cells(plane: &[u8], stride: usize, width: usize, height: usize, mask: &[f32], strength_percent: u8) -> Cells {
    let (cell_width, cell_height) = (width / CELL, height / CELL);
    let small = cell_means(plane, stride, cell_width, cell_height);
    let squared: Vec<f32> = small.iter().map(|value| value * value).collect();
    let mean = box_mean(&small, cell_width, cell_height);
    let mean_squared = box_mean(&squared, cell_width, cell_height);
    let slope: Vec<f32> = mean.iter().zip(&mean_squared).map(|(mean, squared)| {
        let variance = (squared - mean * mean).max(0.);
        variance / (variance + FILTER_EPSILON)
    }).collect();
    let offset: Vec<f32> = mean.iter().zip(&slope).map(|(mean, slope)| mean * (1. - slope)).collect();
    let (mean_slope, mean_offset) = (box_mean(&slope, cell_width, cell_height), box_mean(&offset, cell_width, cell_height));
    let (mask_x, mask_y) = (axis(cell_width, MASK_WIDTH), axis(cell_height, MASK_HEIGHT));
    let mut cells = Cells { gain: vec![1.; small.len()], bias: vec![0.; small.len()], alpha: vec![0.; small.len()], width: cell_width, height: cell_height };
    for row in 0..cell_height {
        let (mask_top, mask_bottom, mask_fraction) = (mask_y.first[row] * MASK_WIDTH, mask_y.second[row] * MASK_WIDTH, mask_y.weight[row]);
        for column in 0..cell_width {
            let (left, right, across) = (mask_x.first[column], mask_x.second[column], mask_x.weight[column]);
            let top = mask[mask_top + left] * (1. - across) + mask[mask_top + right] * across;
            let bottom = mask[mask_bottom + left] * (1. - across) + mask[mask_bottom + right] * across;
            let index = row * cell_width + column;
            let alpha = soft_focus_alpha(strength_percent, inside_person(top * (1. - mask_fraction) + bottom * mask_fraction));
            (cells.gain[index], cells.bias[index]) = blend_coefficients(alpha, mean_slope[index], mean_offset[index]);
            cells.alpha[index] = alpha;
        }
    }
    cells
}

fn neighbor(index: usize, step: usize, last: usize) -> usize {
    if step < CELL / 2 { index.saturating_sub(1) } else { (index + 1).min(last) }
}

fn active_columns(cells: &Cells, near_start: usize, far_start: usize, active: &mut [bool]) {
    let alpha_at = |column: usize| cells.alpha[near_start + column] > 0. || cells.alpha[far_start + column] > 0.;
    for (column, slot) in active.iter_mut().enumerate() {
        *slot = alpha_at(column) || alpha_at(column.saturating_sub(1)) || alpha_at((column + 1).min(cells.width - 1));
    }
}

fn blend_rows(plane: &mut [u8], stride: usize, first_row: usize, cells: &Cells) {
    let (mut gain_row, mut bias_row) = (vec![0f32; cells.width], vec![0f32; cells.width]);
    let mut active = vec![false; cells.width];
    let last = cells.width - 1;
    for (offset, line) in plane.chunks_mut(stride).enumerate() {
        let row = first_row + offset;
        if row >= cells.height * CELL {
            break;
        }
        let (near, step) = (row / CELL, row % CELL);
        let far = neighbor(near, step, cells.height - 1);
        let (near_start, far_start) = (near * cells.width, far * cells.width);
        active_columns(cells, near_start, far_start, &mut active);
        if !active.contains(&true) {
            continue;
        }
        let (near_weight, far_weight) = (NEAR_WEIGHTS[step], 1. - NEAR_WEIGHTS[step]);
        for column in 0..cells.width {
            gain_row[column] = cells.gain[near_start + column] * near_weight + cells.gain[far_start + column] * far_weight;
            bias_row[column] = cells.bias[near_start + column] * near_weight + cells.bias[far_start + column] * far_weight;
        }
        for (column, group) in line[..cells.width * CELL].as_chunks_mut::<CELL>().0.iter_mut().enumerate() {
            if !active[column] {
                continue;
            }
            let (before, after) = (column.saturating_sub(1), (column + 1).min(last));
            let (gain, bias) = (gain_row[column], bias_row[column]);
            let (gain_before, bias_before, gain_after, bias_after) = (gain_row[before], bias_row[before], gain_row[after], bias_row[after]);
            let gains = [gain * NEAR_WEIGHTS[0] + gain_before * (1. - NEAR_WEIGHTS[0]), gain * NEAR_WEIGHTS[1] + gain_before * (1. - NEAR_WEIGHTS[1]), gain * NEAR_WEIGHTS[2] + gain_after * (1. - NEAR_WEIGHTS[2]), gain * NEAR_WEIGHTS[3] + gain_after * (1. - NEAR_WEIGHTS[3])];
            let biases = [bias * NEAR_WEIGHTS[0] + bias_before * (1. - NEAR_WEIGHTS[0]), bias * NEAR_WEIGHTS[1] + bias_before * (1. - NEAR_WEIGHTS[1]), bias * NEAR_WEIGHTS[2] + bias_after * (1. - NEAR_WEIGHTS[2]), bias * NEAR_WEIGHTS[3] + bias_after * (1. - NEAR_WEIGHTS[3])];
            for (value, (gain, bias)) in group.iter_mut().zip(gains.into_iter().zip(biases)) {
                *value = apply_coefficients(*value, gain, bias);
            }
        }
    }
}

/// Edge-preserving smoothing (fast guided filter, radius about 4) of the luma plane, blended in only inside the person mask.
pub fn soften_luma(plane: &mut [u8], stride: usize, width: usize, height: usize, mask: &[f32], strength_percent: u8) {
    if strength_percent == 0 || width < CELL * 2 || height < CELL * 2 {
        return;
    }
    let cells = smoothing_cells(plane, stride, width, height, mask, strength_percent);
    let bands = std::thread::available_parallelism().map_or(1, usize::from).min(MAX_BANDS);
    let rows_per_band = height.div_ceil(bands).next_multiple_of(CELL);
    std::thread::scope(|scope| {
        for (index, band) in plane[..stride * (height - 1) + width].chunks_mut(rows_per_band * stride).enumerate() {
            let cells = &cells;
            scope.spawn(move || blend_rows(band, stride, index * rows_per_band, cells));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 64;
    const HEIGHT: usize = 36;

    #[test]
    fn the_gamma_follows_the_darkness_and_leaves_bright_frames_alone() {
        assert_eq!(target_gamma(0.5), 1.);
        assert_eq!(target_gamma(0.35), 1.);
        assert!((target_gamma(0.3499) - GAMMA_AT_DARK).abs() < 0.001);
        assert_eq!(target_gamma(0.05), GAMMA_AT_VERY_DARK);
        assert_eq!(target_gamma(0.), GAMMA_AT_VERY_DARK);
        let middle = target_gamma(0.2);
        assert!(middle < GAMMA_AT_DARK && middle > GAMMA_AT_VERY_DARK);
    }

    #[test]
    fn the_gamma_glides_instead_of_jumping() {
        let step = Duration::from_millis(33);
        let first = smooth_gamma(1., 0.6, step);
        assert!(first < 1. && first > 0.95);
        let mut gamma = 1.;
        for _ in 0..300 {
            gamma = smooth_gamma(gamma, 0.6, step);
        }
        assert_eq!(gamma, 0.6);
    }

    #[test]
    fn the_table_brightens_dark_values_and_keeps_the_ends() {
        let table = gamma_table(0.6);
        assert_eq!((table[0], table[255]), (0, 255));
        assert!(table[40] > 40 && table[128] > 128);
        assert!(table.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(gamma_table(1.), std::array::from_fn(|value| value as u8));
        let mut plane = vec![40u8; WIDTH * HEIGHT];
        apply_table(&mut plane, WIDTH, WIDTH, HEIGHT, &table);
        assert!(plane.iter().all(|&value| value == table[40]));
    }

    #[test]
    fn the_mean_luma_is_a_fraction_of_full_scale() {
        assert_eq!(mean_luma(&vec![255; WIDTH * HEIGHT], WIDTH, WIDTH, HEIGHT), 1.);
        assert_eq!(mean_luma(&vec![0; WIDTH * HEIGHT], WIDTH, WIDTH, HEIGHT), 0.);
        assert!((mean_luma(&vec![51; WIDTH * HEIGHT], WIDTH, WIDTH, HEIGHT) - 0.2).abs() < 1e-3);
    }

    #[test]
    fn the_blend_only_acts_inside_the_person_and_scales_with_strength() {
        assert_eq!(soft_focus_alpha(100, 1.), SOFT_FOCUS_MAX_ALPHA);
        assert_eq!(soft_focus_alpha(50, 1.), 0.35);
        assert_eq!(soft_focus_alpha(0, 1.), 0.);
        assert_eq!(soft_focus_alpha(100, 0.), 0.);
        assert_eq!(inside_person(0.5), 0.);
        assert_eq!(inside_person(0.2), 0.);
        assert_eq!(inside_person(1.), 1.);
        assert_eq!(blend_coefficients(0., 0.3, 40.), (1., 0.));
        assert_eq!(blend_coefficients(1., 0.3, 40.), (0.3, 40.));
        let (gain, bias) = blend_coefficients(0.5, 0.2, 100.);
        assert_eq!(apply_coefficients(100, gain, bias), 110);
    }

    fn noisy_plane() -> Vec<u8> {
        (0..WIDTH * HEIGHT)
            .map(|index| {
                let (x, y) = (index % WIDTH, index / WIDTH);
                let base = if x < WIDTH / 2 { 60 } else { 200 };
                (base + ((x * 7 + y * 13) % 9) as i32 - 4) as u8
            })
            .collect()
    }

    fn roughness(plane: &[u8], y: usize, from: usize, to: usize) -> i32 {
        (from..to).map(|x| (i32::from(plane[y * WIDTH + x + 1]) - i32::from(plane[y * WIDTH + x])).abs()).sum()
    }

    #[test]
    fn soft_focus_smooths_noise_inside_the_mask_keeps_edges_and_spares_the_outside() {
        let original = noisy_plane();
        let mut softened = original.clone();
        let mut mask = vec![0f32; MASK_WIDTH * MASK_HEIGHT];
        for row in 0..MASK_HEIGHT {
            mask[row * MASK_WIDTH..][..MASK_WIDTH / 2 + 20].fill(1.);
        }
        soften_luma(&mut softened, WIDTH, WIDTH, HEIGHT, &mask, 100);
        assert!(roughness(&softened, 18, 2, 20) < roughness(&original, 18, 2, 20));
        assert!(i32::from(softened[18 * WIDTH + WIDTH / 2 + 2]) - i32::from(softened[18 * WIDTH + WIDTH / 2 - 3]) > 100);
        assert_eq!(softened[18 * WIDTH + WIDTH - 4], original[18 * WIDTH + WIDTH - 4]);
        let mut untouched = original.clone();
        soften_luma(&mut untouched, WIDTH, WIDTH, HEIGHT, &mask, 0);
        assert_eq!(untouched, original);
    }
}
