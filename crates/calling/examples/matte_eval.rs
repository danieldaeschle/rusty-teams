//! Scores the person matte against a portrait matting set with true alphas (PPM-100 layout: `<dir>/fg/*.jpg`, `<dir>/alpha/*.png`).
//! Usage: matte_eval <out_dir> <set_dir>... [--model mediapipe-256x144|modnet-320x192|modnet-512x288] [--gpu] [--shots N] [--limit N] [--motion]
//! `--motion` moves the person over a room backdrop and scores a mask that arrives K frames late, stale and propagated.
use std::path::{Path, PathBuf};
use std::time::Instant;

use calling::background::BackgroundPicture;
use calling::composite::{Backdrop, PicturePlanes, composite};
use calling::blur::{MaskInference, compose_image};
use calling::segmentation::FixedSegmenter;
use calling::segmentation::model::{MEDIAPIPE, MODNET_LARGE, MODNET_SMALL, ModelSpec};
use calling::segmentation::runtime::Device;
use calling::camera::i420_from_rgba;
use calling::mask::Mask;
use calling::matte::{Matte, Planes, RefineSettings, refine_with};
use calling::motion::{LumaGrid, propagate_for_display};
use image::imageops::FilterType;
use libwebrtc::native::yuv_helper::i420_to_abgr;
use libwebrtc::video_frame::I420Buffer;

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const EDGE_RADIUS: usize = 12;
const JITTER_X: usize = 3;
const JITTER_Y: usize = 2;
const SPEEDS: [usize; 2] = [8, 16];
const DELAYS: [usize; 4] = [0, 1, 2, 3];
const METHODS: [&str; 5] = ["stale", "stale+guided", "propagated", "propagated+guided", "shipped"];

struct Sample {
    name: String,
    rgba: Vec<u8>,
    truth: Vec<f32>,
}

#[derive(Default, Clone, Copy)]
struct Score {
    mse: f64,
    edge_mse: f64,
    sad: f64,
    flicker: f64,
    infer_ms: f64,
    refine_ms: f64,
}

fn load(foreground: &Path, alpha: &Path) -> Option<Sample> {
    let photo = image::open(foreground).ok()?.resize_to_fill(WIDTH, HEIGHT, FilterType::Triangle).to_rgba8().into_raw();
    let truth = image::open(alpha).ok()?.resize_to_fill(WIDTH, HEIGHT, FilterType::Triangle).to_luma8().into_raw();
    let name = foreground.file_stem()?.to_string_lossy().into_owned();
    Some(Sample { name, rgba: photo, truth: truth.iter().map(|&value| f32::from(value) / 255.).collect() })
}

fn samples(sets: &[PathBuf]) -> Vec<Sample> {
    let mut found = Vec::new();
    for set in sets {
        let Ok(entries) = std::fs::read_dir(set.join("fg")) else { continue };
        let mut paths: Vec<PathBuf> = entries.filter_map(|entry| entry.ok().map(|entry| entry.path())).collect();
        paths.sort();
        for path in paths {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let alpha = ["png", "jpg"].iter().map(|extension| set.join("alpha").join(format!("{stem}.{extension}"))).find(|candidate| candidate.exists());
            if let Some(sample) = alpha.and_then(|alpha| load(&path, &alpha)) {
                found.push(sample);
            }
        }
    }
    found
}

fn box_mean(values: &[f32], width: usize, height: usize, radius: usize) -> Vec<f32> {
    let mut across = vec![0f32; values.len()];
    for row in 0..height {
        let line = &values[row * width..][..width];
        let mut prefix = vec![0f32; width + 1];
        for (index, &value) in line.iter().enumerate() {
            prefix[index + 1] = prefix[index] + value;
        }
        for column in 0..width {
            let (low, high) = (column.saturating_sub(radius), (column + radius + 1).min(width));
            across[row * width + column] = (prefix[high] - prefix[low]) / (high - low) as f32;
        }
    }
    let mut output = vec![0f32; values.len()];
    for column in 0..width {
        let mut prefix = vec![0f32; height + 1];
        for row in 0..height {
            prefix[row + 1] = prefix[row] + across[row * width + column];
        }
        for row in 0..height {
            let (low, high) = (row.saturating_sub(radius), (row + radius + 1).min(height));
            output[row * width + column] = (prefix[high] - prefix[low]) / (high - low) as f32;
        }
    }
    output
}

fn edge_band(truth: &[f32]) -> Vec<bool> {
    box_mean(truth, WIDTH as usize, HEIGHT as usize, EDGE_RADIUS).iter().map(|&mean| mean > 0.01 && mean < 0.99).collect()
}

fn shifted(rgba: &[u8]) -> Vec<u8> {
    let (width, height) = (WIDTH as usize, HEIGHT as usize);
    let mut output = vec![0u8; rgba.len()];
    for row in 0..height {
        for column in 0..width {
            let (source_row, source_column) = (row.saturating_sub(JITTER_Y), column.saturating_sub(JITTER_X));
            let noise = ((row * 7919 + column * 104_729) % 9) as i16 - 4;
            for channel in 0..4 {
                let value = i16::from(rgba[(source_row * width + source_column) * 4 + channel]);
                output[(row * width + column) * 4 + channel] = if channel == 3 { 255 } else { (value + noise).clamp(0, 255) as u8 };
            }
        }
    }
    output
}

fn settings() -> RefineSettings {
    let read = |name: &str| std::env::var(name).ok().and_then(|value| value.parse::<f32>().ok());
    let defaults = RefineSettings::default();
    RefineSettings {
        guide_width: read("GUIDE_WIDTH").map_or(defaults.guide_width, |value| value as usize),
        edge_low: read("EDGE_LOW").unwrap_or(defaults.edge_low),
        edge_high: read("EDGE_HIGH").unwrap_or(defaults.edge_high),
        sure_person: read("SURE_PERSON").unwrap_or(defaults.sure_person),
        sure_background: read("SURE_BACKGROUND").unwrap_or(defaults.sure_background),
    }
}

fn matte_of(segmenter: &mut FixedSegmenter, rgba: &[u8]) -> (Matte, f64, f64) {
    let camera = i420_from_rgba(rgba, WIDTH, HEIGHT);
    let started = Instant::now();
    let mask = segmenter.infer(i420_from_rgba(rgba, WIDTH, HEIGHT)).expect("mask");
    let infer_ms = started.elapsed().as_secs_f64() * 1000.;
    let started = Instant::now();
    let matte = refine_with(&mask, Planes::of(&camera), settings());
    (matte, infer_ms, started.elapsed().as_secs_f64() * 1000.)
}

fn score(segmenter: &mut FixedSegmenter, sample: &Sample) -> (Score, Matte) {
    let (matte, infer_ms, refine_ms) = matte_of(segmenter, &sample.rgba);
    let band = edge_band(&sample.truth);
    let (mut squared, mut edge_squared, mut edge_count, mut absolute) = (0f64, 0f64, 0usize, 0f64);
    for ((&alpha, &truth), &edge) in matte.alpha.iter().zip(&sample.truth).zip(&band) {
        let difference = f64::from(f32::from(alpha) / 255. - truth);
        squared += difference * difference;
        absolute += difference.abs();
        if edge {
            edge_squared += difference * difference;
            edge_count += 1;
        }
    }
    let (jittered, ..) = matte_of(segmenter, &shifted(&sample.rgba));
    let (width, height) = (WIDTH as usize, HEIGHT as usize);
    let mut flicker = 0f64;
    let mut flicker_count = 0usize;
    for row in JITTER_Y..height {
        for column in JITTER_X..width {
            let source = (row - JITTER_Y) * width + column - JITTER_X;
            if band[source] {
                flicker += f64::from(i16::from(jittered.alpha[row * width + column]).abs_diff(i16::from(matte.alpha[source])));
                flicker_count += 1;
            }
        }
    }
    let pixels = (width * height) as f64;
    let result = Score {
        mse: squared / pixels * 1000.,
        edge_mse: edge_squared / edge_count.max(1) as f64 * 1000.,
        sad: absolute / 1000.,
        flicker: flicker / flicker_count.max(1) as f64,
        infer_ms,
        refine_ms,
    };
    (result, matte)
}

fn rgba_of(buffer: &I420Buffer) -> Vec<u8> {
    let (stride_y, stride_u, stride_v) = buffer.strides();
    let (plane_y, plane_u, plane_v) = buffer.data();
    let mut rgba = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    i420_to_abgr(plane_y, stride_y, plane_u, stride_u, plane_v, stride_v, &mut rgba, WIDTH * 4, WIDTH as i32, HEIGHT as i32);
    rgba
}

fn backdrop() -> PicturePlanes {
    let (width, height) = (64, 36);
    let rgb = (0..width * height).flat_map(|index| {
        let (x, y) = (index % width, index / width);
        [(20 + x) as u8, (170 + y * 2) as u8, (60 + x * 2) as u8]
    });
    let covered = BackgroundPicture::from_rgb(rgb.collect(), width, height).expect("backdrop").covering(WIDTH as usize, HEIGHT as usize);
    PicturePlanes::new(&covered, WIDTH as usize, HEIGHT as usize)
}

fn gray_rgba(values: impl Iterator<Item = u8>) -> Vec<u8> {
    values.flat_map(|value| [value, value, value, 255]).collect()
}

fn save_sheet(out: &Path, sample: &Sample, matte: &Matte) {
    let mut blurred = i420_from_rgba(&sample.rgba, WIDTH, HEIGHT);
    composite(&mut blurred, matte, Backdrop::Blur);
    let mut replaced = sample.rgba.clone();
    compose_image(&mut replaced, WIDTH as usize, HEIGHT as usize, matte, &backdrop());
    let tiles = [sample.rgba.clone(), gray_rgba(sample.truth.iter().map(|&value| (value * 255. + 0.5) as u8)), gray_rgba(matte.alpha.iter().copied()), rgba_of(&blurred), replaced];
    let mut sheet = image::RgbaImage::new(WIDTH * 3, HEIGHT * 2);
    for (index, tile) in tiles.into_iter().enumerate() {
        let tile = image::RgbaImage::from_raw(WIDTH, HEIGHT, tile).expect("tile size");
        image::imageops::replace(&mut sheet, &tile, i64::from(WIDTH) * (index % 3) as i64, i64::from(HEIGHT) * (index / 3) as i64);
    }
    image::imageops::resize(&sheet, WIDTH * 3 / 2, HEIGHT, FilterType::Triangle).save(out.join(format!("{}.png", sample.name))).expect("sheet written");
}

fn room(seed: usize) -> Vec<u8> {
    let (width, height) = (WIDTH as usize, HEIGHT as usize);
    let mut state = (seed as u64).wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    let mut next = move |range: usize| {
        state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as usize) % range.max(1)
    };
    let mut rgba: Vec<u8> = (0..width * height)
        .flat_map(|index| {
            let row = index / width;
            let shade = (150 + row * 60 / height) as u8;
            [shade, shade - 10, shade - 25, 255]
        })
        .collect();
    for _ in 0..40 {
        let (left, top, box_width, box_height) = (next(width), next(height), 20 + next(220), 10 + next(160));
        let colour = [30 + next(200) as u8, 30 + next(200) as u8, 30 + next(200) as u8];
        for row in top..(top + box_height).min(height) {
            for column in left..(left + box_width).min(width) {
                rgba[(row * width + column) * 4..][..3].copy_from_slice(&colour);
            }
        }
    }
    for (index, pixel) in rgba.chunks_exact_mut(4).enumerate() {
        let noise = ((index * 7919 + index / width * 104_729) % 17) as i16 - 8;
        for channel in pixel.iter_mut().take(3) {
            *channel = (i16::from(*channel) + noise).clamp(0, 255) as u8;
        }
    }
    rgba
}

fn moved(sample: &Sample, backdrop: &[u8], (across, down): (usize, usize)) -> (Vec<u8>, Vec<f32>) {
    let (width, height) = (WIDTH as usize, HEIGHT as usize);
    let mut rgba = backdrop.to_vec();
    let mut truth = vec![0f32; width * height];
    for row in down..height {
        for column in across..width {
            let source = (row - down) * width + column - across;
            let alpha = sample.truth[source];
            truth[row * width + column] = alpha;
            for channel in 0..3 {
                let target = &mut rgba[(row * width + column) * 4 + channel];
                *target = (f32::from(sample.rgba[source * 4 + channel]) * alpha + f32::from(*target) * (1. - alpha) + 0.5) as u8;
            }
        }
    }
    (rgba, truth)
}

fn band_error(matte: &Matte, truth: &[f32], band: &[bool]) -> f64 {
    let (mut squared, mut count) = (0f64, 0usize);
    for ((&alpha, &truth), _) in matte.alpha.iter().zip(truth).zip(band).filter(|(_, edge)| **edge) {
        let difference = f64::from(f32::from(alpha) / 255. - truth);
        squared += difference * difference;
        count += 1;
    }
    squared / count.max(1) as f64 * 1000.
}

fn guided(mask: &Mask) -> Mask {
    Mask { matte: false, ..mask.clone() }
}

fn motion_eval(segmenter: &mut FixedSegmenter, spec: ModelSpec, all: &[Sample]) {
    let rows = SPEEDS.len() * DELAYS.len();
    let mut errors = vec![[0f64; METHODS.len()]; rows];
    let mut propagate_ms = 0f64;
    let mut propagations = 0usize;
    for (index, sample) in all.iter().enumerate() {
        let backdrop = room(index);
        let (source_rgba, source_truth) = moved(sample, &backdrop, (0, 0));
        let source = i420_from_rgba(&source_rgba, WIDTH, HEIGHT);
        let mask = segmenter.infer(i420_from_rgba(&source_rgba, WIDTH, HEIGHT)).expect("mask");
        let reference = LumaGrid::of(&source);
        let source_band = edge_band(&source_truth);
        for (speed_index, speed) in SPEEDS.into_iter().enumerate() {
            for (delay_index, delay) in DELAYS.into_iter().enumerate() {
                let (rgba, truth) = moved(sample, &backdrop, (speed * delay, speed * delay / 4));
                let current = i420_from_rgba(&rgba, WIDTH, HEIGHT);
                let band: Vec<bool> = edge_band(&truth).iter().zip(&source_band).map(|(now, before)| *now || *before).collect();
                let started = Instant::now();
                let (propagated, moved) = propagate_for_display(&mask, &reference, &LumaGrid::of(&current));
                propagate_ms += started.elapsed().as_secs_f64() * 1000.;
                propagations += 1;
                let shipped = Mask { matte: propagated.matte && !moved, ..propagated.clone() };
                let candidates = [mask.clone(), guided(&mask), propagated.clone(), guided(&propagated), shipped];
                for (method, candidate) in candidates.iter().enumerate() {
                    let matte = refine_with(candidate, Planes::of(&current), settings());
                    errors[speed_index * DELAYS.len() + delay_index][method] += band_error(&matte, &truth, &band);
                }
            }
        }
    }
    let count = all.len().max(1) as f64;
    println!("{} {} images, motion edge_mse x1000 (band = old and new edge), propagate {:.2} ms", spec.name, all.len(), propagate_ms / propagations.max(1) as f64);
    println!("| speed px/frame | late frames | {} |", METHODS.join(" | "));
    for (speed_index, speed) in SPEEDS.into_iter().enumerate() {
        for (delay_index, delay) in DELAYS.into_iter().enumerate() {
            let row = &errors[speed_index * DELAYS.len() + delay_index];
            println!("| {speed} | {delay} | {} |", row.iter().map(|error| format!("{:.1}", error / count)).collect::<Vec<_>>().join(" | "));
        }
    }
}

fn main() {
    let mut arguments: Vec<String> = std::env::args().skip(1).collect();
    let flag = |arguments: &mut Vec<String>, name: &str| -> Option<usize> {
        let position = arguments.iter().position(|argument| argument == name)?;
        let value = arguments.remove(position + 1).parse().ok();
        arguments.remove(position);
        value
    };
    let model = arguments.iter().position(|argument| argument == "--model").map(|position| {
        let name = arguments.remove(position + 1);
        arguments.remove(position);
        name
    });
    let spec: ModelSpec = [MEDIAPIPE, MODNET_SMALL, MODNET_LARGE].into_iter().find(|spec| Some(spec.name) == model.as_deref()).unwrap_or(MEDIAPIPE);
    let gpu = arguments.iter().position(|argument| argument == "--gpu").map(|position| arguments.remove(position)).is_some();
    let motion = arguments.iter().position(|argument| argument == "--motion").map(|position| arguments.remove(position)).is_some();
    let shots = flag(&mut arguments, "--shots").unwrap_or(6);
    let limit = flag(&mut arguments, "--limit").unwrap_or(usize::MAX);
    let out = PathBuf::from(arguments.first().expect("out dir"));
    let sets: Vec<PathBuf> = arguments[1..].iter().map(PathBuf::from).collect();
    std::fs::create_dir_all(&out).expect("out dir");
    let mut segmenter = FixedSegmenter::new(spec, if gpu { Device::Gpu } else { Device::Cpu { threads: 4 } }).expect("model");
    let all = samples(&sets);
    if motion {
        motion_eval(&mut segmenter, spec, &all[..all.len().min(limit)]);
        return;
    }
    let mut total = Score::default();
    let mut lines = vec!["name,mse,edge_mse,sad,flicker,infer_ms,refine_ms".to_owned()];
    let count = all.len().min(limit);
    for (index, sample) in all.iter().take(limit).enumerate() {
        let (result, matte) = score(&mut segmenter, sample);
        lines.push(format!("{},{:.3},{:.3},{:.2},{:.3},{:.2},{:.2}", sample.name, result.mse, result.edge_mse, result.sad, result.flicker, result.infer_ms, result.refine_ms));
        total.mse += result.mse;
        total.edge_mse += result.edge_mse;
        total.sad += result.sad;
        total.flicker += result.flicker;
        total.infer_ms += result.infer_ms;
        total.refine_ms += result.refine_ms;
        if index < shots {
            save_sheet(&out, sample, &matte);
        }
    }
    std::fs::write(out.join("scores.csv"), lines.join("\n")).expect("csv written");
    let mean = |value: f64| value / count.max(1) as f64;
    println!(
        "{} {count} images: mse {:.2} edge_mse {:.2} sad {:.1} flicker {:.2} infer {:.1} ms refine {:.1} ms",
        spec.name,
        mean(total.mse),
        mean(total.edge_mse),
        mean(total.sad),
        mean(total.flicker),
        mean(total.infer_ms),
        mean(total.refine_ms)
    );
}
