use std::sync::Arc;
use std::time::{Duration, Instant};

use calling::camera::i420_from_rgba;
use calling::background::BackgroundPicture;
use calling::composite::{Backdrop, PicturePlanes, composite};
use calling::motion::{LumaGrid, propagate};
use calling::blur::{BlurSettings, MaskInference, MaskSource, compose, compose_i420, compose_image};
use calling::segmentation::FixedSegmenter;
use calling::segmentation::model::MEDIAPIPE;
use calling::segmentation::runtime::Device;
use calling::effects::{CameraEffects, EffectsStage};
use calling::framing::{Crop, crop_and_scale};
use calling::luma_effects::soften_luma;
use calling::matte::{Planes, refine};
use libwebrtc::native::yuv_helper::i420_to_abgr;
use libwebrtc::video_frame::{I420Buffer, VideoBuffer as _};

const WIDTH: usize = 640;
const HEIGHT: usize = 360;
const SEND_WIDTH: u32 = 1280;
const SEND_HEIGHT: u32 = 720;
const FRAMES: usize = 200;
const WARMUP: usize = 10;
const PACED_WARMUP: usize = 60;
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

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
    for _ in 0..PACED_WARMUP {
        run(&mut stage);
        std::thread::sleep(FRAME_INTERVAL);
    }
    let mut busy = Duration::ZERO;
    for _ in 0..FRAMES {
        let started = Instant::now();
        run(&mut stage);
        busy += started.elapsed();
        std::thread::sleep(FRAME_INTERVAL);
    }
    println!("{label} {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", busy.as_secs_f64() * 1000. / FRAMES as f64);
}

fn picture_backdrop() -> Arc<BackgroundPicture> {
    let (width, height) = (64, 36);
    let rgb = (0..width * height).flat_map(|index| {
        let (x, y) = (index % width, index / width);
        [(40 + x * 2) as u8, (90 + y) as u8, (150 + x) as u8]
    });
    Arc::new(BackgroundPicture::from_rgb(rgb.collect(), width, height).expect("backdrop"))
}

fn rgba_of(buffer: &I420Buffer) -> Vec<u8> {
    let (width, height) = (buffer.width(), buffer.height());
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data();
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    i420_to_abgr(plane_y, stride_y, plane_u, stride_u, plane_v, stride_v, &mut rgba, width * 4, width as i32, height as i32);
    rgba
}

fn save_png(path: &str, rgba: Vec<u8>) {
    image::RgbaImage::from_raw(SEND_WIDTH, SEND_HEIGHT, rgba).expect("frame size").save(path).expect("png written");
}

fn shots(input: &str, prefix: &str) {
    let photo = image::open(input).expect("photo").resize_to_fill(SEND_WIDTH, SEND_HEIGHT, image::imageops::FilterType::Triangle).to_rgba8().into_raw();
    let camera = i420_from_rgba(&photo, SEND_WIDTH, SEND_HEIGHT);
    let mask = FixedSegmenter::new(MEDIAPIPE, Device::Cpu { threads: 1 }).expect("model").infer(i420_from_rgba(&photo, SEND_WIDTH, SEND_HEIGHT)).expect("mask");
    let matte = refine(&mask, Planes::of(&camera));
    image::GrayImage::from_raw(SEND_WIDTH, SEND_HEIGHT, matte.alpha.clone()).expect("matte size").save(format!("{prefix}-matte.png")).expect("png written");
    let mut blurred = i420_from_rgba(&photo, SEND_WIDTH, SEND_HEIGHT);
    compose_i420(&mut blurred, &mask);
    save_png(&format!("{prefix}-blur.png"), rgba_of(&blurred));
    let mut replaced = rgba_of(&camera);
    let backdrop = PicturePlanes::new(&picture_backdrop().covering(SEND_WIDTH as usize, SEND_HEIGHT as usize), SEND_WIDTH as usize, SEND_HEIGHT as usize);
    compose_image(&mut replaced, SEND_WIDTH as usize, SEND_HEIGHT as usize, &matte, &backdrop);
    save_png(&format!("{prefix}-picture.png"), replaced);
}

fn main() {
    let mut arguments = std::env::args().skip(1).peekable();
    if arguments.next_if(|argument| argument == "--shots").is_some() {
        shots(&arguments.next().expect("photo path"), &arguments.next().expect("output prefix"));
        return;
    }
    let input = arguments.next();
    let output = arguments.next();
    let frame = input.map_or_else(synthetic, |path| std::fs::read(path).expect("raw rgba 640x360 input"));
    assert_eq!(frame.len(), WIDTH * HEIGHT * 4, "input must be raw RGBA {WIDTH}x{HEIGHT}");
    let mut masks = MaskSource::new().expect("model");
    let mut segmenter = FixedSegmenter::new(MEDIAPIPE, Device::Cpu { threads: 1 }).expect("model");
    for _ in 0..WARMUP {
        masks.refresh(|| i420_from_rgba(&frame, WIDTH as u32, HEIGHT as u32));
    }
    let started = Instant::now();
    for _ in 0..FRAMES {
        segmenter.infer(i420_from_rgba(&frame, WIDTH as u32, HEIGHT as u32)).expect("mask");
    }
    println!("segmentation only: {:.2} ms/frame", milliseconds_per_frame(started));
    let started = Instant::now();
    let mut result = frame.clone();
    for _ in 0..FRAMES {
        result.copy_from_slice(&frame);
        masks.refresh(|| i420_from_rgba(&frame, WIDTH as u32, HEIGHT as u32));
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
    let oval = calling::mask::Mask::from_fn(256, 144, |x, y| if ((x - 0.5) * (x - 0.5)) / 0.04 + ((y - 0.5) * (y - 0.5)) / 0.09 <= 1. { 1. } else { 0. });
    let started = Instant::now();
    for _ in 0..FRAMES {
        soften_luma(frame.data_mut().0, stride, SEND_WIDTH as usize, SEND_HEIGHT as usize, &oval, 100);
    }
    println!("soft focus only, oval person {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
    let started = Instant::now();
    for _ in 0..FRAMES {
        std::hint::black_box(refine(&oval, Planes::of(&frame)));
    }
    println!("matte refinement {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
    let matte = refine(&oval, Planes::of(&frame));
    let mut target = send_frame();
    let started = Instant::now();
    for _ in 0..FRAMES {
        composite(&mut target, &matte, Backdrop::Blur);
    }
    println!("blur composite only {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
    let reference = LumaGrid::of(&frame);
    let started = Instant::now();
    for _ in 0..FRAMES {
        std::hint::black_box(propagate(&oval, &reference, &LumaGrid::of(&target)));
    }
    println!("mask propagation (grid + block matching + warp) {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
    let backdrop = PicturePlanes::new(&picture_backdrop().covering(SEND_WIDTH as usize, SEND_HEIGHT as usize), SEND_WIDTH as usize, SEND_HEIGHT as usize);
    let camera = rgba_of(&frame);
    let mut replaced = camera.clone();
    let started = Instant::now();
    for _ in 0..FRAMES {
        replaced.copy_from_slice(&camera);
        let matte = refine(&oval, Planes::of(&frame));
        compose_image(&mut replaced, SEND_WIDTH as usize, SEND_HEIGHT as usize, &matte, &backdrop);
    }
    println!("picture compose {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
    let started = Instant::now();
    for _ in 0..FRAMES {
        let mut blurred = send_frame();
        compose_i420(&mut blurred, &oval);
    }
    println!("blur compose i420 {SEND_WIDTH}x{SEND_HEIGHT} (incl. frame fill): {:.2} ms/frame", milliseconds_per_frame(started));
    let source = send_frame();
    let crop = Crop { center_x: 0.5, center_y: 0.5, zoom: 1.6 };
    let started = Instant::now();
    for _ in 0..FRAMES {
        std::hint::black_box(crop_and_scale(&source, &crop));
    }
    println!("crop and scale {SEND_WIDTH}x{SEND_HEIGHT}: {:.2} ms/frame", milliseconds_per_frame(started));
}
