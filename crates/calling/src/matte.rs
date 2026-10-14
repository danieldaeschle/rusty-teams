use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

use crate::blur::{Axis, MASK_HEIGHT, MASK_WIDTH, axis};
use crate::camera::i420_from_rgba;

const GUIDE_WIDTH: usize = 160;
const FILTER_RADIUS: usize = 3;
const FILTER_EPSILON: f32 = 3e-3;
const CHROMA_WEIGHT: f32 = 2.;
const MATTE_LOW: f32 = 0.45;
const MATTE_HIGH: f32 = 0.8;
const SURE_PERSON: f32 = 0.85;
const SURE_BACKGROUND: f32 = 0.1;
const CERTAINTY_RAMP: f32 = 0.05;
const MAX_BANDS: usize = 4;
const CHANNELS: usize = 3;

/// Person alpha (0..255) per pixel of a `width` x `height` frame.
pub struct Matte {
    pub width: usize,
    pub height: usize,
    pub alpha: Vec<u8>,
}

impl Matte {
    /// Same matte at half resolution, for 4:2:0 chroma planes.
    pub fn halved(&self) -> Matte {
        let (width, height) = (self.width.div_ceil(2), self.height.div_ceil(2));
        let mut alpha = vec![0u8; width * height];
        for (row, line) in alpha.chunks_exact_mut(width).enumerate() {
            let (top, bottom) = (row * 2, (row * 2 + 1).min(self.height - 1));
            for (column, slot) in line.iter_mut().enumerate() {
                let (left, right) = (column * 2, (column * 2 + 1).min(self.width - 1));
                let sum: u32 = [(top, left), (top, right), (bottom, left), (bottom, right)].iter().map(|&(y, x)| u32::from(self.alpha[y * self.width + x])).sum();
                *slot = ((sum + 2) / 4) as u8;
            }
        }
        Matte { width, height, alpha }
    }
}

/// The 4:2:0 planes of the frame the matte follows.
#[derive(Clone, Copy)]
pub struct Planes<'a> {
    pub luma: &'a [u8],
    pub luma_stride: usize,
    pub blue: &'a [u8],
    pub blue_stride: usize,
    pub red: &'a [u8],
    pub red_stride: usize,
    pub width: usize,
    pub height: usize,
}

impl<'a> Planes<'a> {
    pub fn of(buffer: &'a I420Buffer) -> Planes<'a> {
        let (luma_stride, blue_stride, red_stride) = buffer.strides();
        let (luma, blue, red) = buffer.data();
        Planes { luma, luma_stride: luma_stride as usize, blue, blue_stride: blue_stride as usize, red, red_stride: red_stride as usize, width: buffer.width() as usize, height: buffer.height() as usize }
    }

    fn pixel(&self, column: usize, row: usize) -> [f32; CHANNELS] {
        let chroma = |plane: &[u8], stride: usize| (f32::from(plane[row / 2 * stride + column / 2]) - 128.) * CHROMA_WEIGHT / 255.;
        [f32::from(self.luma[row * self.luma_stride + column]) / 255., chroma(self.blue, self.blue_stride), chroma(self.red, self.red_stride)]
    }
}

fn ramp(value: f32, start: f32) -> f32 {
    ((value - start) / CERTAINTY_RAMP).clamp(0., 1.)
}

fn edge_curve(value: f32) -> f32 {
    let t = ((value - MATTE_LOW) / (MATTE_HIGH - MATTE_LOW)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn box_filter(input: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let mut across = vec![0f32; input.len()];
    let mut prefix = vec![0f32; width + 1];
    for (line, output) in input.chunks_exact(width).zip(across.chunks_exact_mut(width)) {
        for (index, &value) in line.iter().enumerate() {
            prefix[index + 1] = prefix[index] + value;
        }
        for (index, slot) in output.iter_mut().enumerate() {
            let (low, high) = (index.saturating_sub(radius), (index + radius + 1).min(width));
            *slot = (prefix[high] - prefix[low]) / (high - low) as f32;
        }
    }
    let mut output = vec![0f32; input.len()];
    let mut sums = vec![0f32; width];
    for line in across.chunks_exact(width).take(radius.min(height)) {
        sums.iter_mut().zip(line).for_each(|(sum, value)| *sum += value);
    }
    for row in 0..height {
        if row + radius < height {
            sums.iter_mut().zip(&across[(row + radius) * width..][..width]).for_each(|(sum, value)| *sum += value);
        }
        if row > radius {
            sums.iter_mut().zip(&across[(row - radius - 1) * width..][..width]).for_each(|(sum, value)| *sum -= value);
        }
        let count = ((row + radius + 1).min(height) - row.saturating_sub(radius)) as f32;
        output[row * width..][..width].iter_mut().zip(&sums).for_each(|(slot, sum)| *slot = sum / count);
    }
    output
}

fn cell_means(plane: &[u8], stride: usize, subsampling: usize, (plane_width, plane_height): (usize, usize), factor: usize, (small_width, small_height): (usize, usize)) -> Vec<f32> {
    let span = |cell: usize, limit: usize| {
        let first = (cell * factor / subsampling).min(limit - 1);
        first..((cell + 1) * factor).div_ceil(subsampling).clamp(first + 1, limit)
    };
    let columns: Vec<_> = (0..small_width).map(|column| span(column, plane_width)).collect();
    let mut means = Vec::with_capacity(small_width * small_height);
    let mut sums = vec![0u32; plane_width];
    for row in 0..small_height {
        let rows = span(row, plane_height);
        sums.fill(0);
        for line in rows.clone() {
            sums.iter_mut().zip(&plane[line * stride..][..plane_width]).for_each(|(sum, &value)| *sum += u32::from(value));
        }
        for range in &columns {
            means.push(sums[range.clone()].iter().sum::<u32>() as f32 / (rows.len() * range.len()) as f32);
        }
    }
    means
}

fn small_guide(planes: &Planes, factor: usize) -> ([Vec<f32>; CHANNELS], usize, usize) {
    let small = (planes.width.div_ceil(factor), planes.height.div_ceil(factor));
    let chroma = (planes.width.div_ceil(2), planes.height.div_ceil(2));
    let luma = cell_means(planes.luma, planes.luma_stride, 1, (planes.width, planes.height), factor, small).into_iter().map(|value| value / 255.).collect();
    let colour = |plane: &[u8], stride: usize| cell_means(plane, stride, 2, chroma, factor, small).into_iter().map(|value| (value - 128.) * CHROMA_WEIGHT / 255.).collect();
    ([luma, colour(planes.blue, planes.blue_stride), colour(planes.red, planes.red_stride)], small.0, small.1)
}

fn small_mask(mask: &[f32], width: usize, height: usize) -> Vec<f32> {
    let (mask_x, mask_y) = (axis(width, MASK_WIDTH), axis(height, MASK_HEIGHT));
    let mut small = Vec::with_capacity(width * height);
    for row in 0..height {
        let (top, bottom, down) = (mask_y.first[row] * MASK_WIDTH, mask_y.second[row] * MASK_WIDTH, mask_y.weight[row]);
        for column in 0..width {
            let (left, right, across) = (mask_x.first[column], mask_x.second[column], mask_x.weight[column]);
            let upper = mask[top + left] * (1. - across) + mask[top + right] * across;
            let lower = mask[bottom + left] * (1. - across) + mask[bottom + right] * across;
            small.push(upper * (1. - down) + lower * down);
        }
    }
    small
}

struct Coefficients {
    slope: [Vec<f32>; CHANNELS],
    offset: Vec<f32>,
    width: usize,
}

fn solve(covariance: [[f32; CHANNELS]; CHANNELS], target: [f32; CHANNELS]) -> [f32; CHANNELS] {
    let [[a, b, c], [_, d, e], [_, _, f]] = covariance;
    let cofactors = [[d * f - e * e, c * e - b * f, b * e - c * d], [c * e - b * f, a * f - c * c, b * c - a * e], [b * e - c * d, b * c - a * e, a * d - b * b]];
    let determinant = a * cofactors[0][0] + b * cofactors[0][1] + c * cofactors[0][2];
    std::array::from_fn(|row| (0..CHANNELS).map(|column| cofactors[row][column] * target[column]).sum::<f32>() / determinant)
}

fn coefficients(guide: &[Vec<f32>; CHANNELS], person: &[f32], width: usize, height: usize) -> Coefficients {
    let mean = |values: Vec<f32>| box_filter(&values, width, height, FILTER_RADIUS);
    let mean_guide: [Vec<f32>; CHANNELS] = std::array::from_fn(|channel| mean(guide[channel].clone()));
    let mean_person = mean(person.to_vec());
    let mean_product: [Vec<f32>; CHANNELS] = std::array::from_fn(|channel| mean(guide[channel].iter().zip(person).map(|(guide, person)| guide * person).collect()));
    let pairs = [(0, 0), (0, 1), (0, 2), (1, 1), (1, 2), (2, 2)];
    let mean_pair: Vec<Vec<f32>> = pairs.iter().map(|&(first, second)| mean(guide[first].iter().zip(&guide[second]).map(|(first, second)| first * second).collect())).collect();
    let mut slope: [Vec<f32>; CHANNELS] = std::array::from_fn(|_| vec![0f32; person.len()]);
    let mut offset = vec![0f32; person.len()];
    for index in 0..person.len() {
        let mut covariance = [[0f32; CHANNELS]; CHANNELS];
        for (&(first, second), pair) in pairs.iter().zip(&mean_pair) {
            let value = pair[index] - mean_guide[first][index] * mean_guide[second][index] + if first == second { FILTER_EPSILON } else { 0. };
            covariance[first][second] = value;
            covariance[second][first] = value;
        }
        let target = std::array::from_fn(|channel| mean_product[channel][index] - mean_guide[channel][index] * mean_person[index]);
        let solution = solve(covariance, target);
        offset[index] = mean_person[index] - (0..CHANNELS).map(|channel| solution[channel] * mean_guide[channel][index]).sum::<f32>();
        for channel in 0..CHANNELS {
            slope[channel][index] = solution[channel];
        }
    }
    Coefficients { slope: slope.map(mean), offset: mean(offset), width }
}

struct Upsampler<'a> {
    coefficients: &'a Coefficients,
    prior: &'a [f32],
    planes: Planes<'a>,
    across: Axis,
    down: Axis,
}

impl Upsampler<'_> {
    fn rows(&self, alpha: &mut [u8], first_row: usize) {
        let small_width = self.coefficients.width;
        let mut lines = vec![[0f32; CHANNELS + 2]; small_width];
        for (offset, line) in alpha.chunks_exact_mut(self.planes.width).enumerate() {
            let row = first_row + offset;
            let (top, bottom, vertical) = (self.down.first[row] * small_width, self.down.second[row] * small_width, self.down.weight[row]);
            let blend = |grid: &[f32], column: usize| grid[top + column] * (1. - vertical) + grid[bottom + column] * vertical;
            for (column, slot) in lines.iter_mut().enumerate() {
                *slot = [blend(&self.coefficients.slope[0], column), blend(&self.coefficients.slope[1], column), blend(&self.coefficients.slope[2], column), blend(&self.coefficients.offset, column), blend(self.prior, column)];
            }
            for (column, slot) in line.iter_mut().enumerate() {
                let (left, right, horizontal) = (&lines[self.across.first[column]], &lines[self.across.second[column]], self.across.weight[column]);
                let at = |index: usize| left[index] * (1. - horizontal) + right[index] * horizontal;
                let prior = at(4);
                let (floor, ceiling) = (ramp(prior, SURE_PERSON), ramp(prior, SURE_BACKGROUND - CERTAINTY_RAMP));
                if floor >= 1. || ceiling <= 0. {
                    *slot = if floor >= 1. { u8::MAX } else { 0 };
                    continue;
                }
                let guide = self.planes.pixel(column, row);
                let person = at(0) * guide[0] + at(1) * guide[1] + at(2) * guide[2] + at(3);
                *slot = (edge_curve(person).max(floor).min(ceiling) * 255. + 0.5) as u8;
            }
        }
    }
}

/// Snaps the low-resolution `mask` to the colour edges of the full-resolution frame with a fast colour guided filter.
pub fn refine(mask: &[f32], planes: Planes) -> Matte {
    let (width, height) = (planes.width, planes.height);
    let factor = (width / GUIDE_WIDTH).max(1);
    let (guide, small_width, small_height) = small_guide(&planes, factor);
    let prior = small_mask(mask, small_width, small_height);
    let coefficients = coefficients(&guide, &prior, small_width, small_height);
    let upsampler = Upsampler { coefficients: &coefficients, prior: &prior, planes, across: axis(width, small_width), down: axis(height, small_height) };
    let mut alpha = vec![0u8; width * height];
    let bands = std::thread::available_parallelism().map_or(1, usize::from).min(MAX_BANDS);
    let rows_per_band = height.div_ceil(bands);
    std::thread::scope(|scope| {
        for (index, band) in alpha.chunks_mut(rows_per_band * width).enumerate() {
            let upsampler = &upsampler;
            scope.spawn(move || upsampler.rows(band, index * rows_per_band));
        }
    });
    Matte { width, height, alpha }
}

pub fn refine_rgba(mask: &[f32], rgba: &[u8], width: usize, height: usize) -> Matte {
    refine(mask, Planes::of(&i420_from_rgba(rgba, width as u32, height as u32)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 1280;
    const HEIGHT: usize = 720;
    const LEFT: usize = 403;
    const RIGHT: usize = 877;
    const TOP: usize = 161;
    const BOTTOM: usize = 598;
    const BLOCK: usize = 3;

    fn frame(width: usize, height: usize, luma: impl Fn(usize, usize) -> u8) -> I420Buffer {
        let mut buffer = I420Buffer::new(width as u32, height as u32);
        let stride = buffer.strides().0 as usize;
        let (plane_y, plane_u, plane_v) = buffer.data_mut();
        for row in 0..height {
            for column in 0..width {
                plane_y[row * stride + column] = luma(column, row);
            }
        }
        plane_u.fill(128);
        plane_v.fill(128);
        buffer
    }

    fn square_frame() -> I420Buffer {
        frame(WIDTH, HEIGHT, |column, row| if (LEFT..RIGHT).contains(&column) && (TOP..BOTTOM).contains(&row) { 200 } else { 50 })
    }

    fn blocky_mask() -> Vec<f32> {
        (0..MASK_WIDTH * MASK_HEIGHT)
            .map(|index| {
                let (block_column, block_row) = ((index % MASK_WIDTH) / BLOCK * BLOCK, (index / MASK_WIDTH) / BLOCK * BLOCK);
                let center_x = (block_column as f32 + BLOCK as f32 / 2.) * WIDTH as f32 / MASK_WIDTH as f32;
                let center_y = (block_row as f32 + BLOCK as f32 / 2.) * HEIGHT as f32 / MASK_HEIGHT as f32;
                let inside = (LEFT as f32..RIGHT as f32).contains(&center_x) && (TOP as f32..BOTTOM as f32).contains(&center_y);
                if inside { 0.9 } else { 0.1 }
            })
            .collect()
    }

    fn crossing(values: impl Iterator<Item = u8>) -> Vec<usize> {
        let values: Vec<u8> = values.collect();
        values.windows(2).enumerate().filter(|(_, pair)| (pair[0] < 128) != (pair[1] < 128)).map(|(index, _)| index + 1).collect()
    }

    #[test]
    fn a_blocky_mask_snaps_to_the_sharp_edge_of_the_frame() {
        let matte = refine(&blocky_mask(), Planes::of(&square_frame()));
        for row in [TOP + 40, HEIGHT / 2, BOTTOM - 40] {
            let edges = crossing(matte.alpha[row * WIDTH..][..WIDTH].iter().copied());
            assert_eq!(edges.len(), 2, "row {row}: {edges:?}");
            assert!(edges[0].abs_diff(LEFT) <= 2 && edges[1].abs_diff(RIGHT) <= 2, "row {row}: {edges:?}");
        }
        for column in [LEFT + 40, WIDTH / 2, RIGHT - 40] {
            let edges = crossing((0..HEIGHT).map(|row| matte.alpha[row * WIDTH + column]));
            assert!(edges.len() == 2 && edges[0].abs_diff(TOP) <= 2 && edges[1].abs_diff(BOTTOM) <= 2, "column {column}: {edges:?}");
        }
    }

    #[test]
    fn a_colour_edge_without_a_brightness_step_still_guides_the_matte() {
        let mut colour = frame(WIDTH, HEIGHT, |_, _| 120);
        let stride = colour.strides().1 as usize;
        let blue = colour.data_mut().1;
        for row in 0..HEIGHT / 2 {
            for column in 0..WIDTH / 2 {
                let inside = (LEFT / 2..RIGHT / 2).contains(&column) && (TOP / 2..BOTTOM / 2).contains(&row);
                blue[row * stride + column] = if inside { 170 } else { 90 };
            }
        }
        let matte = refine(&blocky_mask(), Planes::of(&colour));
        let edges = crossing(matte.alpha[HEIGHT / 2 * WIDTH..][..WIDTH].iter().copied());
        assert!(edges.len() == 2 && edges[0].abs_diff(LEFT) <= 2 && edges[1].abs_diff(RIGHT) <= 2, "{edges:?}");
    }

    #[test]
    fn no_background_shows_outside_the_true_edge() {
        let matte = refine(&blocky_mask(), Planes::of(&square_frame()));
        let line = &matte.alpha[HEIGHT / 2 * WIDTH..][..WIDTH];
        assert!(line[..LEFT - 1].iter().chain(&line[RIGHT + 1..]).all(|&alpha| alpha == 0), "{:?} {:?}", &line[LEFT - 8..LEFT + 2], &line[RIGHT - 2..RIGHT + 8]);
        assert!(line[LEFT + 1..RIGHT - 1].iter().all(|&alpha| alpha > 128), "{:?}", &line[RIGHT - 16..RIGHT]);
    }

    #[test]
    fn flat_regions_keep_the_mask_and_halving_averages() {
        let flat = frame(64, 36, |_, _| 120);
        let everyone = refine(&vec![1.; MASK_WIDTH * MASK_HEIGHT], Planes::of(&flat));
        assert!(everyone.alpha.iter().all(|&alpha| alpha == 255));
        let nobody = refine(&vec![0.; MASK_WIDTH * MASK_HEIGHT], Planes::of(&flat));
        assert!(nobody.alpha.iter().all(|&alpha| alpha == 0));
        let halved = Matte { width: 3, height: 2, alpha: vec![0, 255, 255, 0, 255, 255] }.halved();
        assert_eq!((halved.width, halved.height, halved.alpha), (2, 1, vec![128, 255]));
    }

    #[test]
    fn the_box_filter_averages_clamped_windows() {
        let filtered = box_filter(&[0., 3., 6., 9.], 4, 1, 1);
        assert_eq!(filtered, vec![1.5, 3., 6., 7.5]);
        let tall = box_filter(&[0., 3., 6., 9.], 1, 4, 1);
        assert_eq!(tall, vec![1.5, 3., 6., 7.5]);
    }
}
