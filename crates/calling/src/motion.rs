use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

use crate::mask::{Mask, axis};

pub const GRID_WIDTH: usize = 160;
const BLOCK: usize = 4;
const WINDOW: usize = 8;
const COARSE_REACH: isize = 4;
const FINE_REACH: isize = 1;
const MOTION_COST: f32 = 0.4;
const PERSON_SHARE: f32 = 0.2;
const MAX_BANDS: usize = 4;

/// Mean luma (0..255) on a grid about `GRID_WIDTH` cells wide; the frame a mask was made from and the frame it is shown on are compared on it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LumaGrid {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
}

impl LumaGrid {
    pub fn of(buffer: &I420Buffer) -> LumaGrid {
        LumaGrid::of_plane(buffer.data().0, buffer.strides().0 as usize, buffer.width() as usize, buffer.height() as usize)
    }

    pub fn of_plane(plane: &[u8], stride: usize, width: usize, height: usize) -> LumaGrid {
        let factor = (width / GRID_WIDTH).max(1);
        let (grid_width, grid_height) = (width / factor, height / factor);
        let mut values = vec![0f32; grid_width * grid_height];
        let mut sums = vec![0u32; grid_width];
        let scale = 1. / (factor * factor) as f32;
        for (row, line) in values.chunks_exact_mut(grid_width).enumerate() {
            sums.fill(0);
            for source_row in row * factor..(row + 1) * factor {
                for (sum, group) in sums.iter_mut().zip(plane[source_row * stride..][..grid_width * factor].chunks_exact(factor)) {
                    *sum += group.iter().map(|&value| u32::from(value)).sum::<u32>();
                }
            }
            for (slot, &sum) in line.iter_mut().zip(&sums) {
                *slot = sum as f32 * scale;
            }
        }
        LumaGrid { width: grid_width, height: grid_height, values }
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn halved(&self) -> LumaGrid {
        let (width, height) = (self.width / 2, self.height / 2);
        let mut values = Vec::with_capacity(width * height);
        for row in 0..height {
            for column in 0..width {
                let at = |y: usize, x: usize| self.values[(row * 2 + y) * self.width + column * 2 + x];
                values.push((at(0, 0) + at(0, 1) + at(1, 0) + at(1, 1)) / 4.);
            }
        }
        LumaGrid { width, height, values }
    }

    fn at(&self, column: isize, row: isize) -> f32 {
        let column = column.clamp(0, self.width as isize - 1) as usize;
        let row = row.clamp(0, self.height as isize - 1) as usize;
        self.values[row * self.width + column]
    }
}

/// Where each block of the current frame was in the reference frame, in grid cells.
#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub columns: usize,
    pub rows: usize,
    pub grid_width: usize,
    pub grid_height: usize,
    pub offsets: Vec<(f32, f32)>,
}

impl Flow {
    pub fn is_still(&self) -> bool {
        self.offsets.iter().all(|&(across, down)| across == 0. && down == 0.)
    }
}

struct Level<'a> {
    reference: &'a LumaGrid,
    current: &'a LumaGrid,
    person: Option<&'a LumaGrid>,
}

impl Level<'_> {
    fn fits(grid: &LumaGrid, (left, top): (isize, isize), size: isize) -> bool {
        left >= 0 && top >= 0 && left + size <= grid.width as isize && top + size <= grid.height as isize
    }

    /// Mean difference; where the reference mask shows the person, only person pixels count, so blocks on the edge follow the person instead of the still background.
    fn cost(&self, (left, top): (isize, isize), size: isize, (across, down): (isize, isize)) -> f32 {
        let (mut plain, mut weighted, mut weight) = (0f32, 0f32, 0f32);
        if Self::fits(self.current, (left, top), size) && Self::fits(self.reference, (left + across, top + down), size) {
            let length = size as usize;
            for row in top..top + size {
                let now = &self.current.values[row as usize * self.current.width + left as usize..][..length];
                let source = (row + down) as usize * self.reference.width + (left + across) as usize;
                let before = &self.reference.values[source..][..length];
                match self.person {
                    Some(person) => {
                        for ((now, before), share) in now.iter().zip(before).zip(&person.values[source..][..length]) {
                            let difference = (now - before).abs();
                            plain += difference;
                            weighted += difference * share;
                            weight += share;
                        }
                    }
                    None => plain += now.iter().zip(before).map(|(now, before)| (now - before).abs()).sum::<f32>(),
                }
            }
        } else {
            for row in top..top + size {
                for column in left..left + size {
                    let difference = (self.current.at(column, row) - self.reference.at(column + across, row + down)).abs();
                    let share = self.person.map_or(0., |person| person.at(column + across, row + down));
                    plain += difference;
                    weighted += difference * share;
                    weight += share;
                }
            }
        }
        let count = (size * size) as f32;
        let mean = if weight >= PERSON_SHARE * count { weighted / weight } else { plain / count };
        mean + MOTION_COST * (across.abs() + down.abs()) as f32
    }

    fn best_offset(&self, corner: (isize, isize), size: isize, centre: (isize, isize), reach: isize) -> ((isize, isize), f32) {
        let mut best = (centre, f32::MAX);
        for down in centre.1 - reach..=centre.1 + reach {
            for across in centre.0 - reach..=centre.0 + reach {
                let value = self.cost(corner, size, (across, down));
                if value < best.1 {
                    best = ((across, down), value);
                }
            }
        }
        best
    }
}

fn parabola(before: f32, at: f32, after: f32) -> f32 {
    let curvature = before - 2. * at + after;
    if curvature <= f32::EPSILON { 0. } else { ((before - after) / (2. * curvature)).clamp(-0.5, 0.5) }
}

fn median_of_nine(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

fn median_filtered(offsets: &[(f32, f32)], columns: usize, rows: usize) -> Vec<(f32, f32)> {
    let mut output = Vec::with_capacity(offsets.len());
    let (mut across, mut down) = (Vec::with_capacity(9), Vec::with_capacity(9));
    for row in 0..rows {
        for column in 0..columns {
            across.clear();
            down.clear();
            for neighbour_row in row.saturating_sub(1)..(row + 2).min(rows) {
                for neighbour_column in column.saturating_sub(1)..(column + 2).min(columns) {
                    let (x, y) = offsets[neighbour_row * columns + neighbour_column];
                    across.push(x);
                    down.push(y);
                }
            }
            output.push((median_of_nine(&mut across), median_of_nine(&mut down)));
        }
    }
    output
}

fn band_offsets(fine: &Level, coarse: &Level, columns: usize, rows: std::ops::Range<usize>) -> Vec<(f32, f32)> {
    let half_window = (WINDOW / 2) as isize;
    let size = WINDOW as isize;
    let mut offsets: Vec<(f32, f32)> = Vec::with_capacity(columns * rows.len());
    let first_row = rows.start;
    for row in rows {
        for column in 0..columns {
            let centre = ((column * BLOCK + BLOCK / 2) as isize, (row * BLOCK + BLOCK / 2) as isize);
            let coarse_corner = (centre.0 / 2 - half_window / 2, centre.1 / 2 - half_window / 2);
            let ((coarse_across, coarse_down), _) = coarse.best_offset(coarse_corner, half_window, (0, 0), COARSE_REACH);
            let corner = (centre.0 - half_window, centre.1 - half_window);
            let neighbour = |index: Option<usize>| {
                index.map(|index: usize| -> (isize, isize) {
                    let (across, down) = offsets[index];
                    (across.round() as isize, down.round() as isize)
                })
            };
            let left = neighbour((column > 0).then(|| offsets.len() - 1));
            let above = neighbour((row > first_row).then(|| offsets.len() - columns));
            let (found, found_cost) = [Some((coarse_across * 2, coarse_down * 2)), left, above]
                .into_iter()
                .flatten()
                .map(|start| fine.best_offset(corner, size, start, FINE_REACH))
                .min_by(|first, second| first.1.total_cmp(&second.1))
                .expect("the coarse guess is always there");
            if fine.cost(corner, size, (0, 0)) <= found_cost {
                offsets.push((0., 0.));
                continue;
            }
            let sample = |across: isize, down: isize| fine.cost(corner, size, (found.0 + across, found.1 + down));
            let fine_across = parabola(sample(-1, 0), found_cost, sample(1, 0));
            let fine_down = parabola(sample(0, -1), found_cost, sample(0, 1));
            offsets.push((found.0 as f32 + fine_across, found.1 as f32 + fine_down));
        }
    }
    offsets
}

/// Block matching, coarse to fine with a sub-cell fit; a small cost per cell of motion keeps flat and still areas at zero.
/// `person` is the mask of the reference frame; it may be empty.
pub fn estimate(reference: &LumaGrid, current: &LumaGrid, person: &Mask) -> Option<Flow> {
    if reference.is_empty() || (reference.width, reference.height) != (current.width, current.height) || current.width < WINDOW * 2 || current.height < WINDOW * 2 {
        return None;
    }
    let person = (!person.is_empty()).then(|| LumaGrid { width: reference.width, height: reference.height, values: person.resampled(reference.width, reference.height) });
    let coarse_person = person.as_ref().map(LumaGrid::halved);
    let (coarse_reference, coarse_current) = (reference.halved(), current.halved());
    let fine = Level { reference, current, person: person.as_ref() };
    let coarse = Level { reference: &coarse_reference, current: &coarse_current, person: coarse_person.as_ref() };
    let (columns, rows) = (current.width.div_ceil(BLOCK), current.height.div_ceil(BLOCK));
    let bands = std::thread::available_parallelism().map_or(1, usize::from).min(MAX_BANDS);
    let rows_per_band = rows.div_ceil(bands);
    let offsets: Vec<(f32, f32)> = std::thread::scope(|scope| {
        let (fine, coarse) = (&fine, &coarse);
        let workers: Vec<_> = (0..rows).step_by(rows_per_band).map(|first| scope.spawn(move || band_offsets(fine, coarse, columns, first..(first + rows_per_band).min(rows)))).collect();
        workers.into_iter().flat_map(|worker| worker.join().expect("block matching band")).collect()
    });
    let offsets = median_filtered(&offsets, columns, rows);
    Some(Flow { columns, rows, grid_width: current.width, grid_height: current.height, offsets })
}

fn sample(mask: &Mask, column: f32, row: f32) -> f32 {
    let column = column.clamp(0., (mask.width - 1) as f32);
    let row = row.clamp(0., (mask.height - 1) as f32);
    let (left, top) = (column.floor() as usize, row.floor() as usize);
    let (right, bottom) = ((left + 1).min(mask.width - 1), (top + 1).min(mask.height - 1));
    let (horizontal, vertical) = (column - left as f32, row - top as f32);
    let at = |x: usize, y: usize| mask.values[y * mask.width + x];
    let upper = at(left, top) * (1. - horizontal) + at(right, top) * horizontal;
    let lower = at(left, bottom) * (1. - horizontal) + at(right, bottom) * horizontal;
    upper * (1. - vertical) + lower * vertical
}

/// Moves the mask along `flow`: every cell takes the value from where its content was in the reference frame.
pub fn warp(mask: &Mask, flow: &Flow) -> Mask {
    if mask.is_empty() || flow.is_still() {
        return mask.clone();
    }
    let block_axis = |cells: usize, blocks: usize, grid: usize| {
        let mut positions = axis(cells, blocks);
        for index in 0..cells {
            let grid_position = (index as f32 + 0.5) * grid as f32 / cells as f32 - 0.5;
            let block_position = ((grid_position - (BLOCK / 2) as f32 + 0.5) / BLOCK as f32).clamp(0., (blocks - 1) as f32);
            positions.first[index] = block_position.floor() as usize;
            positions.second[index] = (positions.first[index] + 1).min(blocks - 1);
            positions.weight[index] = block_position - positions.first[index] as f32;
        }
        positions
    };
    let (across, down) = (block_axis(mask.width, flow.columns, flow.grid_width), block_axis(mask.height, flow.rows, flow.grid_height));
    let (scale_across, scale_down) = (mask.width as f32 / flow.grid_width as f32, mask.height as f32 / flow.grid_height as f32);
    let mut values = Vec::with_capacity(mask.values.len());
    for row in 0..mask.height {
        let (top, bottom, vertical) = (down.first[row] * flow.columns, down.second[row] * flow.columns, down.weight[row]);
        for column in 0..mask.width {
            let (left, right, horizontal) = (across.first[column], across.second[column], across.weight[column]);
            let blend = |pick: fn(&(f32, f32)) -> f32| {
                let upper = pick(&flow.offsets[top + left]) * (1. - horizontal) + pick(&flow.offsets[top + right]) * horizontal;
                let lower = pick(&flow.offsets[bottom + left]) * (1. - horizontal) + pick(&flow.offsets[bottom + right]) * horizontal;
                upper * (1. - vertical) + lower * vertical
            };
            let (offset_across, offset_down) = (blend(|offset| offset.0), blend(|offset| offset.1));
            values.push(sample(mask, column as f32 + offset_across * scale_across, row as f32 + offset_down * scale_down));
        }
    }
    Mask { width: mask.width, height: mask.height, values, matte: mask.matte }
}

/// The mask of `reference`, moved to where the person is in `current`.
pub fn propagate(mask: &Mask, reference: &LumaGrid, current: &LumaGrid) -> Mask {
    propagate_for_display(mask, reference, current).0
}

/// Like `propagate`, and answers whether anything moved; a moved matte has lost its exact edges and wants the guided filter again.
pub fn propagate_for_display(mask: &Mask, reference: &LumaGrid, current: &LumaGrid) -> (Mask, bool) {
    match estimate(reference, current, mask) {
        Some(flow) if !flow.is_still() => (warp(mask, &flow), true),
        _ => (mask.clone(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 160;
    const HEIGHT: usize = 90;

    fn texture(column: isize, row: isize) -> f32 {
        let hash = (column.wrapping_mul(73_856_093) ^ row.wrapping_mul(19_349_663)).rem_euclid(97) as f32;
        40. + hash * 1.5
    }

    fn scene(shift: isize) -> (LumaGrid, Mask) {
        let inside = |column: isize, row: isize| (60 + shift..100 + shift).contains(&column) && (20..80).contains(&row);
        let mut values = Vec::with_capacity(WIDTH * HEIGHT);
        let mut person = Vec::with_capacity(WIDTH * HEIGHT);
        for row in 0..HEIGHT as isize {
            for column in 0..WIDTH as isize {
                let person_here = inside(column, row);
                values.push(if person_here { 60. + texture(column - shift, row) } else { texture(column / 3, row / 3) });
                person.push(if person_here { 1. } else { 0. });
            }
        }
        (LumaGrid { width: WIDTH, height: HEIGHT, values }, Mask::new(WIDTH, HEIGHT, person))
    }

    fn error(mask: &Mask, truth: &Mask) -> f32 {
        mask.values.iter().zip(&truth.values).map(|(value, truth)| (value - truth).abs()).sum()
    }

    #[test]
    fn a_still_frame_keeps_the_mask_as_it_is() {
        let (grid, mask) = scene(0);
        let flow = estimate(&grid, &grid, &mask).expect("same size");
        assert!(flow.is_still());
        assert_eq!(propagate(&mask, &grid, &grid), mask);
    }

    #[test]
    fn the_mask_follows_the_person_to_the_current_frame() {
        let (reference, mask) = scene(0);
        for shift in [3, 6, -5] {
            let (current, truth) = scene(shift);
            let moved = propagate(&mask, &reference, &current);
            assert!(error(&moved, &truth) * 4. < error(&mask, &truth), "shift {shift}: {} vs stale {}", error(&moved, &truth), error(&mask, &truth));
        }
    }

    #[test]
    fn the_grid_averages_cells_of_the_frame() {
        let width = 320;
        let plane: Vec<u8> = (0..width * 4).map(|index| if index % width < 2 { 100 } else { 0 }).collect();
        let grid = LumaGrid::of_plane(&plane, width, width, 4);
        assert_eq!((grid.width, grid.height), (160, 2));
        assert_eq!(grid.values[0], 100.);
        assert_eq!(grid.values[1], 0.);
    }

    #[test]
    fn different_sizes_leave_the_mask_alone() {
        let (reference, mask) = scene(0);
        assert!(estimate(&reference, &LumaGrid::default(), &mask).is_none());
        assert_eq!(propagate(&mask, &LumaGrid::default(), &reference), mask);
    }
}
