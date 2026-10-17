use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

use crate::camera::i420_from_rgba;
use crate::mask::{Axis, axis};
use crate::matte::{Matte, box_filter};

const BACKGROUND_SCALE: usize = 4;
const BLUR_RADIUS: usize = 3;
const BLUR_PASSES: usize = 2;
const ESTIMATE_REGULARISER: f32 = 0.005;
const DEBLEED_RADIUS: usize = 2;
const MIN_FOREGROUND_ALPHA: f32 = 0.05;
const WRAP_RADIUS: usize = 1;
const WRAP_PASSES: usize = 2;
const WRAP_STRENGTH: f32 = 0.3;
const MAX_BANDS: usize = 4;

struct Grid {
    values: Vec<f32>,
    width: usize,
    across: Axis,
    down: Axis,
}

impl Grid {
    fn new(values: Vec<f32>, (width, height): (usize, usize), (plane_width, plane_height): (usize, usize)) -> Grid {
        Grid { values, width, across: axis(plane_width, width), down: axis(plane_height, height) }
    }

    fn row(&self, row: usize) -> (usize, usize, f32) {
        (self.down.first[row] * self.width, self.down.second[row] * self.width, self.down.weight[row])
    }

    fn at(&self, (top, bottom, vertical): (usize, usize, f32), column: usize) -> f32 {
        let (left, right, horizontal) = (self.across.first[column], self.across.second[column], self.across.weight[column]);
        let upper = self.values[top + left] * (1. - horizontal) + self.values[top + right] * horizontal;
        let lower = self.values[bottom + left] * (1. - horizontal) + self.values[bottom + right] * horizontal;
        upper * (1. - vertical) + lower * vertical
    }
}

fn cell_means(plane: &[u8], stride: usize, width: usize, height: usize, factor: usize) -> (Vec<f32>, usize, usize) {
    let (small_width, small_height) = (width.div_ceil(factor), height.div_ceil(factor));
    let mut small = vec![0f32; small_width * small_height];
    let mut sums = vec![0u32; small_width];
    for (row, line) in small.chunks_exact_mut(small_width).enumerate() {
        let rows = row * factor..((row + 1) * factor).min(height);
        sums.fill(0);
        for source_row in rows.clone() {
            for (sum, group) in sums.iter_mut().zip(plane[source_row * stride..][..width].chunks(factor)) {
                *sum += group.iter().map(|&value| u32::from(value)).sum::<u32>();
            }
        }
        for (column, (slot, sum)) in line.iter_mut().zip(&sums).enumerate() {
            let columns = factor.min(width - column * factor);
            *slot = *sum as f32 / (rows.len() * columns) as f32;
        }
    }
    (small, small_width, small_height)
}

fn blurred(values: &[f32], width: usize, height: usize, radius: usize, passes: usize) -> Vec<f32> {
    (0..passes).fold(values.to_vec(), |current, _| box_filter(&current, width, height, radius))
}

fn dilated(values: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut across = vec![0f32; values.len()];
    for (line, output) in values.chunks_exact(width).zip(across.chunks_exact_mut(width)) {
        for (column, slot) in output.iter_mut().enumerate() {
            *slot = line[column.saturating_sub(1)..(column + 2).min(width)].iter().copied().fold(0f32, f32::max);
        }
    }
    let mut output = vec![0f32; values.len()];
    for row in 0..height {
        let (first, last) = (row.saturating_sub(1), (row + 1).min(height - 1));
        let target = &mut output[row * width..][..width];
        for neighbour in first..=last {
            target.iter_mut().zip(&across[neighbour * width..][..width]).for_each(|(slot, &value)| *slot = slot.max(value));
        }
    }
    output
}

/// The person on the small background grid, the same for luma and both chroma planes.
struct PersonGrid {
    width: usize,
    height: usize,
    weight: Vec<f32>,
    blurred_weight: Vec<f32>,
    wide: Option<Vec<f32>>,
}

impl PersonGrid {
    fn new(matte: &Matte, (radius, passes): (usize, usize), wrap: bool) -> PersonGrid {
        let (person, width, height) = cell_means(&matte.alpha, matte.width, matte.width, matte.height, BACKGROUND_SCALE);
        let weight: Vec<f32> = dilated(&person, width, height).iter().map(|&alpha| (1. - alpha / 255.).powi(2)).collect();
        let blurred_weight = blurred(&weight, width, height, radius, passes);
        let wide = wrap.then(|| blurred(&person, width, height, WRAP_RADIUS, WRAP_PASSES).iter().map(|alpha| alpha / 255.).collect());
        PersonGrid { width, height, weight, blurred_weight, wide }
    }
}

/// Normalised convolution: only pixels the matte calls background count, so the person never bleeds into it.
fn background_estimate(plane: &[u8], stride: usize, (width, height): (usize, usize), factor: usize, person: &PersonGrid, (radius, passes): (usize, usize)) -> Vec<f32> {
    let (values, small_width, small_height) = cell_means(plane, stride, width, height, factor);
    debug_assert_eq!((small_width, small_height), (person.width, person.height));
    let weighted: Vec<f32> = values.iter().zip(&person.weight).map(|(value, weight)| value * weight).collect();
    let (weighted, plain) = (blurred(&weighted, small_width, small_height, radius, passes), blurred(&values, small_width, small_height, radius, passes));
    weighted.iter().zip(&person.blurred_weight).zip(&plain).map(|((weighted, weight), plain)| (weighted + ESTIMATE_REGULARISER * plain) / (weight + ESTIMATE_REGULARISER)).collect()
}

pub struct PicturePlanes {
    width: usize,
    height: usize,
    planes: [Vec<u8>; 3],
    soft: [(Vec<f32>, usize, usize); 3],
}

impl PicturePlanes {
    pub fn new(rgb: &[u8], width: usize, height: usize) -> PicturePlanes {
        let rgba: Vec<u8> = rgb.chunks_exact(3).flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255]).collect();
        let buffer = i420_from_rgba(&rgba, width as u32, height as u32);
        let (stride_y, stride_u, stride_v) = buffer.strides();
        let (plane_y, plane_u, plane_v) = buffer.data();
        let (chroma_width, chroma_height) = (width.div_ceil(2), height.div_ceil(2));
        let copy = |plane: &[u8], stride: u32, plane_width: usize, plane_height: usize| (0..plane_height).flat_map(|row| plane[row * stride as usize..][..plane_width].to_vec()).collect::<Vec<u8>>();
        let planes = [copy(plane_y, stride_y, width, height), copy(plane_u, stride_u, chroma_width, chroma_height), copy(plane_v, stride_v, chroma_width, chroma_height)];
        let soft = std::array::from_fn(|index| {
            let (plane_width, plane_height, factor) = if index == 0 { (width, height, BACKGROUND_SCALE) } else { (chroma_width, chroma_height, BACKGROUND_SCALE / 2) };
            let (values, small_width, small_height) = cell_means(&planes[index], plane_width, plane_width, plane_height, factor);
            (blurred(&values, small_width, small_height, BLUR_RADIUS, BLUR_PASSES), small_width, small_height)
        });
        PicturePlanes { width, height, planes, soft }
    }

    pub fn fits(&self, width: usize, height: usize) -> bool {
        (self.width, self.height) == (width, height)
    }
}

pub enum Backdrop<'a> {
    Blur,
    Picture(&'a PicturePlanes),
}

struct PlaneJob<'a> {
    matte: &'a Matte,
    estimate: Grid,
    picture: Option<(&'a [u8], Grid, Grid)>,
}

impl PlaneJob<'_> {
    fn rows(&self, plane: &mut [u8], stride: usize, first_row: usize) {
        for (offset, line) in plane.chunks_mut(stride).enumerate() {
            let length = self.matte.width.min(line.len());
            self.row(&mut line[..length], first_row + offset);
        }
    }

    fn row(&self, line: &mut [u8], row: usize) {
        let width = self.matte.width;
        let alphas = &self.matte.alpha[row * width..][..width];
        let estimate_row = self.estimate.row(row);
        match &self.picture {
            None => {
                for (column, (value, &alpha)) in line.iter_mut().zip(alphas).enumerate() {
                    if alpha == u8::MAX {
                        continue;
                    }
                    let person = f32::from(alpha) / 255.;
                    *value = (f32::from(*value) * person + self.estimate.at(estimate_row, column) * (1. - person) + 0.5) as u8;
                }
            }
            Some((picture, soft, wide_alpha)) => {
                let picture = &picture[row * width..][..width];
                let (soft_row, wide_row) = (soft.row(row), wide_alpha.row(row));
                for (column, (value, &alpha)) in line.iter_mut().zip(alphas).enumerate() {
                    let wrap = WRAP_STRENGTH * (1. - wide_alpha.at(wide_row, column));
                    if alpha == u8::MAX && wrap < 1e-3 {
                        continue;
                    }
                    let person = f32::from(alpha) / 255.;
                    let background = f32::from(picture[column]);
                    let foreground = if person >= 1. {
                        f32::from(*value)
                    } else {
                        ((f32::from(*value) - (1. - person) * self.estimate.at(estimate_row, column)) / person.max(MIN_FOREGROUND_ALPHA)).clamp(0., 255.)
                    };
                    let wrapped = foreground + wrap * (soft.at(soft_row, column) - foreground);
                    *value = (wrapped * person + background * (1. - person) + 0.5).clamp(0., 255.) as u8;
                }
            }
        }
    }
}

fn plane_job<'a>(plane: &[u8], stride: usize, matte: &'a Matte, factor: usize, person: &PersonGrid, reach: (usize, usize), picture: Option<(&'a [u8], &(Vec<f32>, usize, usize))>) -> PlaneJob<'a> {
    let size = (matte.width, matte.height);
    let estimate = background_estimate(plane, stride, size, factor, person, reach);
    let picture = picture.map(|(planar, (soft, soft_width, soft_height))| {
        let wide = person.wide.clone().unwrap_or_else(|| vec![0.; person.width * person.height]);
        (planar, Grid::new(soft.clone(), (*soft_width, *soft_height), size), Grid::new(wide, (person.width, person.height), size))
    });
    PlaneJob { matte, estimate: Grid::new(estimate, (person.width, person.height), size), picture }
}

pub fn composite(buffer: &mut I420Buffer, matte: &Matte, backdrop: Backdrop) {
    let (width, height) = (buffer.width() as usize, buffer.height() as usize);
    let chroma_height = height.div_ceil(2);
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (stride_y, stride_u, stride_v) = (stride_y as usize, stride_u as usize, stride_v as usize);
    let picture = match backdrop {
        Backdrop::Picture(picture) if picture.fits(width, height) => Some(picture),
        _ => None,
    };
    let reach = if picture.is_some() { (DEBLEED_RADIUS, 1) } else { (BLUR_RADIUS, BLUR_PASSES) };
    let (chroma_matte, person) = std::thread::scope(|scope| {
        let halved = scope.spawn(|| matte.halved());
        let person = PersonGrid::new(matte, reach, picture.is_some());
        (halved.join().expect("chroma matte"), person)
    });
    let (plane_y, plane_u, plane_v) = buffer.data_mut();
    let layer = |index: usize| picture.map(|picture| (picture.planes[index].as_slice(), &picture.soft[index]));
    let jobs = std::thread::scope(|scope| {
        let (person, chroma_matte, layer) = (&person, &chroma_matte, &layer);
        let (blue, red) = (&*plane_u, &*plane_v);
        let blue = scope.spawn(move || plane_job(blue, stride_u, chroma_matte, BACKGROUND_SCALE / 2, person, reach, layer(1)));
        let red = scope.spawn(move || plane_job(red, stride_v, chroma_matte, BACKGROUND_SCALE / 2, person, reach, layer(2)));
        let luma = plane_job(plane_y, stride_y, matte, BACKGROUND_SCALE, person, reach, layer(0));
        [luma, blue.join().expect("blue job"), red.join().expect("red job")]
    });
    let bands = std::thread::available_parallelism().map_or(1, usize::from).min(MAX_BANDS);
    std::thread::scope(|scope| {
        for (job, plane, stride, rows) in [(&jobs[0], plane_y, stride_y, height), (&jobs[1], plane_u, stride_u, chroma_height), (&jobs[2], plane_v, stride_v, chroma_height)] {
            let rows_per_band = rows.div_ceil(bands);
            for (index, band) in plane.chunks_mut(rows_per_band * stride).enumerate() {
                scope.spawn(move || job.rows(band, stride, index * rows_per_band));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 320;
    const HEIGHT: usize = 180;
    const LEFT: usize = 120;
    const RIGHT: usize = 200;

    fn frame(person_luma: u8, background_luma: u8) -> I420Buffer {
        let mut buffer = I420Buffer::new(WIDTH as u32, HEIGHT as u32);
        let stride = buffer.strides().0 as usize;
        let (plane_y, plane_u, plane_v) = buffer.data_mut();
        for row in 0..HEIGHT {
            for column in 0..WIDTH {
                plane_y[row * stride + column] = if (LEFT..RIGHT).contains(&column) { person_luma } else { background_luma };
            }
        }
        plane_u.fill(128);
        plane_v.fill(128);
        buffer
    }

    fn matte(edge: impl Fn(usize) -> u8) -> Matte {
        Matte { width: WIDTH, height: HEIGHT, alpha: (0..WIDTH * HEIGHT).map(|index| edge(index % WIDTH)).collect() }
    }

    fn luma(buffer: &I420Buffer, column: usize) -> u8 {
        let stride = buffer.strides().0 as usize;
        buffer.data().0[HEIGHT / 2 * stride + column]
    }

    #[test]
    fn the_blurred_background_takes_no_colour_from_the_person() {
        let mut buffer = frame(250, 60);
        let exact = matte(|column| if (LEFT..RIGHT).contains(&column) { 255 } else { 0 });
        composite(&mut buffer, &exact, Backdrop::Blur);
        for column in [LEFT - 2, LEFT - 6, RIGHT + 1, RIGHT + 5] {
            assert!(luma(&buffer, column).abs_diff(60) <= 3, "column {column}: {}", luma(&buffer, column));
        }
        assert_eq!(luma(&buffer, WIDTH / 2), 250);
    }

    #[test]
    fn a_picture_replaces_the_old_background_inside_soft_edges() {
        let mut buffer = frame(40, 240);
        let soft = matte(|column| match column {
            column if (LEFT + 4..RIGHT - 4).contains(&column) => 255,
            column if (LEFT..RIGHT).contains(&column) => 128,
            _ => 0,
        });
        let stride = buffer.strides().0 as usize;
        let plane = buffer.data_mut().0;
        for row in 0..HEIGHT {
            for column in (LEFT..LEFT + 4).chain(RIGHT - 4..RIGHT) {
                plane[row * stride + column] = 140;
            }
        }
        let black = PicturePlanes::new(&vec![0u8; WIDTH * HEIGHT * 3], WIDTH, HEIGHT);
        composite(&mut buffer, &soft, Backdrop::Picture(&black));
        let edge = luma(&buffer, LEFT + 1);
        assert!(edge < 50, "the old bright background shows through the soft edge: {edge}");
        assert!(luma(&buffer, 4) <= 17, "background {}", luma(&buffer, 4));
        assert!(luma(&buffer, WIDTH / 2).abs_diff(40) <= 1);
    }

    #[test]
    fn light_from_the_picture_wraps_onto_the_edge_of_the_person() {
        let mut buffer = frame(40, 40);
        let exact = matte(|column| if (LEFT..RIGHT).contains(&column) { 255 } else { 0 });
        let white = PicturePlanes::new(&vec![255u8; WIDTH * HEIGHT * 3], WIDTH, HEIGHT);
        composite(&mut buffer, &exact, Backdrop::Picture(&white));
        assert!(luma(&buffer, LEFT) > 60, "edge {}", luma(&buffer, LEFT));
        assert!(luma(&buffer, WIDTH / 2).abs_diff(40) <= 1);
    }
}
