use std::sync::Arc;
use std::time::Instant;

use calling::blur::{BlurSettings, MaskSource, Segmenter, compose};
use calling::effects::{CameraEffects, EffectsStage};
use calling::framing::{Crop, crop_and_scale};
use calling::luma_effects::soften_luma;
use libwebrtc::video_frame::I420Buffer;

const WIDTH: usize = 640;
const HEIGHT: usize = 360;
const SEND_WIDTH: u32 = 1280;
const SEND_HEIGHT: u32 = 720;
const FRAMES: usize = 200;
const WARMUP: usize = 10;

fn synthetic() -> Vec<u8> {
    (0..WIDTH * HEIGHT * 4).map(|index| if index % 4 == 3 { 255 } else { ((index / 4 % WIDTH + index / 4 / WIDTH) * 255 / (WIDTH + HEIGHT)) as u8 }).collect()
}

fn send_frame() -> I420Buffer {
    let mut buffer = I420Buffer::new(SEND_WIDTH, SEND_HEIGHT);
    let stride = buffer.strides().0 as usize;
    let (plane_y, plane_u, plane_v) = buffer.data_mut();
    for (index, value) in plane_y.iter_mut().enumerate() {
        *value = (((index % stride) + index / stride) * 255 / (SEND_WIDTH as usize + SEND_HEIGHT as usize)) as u8;
    }
    plane_u.fill(128);
    plane_v.fill(128);
    buffer
}

fn milliseconds_per_frame(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000. / FRAMES as f64
}

fn stage_cost(label: &str, blur: bool, effects: CameraEffects) {
    let settings = Arc::new(BlurSettings::default());
    settings.set_enabled(blur);
    settings.set_effects(effects);
    let mut stage = EffectsStage::new(settings);
    let source = send_frame();
    let run = |stage: &mut EffectsStage| {
        let mut frame = I420Buffer::new(SEND_WIDTH, SEND_HEIGHT);
        let (source_y, source_u, source_v) = source.data();
        let (target_y, target_u, target_v) = frame.data_mut();
        target_y.copy_from_slice(source_y);
        target_u.copy_from_slice(source_u);
        target_v.copy_from_slice(source_v);
        stage.apply_i420(frame)
    };
    for _ in 0..WARMUP {
        run(&mut stage);
    }
    let started = Instant::now();
    for _ in 0..FRAMES {
        run(&mut stage);
    }
    println!("{label} {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let input = arguments.next();
    let output = arguments.next();
    let frame = input.map_or_else(synthetic, |path| std::fs::read(path).expect("raw rgba 640x360 input"));
    assert_eq!(frame.len(), WIDTH * HEIGHT * 4, "input must be raw RGBA {WIDTH}x{HEIGHT}");
    let mut masks = MaskSource::new().expect("model");
    let segmenter = Segmenter::new().expect("model");
    for _ in 0..WARMUP {
        masks.refresh(|| (frame.clone(), WIDTH, HEIGHT));
    }
    let started = Instant::now();
    for _ in 0..FRAMES {
        segmenter.mask(&frame, WIDTH, HEIGHT).expect("mask");
    }
    println!("segmentation only: {:.2} ms/frame", milliseconds_per_frame(started));
    let started = Instant::now();
    let mut result = frame.clone();
    for _ in 0..FRAMES {
        result.copy_from_slice(&frame);
        masks.refresh(|| (frame.clone(), WIDTH, HEIGHT));
        compose(&mut result, WIDTH, HEIGHT, masks.mask());
    }
    println!("segment + blur + compose {WIDTH}x{HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
    if let Some(path) = output {
        std::fs::write(path, &result).expect("output");
    }
    let off = CameraEffects::default();
    let all = CameraEffects { auto_framing: true, adjust_brightness: true, soft_focus: true, ..off };
    stage_cost("stage, brightness", false, CameraEffects { adjust_brightness: true, ..off });
    stage_cost("stage, soft focus", false, CameraEffects { soft_focus: true, ..off });
    stage_cost("stage, auto framing", false, CameraEffects { auto_framing: true, ..off });
    stage_cost("stage, all three", false, all);
    stage_cost("stage, all three + blur", true, all);
    stage_cost("stage, blur only", true, off);
    let mut frame = send_frame();
    let stride = frame.strides().0 as usize;
    let oval: Vec<f32> = (0..calling::blur::MASK_WIDTH * calling::blur::MASK_HEIGHT)
        .map(|index| {
            let (x, y) = ((index % calling::blur::MASK_WIDTH) as f32 / calling::blur::MASK_WIDTH as f32 - 0.5, (index / calling::blur::MASK_WIDTH) as f32 / calling::blur::MASK_HEIGHT as f32 - 0.5);
            if (x * x) / 0.04 + (y * y) / 0.09 <= 1. { 1. } else { 0. }
        })
        .collect();
    let started = Instant::now();
    for _ in 0..FRAMES {
        soften_luma(frame.data_mut().0, stride, SEND_WIDTH as usize, SEND_HEIGHT as usize, &oval, 100);
    }
    println!("soft focus only, oval person {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
    let source = send_frame();
    let crop = Crop { center_x: 0.5, center_y: 0.5, zoom: 1.6 };
    let started = Instant::now();
    for _ in 0..FRAMES {
        std::hint::black_box(crop_and_scale(&source, &crop));
    }
    println!("crop and scale {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
}
