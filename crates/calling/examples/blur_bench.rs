use std::time::Instant;

use calling::blur::{BackgroundBlur, Segmenter};

const WIDTH: usize = 640;
const HEIGHT: usize = 360;
const FRAMES: usize = 200;
const WARMUP: usize = 10;

fn synthetic() -> Vec<u8> {
    (0..WIDTH * HEIGHT * 4).map(|index| if index % 4 == 3 { 255 } else { ((index / 4 % WIDTH + index / 4 / WIDTH) * 255 / (WIDTH + HEIGHT)) as u8 }).collect()
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let input = arguments.next();
    let output = arguments.next();
    let frame = input.map_or_else(synthetic, |path| std::fs::read(path).expect("raw rgba 640x360 input"));
    assert_eq!(frame.len(), WIDTH * HEIGHT * 4, "input must be raw RGBA {WIDTH}x{HEIGHT}");
    let mut blur = BackgroundBlur::new().expect("model");
    let segmenter = Segmenter::new().expect("model");
    for _ in 0..WARMUP {
        blur.apply(&mut frame.clone(), WIDTH, HEIGHT, None);
    }
    let started = Instant::now();
    for _ in 0..FRAMES {
        segmenter.mask(&frame, WIDTH, HEIGHT).expect("mask");
    }
    println!("segmentation only: {:.2} ms/frame", started.elapsed().as_secs_f64() * 1000. / FRAMES as f64);
    let started = Instant::now();
    let mut result = frame.clone();
    for _ in 0..FRAMES {
        result.copy_from_slice(&frame);
        blur.apply(&mut result, WIDTH, HEIGHT, None);
    }
    println!("segment + blur + compose {WIDTH}x{HEIGHT}: {:.2} ms/frame (running average {:.2} ms)", started.elapsed().as_secs_f64() * 1000. / FRAMES as f64, blur.average_ms());
    if let Some(path) = output {
        std::fs::write(path, &result).expect("output");
    }
}
