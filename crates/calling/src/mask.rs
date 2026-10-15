/// Person probability (0..1) on a grid of its own size, independent of the frame size.
/// `matte` marks a matting model's output: already a soft alpha with real edges, not a coarse probability.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mask {
    pub width: usize,
    pub height: usize,
    pub values: Vec<f32>,
    pub matte: bool,
}

impl Mask {
    pub fn new(width: usize, height: usize, values: Vec<f32>) -> Mask {
        assert_eq!(values.len(), width * height, "mask values match its size");
        Mask { width, height, values, matte: false }
    }

    pub fn filled(width: usize, height: usize, value: f32) -> Mask {
        Mask::new(width, height, vec![value; width * height])
    }

    pub fn into_matte(self) -> Mask {
        Mask { matte: true, ..self }
    }

    pub fn from_fn(width: usize, height: usize, person: impl Fn(f32, f32) -> f32) -> Mask {
        let values = (0..width * height).map(|index| person(((index % width) as f32 + 0.5) / width as f32, ((index / width) as f32 + 0.5) / height as f32)).collect();
        Mask::new(width, height, values)
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn resampled(&self, width: usize, height: usize) -> Vec<f32> {
        let (across, down) = (axis(width, self.width), axis(height, self.height));
        let mut output = Vec::with_capacity(width * height);
        for row in 0..height {
            let (top, bottom, vertical) = (down.first[row] * self.width, down.second[row] * self.width, down.weight[row]);
            for column in 0..width {
                let (left, right, horizontal) = (across.first[column], across.second[column], across.weight[column]);
                let upper = self.values[top + left] * (1. - horizontal) + self.values[top + right] * horizontal;
                let lower = self.values[bottom + left] * (1. - horizontal) + self.values[bottom + right] * horizontal;
                output.push(upper * (1. - vertical) + lower * vertical);
            }
        }
        output
    }
}

pub(crate) struct Axis {
    pub(crate) first: Vec<usize>,
    pub(crate) second: Vec<usize>,
    pub(crate) weight: Vec<f32>,
}

pub(crate) fn axis(output: usize, input: usize) -> Axis {
    let mut axis = Axis { first: Vec::with_capacity(output), second: Vec::with_capacity(output), weight: Vec::with_capacity(output) };
    for index in 0..output {
        let position = ((index as f32 + 0.5) * input as f32 / output as f32 - 0.5).clamp(0., (input - 1) as f32);
        let first = position.floor() as usize;
        axis.first.push(first);
        axis.second.push((first + 1).min(input - 1));
        axis.weight.push(position - first as f32);
    }
    axis
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_flat_masks_and_interpolates_steps() {
        assert!(Mask::filled(4, 3, 0.7).resampled(9, 5).iter().all(|&value| (value - 0.7).abs() < 1e-6));
        let step = Mask::new(2, 1, vec![0., 1.]);
        assert_eq!(step.resampled(4, 1), vec![0., 0.25, 0.75, 1.]);
        let centre = Mask::from_fn(4, 2, |x, y| x + y);
        assert_eq!(centre.values, vec![0.375, 0.625, 0.875, 1.125, 0.875, 1.125, 1.375, 1.625]);
    }
}
