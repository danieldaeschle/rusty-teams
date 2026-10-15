#[cfg(windows)]
mod embedded;
pub mod model;
pub mod runtime;

use std::collections::VecDeque;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use libwebrtc::video_frame::I420Buffer;

use crate::blur::MaskInference;
use crate::error::{Error, Result};
use crate::mask::Mask;
use model::{MEDIAPIPE, MODNET_LARGE, MODNET_SMALL, ModelSpec};
use runtime::{Device, Runner};

const BUDGET: Duration = Duration::from_millis(35);
const BEHIND: Duration = Duration::from_millis(45);
const PROBE_AFTER: u32 = 15;
const PROBE_RUNS: usize = 5;
const TIMING_WINDOW: usize = 20;
const MODNET_CPU_THREADS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rung {
    pub spec: ModelSpec,
    pub device: Device,
}

pub fn rungs(onnx_runtime: bool, gpu: bool, on_battery: bool) -> Vec<Rung> {
    let mut rungs = vec![Rung { spec: MEDIAPIPE, device: Device::Cpu { threads: 1 } }];
    if on_battery || !onnx_runtime {
        return rungs;
    }
    if gpu {
        rungs.push(Rung { spec: MODNET_SMALL, device: Device::Gpu });
        rungs.push(Rung { spec: MODNET_LARGE, device: Device::Gpu });
    } else {
        rungs.push(Rung { spec: MODNET_SMALL, device: Device::Cpu { threads: MODNET_CPU_THREADS } });
    }
    rungs
}

fn median(timings: impl Iterator<Item = Duration>) -> Duration {
    let mut sorted: Vec<Duration> = timings.collect();
    sorted.sort();
    sorted.get(sorted.len() / 2).copied().unwrap_or_default()
}

type Probe = JoinHandle<Option<(Box<dyn Runner>, Duration)>>;

fn probe(rung: Rung, input: Vec<f32>) -> Probe {
    std::thread::spawn(move || {
        let mut runner = runtime::runner(&rung.spec, rung.device).ok()?;
        runner.run(input.clone()).ok()?;
        let mut timings = Vec::with_capacity(PROBE_RUNS);
        for _ in 0..PROBE_RUNS {
            let started = Instant::now();
            runner.run(input.clone()).ok()?;
            timings.push(started.elapsed());
        }
        Some((runner, median(timings.into_iter())))
    })
}

pub struct FixedSegmenter {
    spec: ModelSpec,
    runner: Box<dyn Runner>,
}

impl FixedSegmenter {
    pub fn new(spec: ModelSpec, device: Device) -> Result<Self> {
        let runner = runtime::runner(&spec, device).map_err(|error| Error::Webrtc(format!("segmentation model: {error}")))?;
        Ok(FixedSegmenter { spec, runner })
    }
}

impl MaskInference for FixedSegmenter {
    fn infer(&mut self, mut frame: I420Buffer) -> Result<Mask> {
        let output = self.runner.run(self.spec.input(&mut frame)).map_err(|error| Error::Webrtc(format!("segmentation: {error}")))?;
        self.spec.mask(&output).ok_or_else(|| Error::Webrtc(format!("segmentation output has {} values", output.len())))
    }
}

pub struct LadderSegmenter {
    rungs: Vec<Rung>,
    runners: Vec<Option<Box<dyn Runner>>>,
    active: usize,
    next_probe: usize,
    probe: Option<(usize, Probe)>,
    served: u32,
    timings: VecDeque<Duration>,
}

impl LadderSegmenter {
    pub fn new() -> Result<Self> {
        Self::with_rungs(rungs(runtime::environment().is_some(), runtime::gpu_available(), on_battery()))
    }

    pub fn with_rungs(rungs: Vec<Rung>) -> Result<Self> {
        let first = rungs.first().ok_or_else(|| Error::Webrtc("segmentation: no model".into()))?;
        let runner = runtime::runner(&first.spec, first.device).map_err(|error| Error::Webrtc(format!("segmentation model: {error}")))?;
        let mut runners: Vec<Option<Box<dyn Runner>>> = rungs.iter().map(|_| None).collect();
        runners[0] = Some(runner);
        Ok(LadderSegmenter { next_probe: 1, rungs, runners, active: 0, probe: None, served: 0, timings: VecDeque::new() })
    }

    pub fn active(&self) -> ModelSpec {
        self.rungs[self.active].spec
    }

    fn adopt_probe(&mut self) {
        let Some((rung, handle)) = self.probe.take_if(|(_, handle)| handle.is_finished()) else { return };
        match handle.join().ok().flatten() {
            Some((runner, typical)) if typical <= BUDGET => {
                self.runners[rung] = Some(runner);
                self.active = rung;
                self.timings.clear();
                self.served = 0;
                self.next_probe = rung + 1;
            }
            _ => self.next_probe = self.rungs.len(),
        }
    }

    fn start_probe(&mut self, frame: &mut I420Buffer) {
        if self.probe.is_some() || self.served < PROBE_AFTER || self.next_probe >= self.rungs.len() {
            return;
        }
        let rung = self.rungs[self.next_probe];
        self.probe = Some((self.next_probe, probe(rung, rung.spec.input(frame))));
    }

    fn record(&mut self, elapsed: Duration) {
        self.timings.push_back(elapsed);
        if self.timings.len() > TIMING_WINDOW {
            self.timings.pop_front();
        }
        let behind = self.timings.len() == TIMING_WINDOW && median(self.timings.iter().copied()) > BEHIND;
        if behind && self.active > 0 {
            self.runners[self.active] = None;
            self.active -= 1;
            self.next_probe = self.rungs.len();
            self.timings.clear();
        }
    }
}

impl MaskInference for LadderSegmenter {
    fn infer(&mut self, mut frame: I420Buffer) -> Result<Mask> {
        self.adopt_probe();
        self.start_probe(&mut frame);
        let spec = self.rungs[self.active].spec;
        let input = spec.input(&mut frame);
        let runner = self.runners[self.active].as_mut().ok_or_else(|| Error::Webrtc("segmentation: runner missing".into()))?;
        let started = Instant::now();
        let output = runner.run(input).map_err(|error| Error::Webrtc(format!("segmentation: {error}")))?;
        let elapsed = started.elapsed();
        self.served += 1;
        if self.active > 0 {
            self.record(elapsed);
        }
        spec.mask(&output).ok_or_else(|| Error::Webrtc(format!("segmentation output has {} values", output.len())))
    }
}

#[cfg(windows)]
fn on_battery() -> bool {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.ACLineStatus == 0
}

#[cfg(not(windows))]
fn on_battery() -> bool {
    std::fs::read_dir("/sys/class/power_supply").into_iter().flatten().flatten().any(|supply| {
        let read = |name: &str| std::fs::read_to_string(supply.path().join(name)).unwrap_or_default();
        read("type").trim() == "Battery" && read("status").trim() == "Discharging"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::i420_from_rgba;

    struct Fixed(Duration, usize);

    impl Runner for Fixed {
        fn run(&mut self, _input: Vec<f32>) -> std::result::Result<Vec<f32>, String> {
            std::thread::sleep(self.0);
            Ok(vec![0.5; self.1])
        }
    }

    fn ladder(times: [Duration; 2]) -> LadderSegmenter {
        let rungs = vec![Rung { spec: MEDIAPIPE, device: Device::Cpu { threads: 1 } }, Rung { spec: MODNET_SMALL, device: Device::Cpu { threads: 1 } }];
        let mut ladder = LadderSegmenter::with_rungs(rungs.clone()).expect("mediapipe loads");
        ladder.runners[0] = Some(Box::new(Fixed(times[0], MEDIAPIPE.width * MEDIAPIPE.height)));
        ladder.runners[1] = Some(Box::new(Fixed(times[1], MODNET_SMALL.width * MODNET_SMALL.height)));
        ladder
    }

    fn frame() -> I420Buffer {
        i420_from_rgba(&vec![128u8; 64 * 36 * 4], 64, 36)
    }

    #[test]
    fn the_ladder_starts_small_and_steps_down_when_the_big_model_falls_behind() {
        let mut ladder = ladder([Duration::ZERO, BEHIND + Duration::from_millis(5)]);
        assert_eq!(ladder.infer(frame()).unwrap().width, MEDIAPIPE.width);
        ladder.active = 1;
        ladder.next_probe = 2;
        assert_eq!(ladder.infer(frame()).unwrap().width, MODNET_SMALL.width);
        for _ in 1..TIMING_WINDOW {
            ladder.infer(frame()).unwrap();
        }
        assert_eq!(ladder.active(), MEDIAPIPE);
        assert!(ladder.runners[1].is_none());
    }

    #[test]
    fn a_probe_within_budget_takes_over_and_a_slow_one_ends_the_climb() {
        for (typical, climbs) in [(BUDGET / 2, true), (BUDGET * 2, false)] {
            let mut ladder = ladder([Duration::ZERO, Duration::ZERO]);
            ladder.runners[1] = None;
            let handle = std::thread::spawn(move || Some((Box::new(Fixed(Duration::ZERO, MODNET_SMALL.width * MODNET_SMALL.height)) as Box<dyn Runner>, typical)));
            while !handle.is_finished() {
                std::thread::yield_now();
            }
            ladder.probe = Some((1, handle));
            let mask = ladder.infer(frame()).unwrap();
            assert_eq!(mask.width == MODNET_SMALL.width, climbs);
            assert_eq!(ladder.next_probe, 2);
        }
    }

    #[test]
    fn battery_missing_gpu_and_missing_onnx_runtime_shorten_the_ladder() {
        assert_eq!(rungs(true, true, true).len(), 1);
        assert_eq!(rungs(false, false, false).len(), 1, "no matting model on tract");
        assert_eq!(rungs(true, false, false).iter().map(|rung| rung.spec).collect::<Vec<_>>(), vec![MEDIAPIPE, MODNET_SMALL]);
        assert_eq!(rungs(true, true, false).last().unwrap(), &Rung { spec: MODNET_LARGE, device: Device::Gpu });
    }

    #[test]
    fn every_ladder_model_runs_and_answers_one_value_per_cell() {
        for spec in [MEDIAPIPE, MODNET_SMALL] {
            let mut runner = runtime::runner(&spec, Device::Cpu { threads: 2 }).expect("model loads");
            let output = runner.run(spec.input(&mut frame())).expect("inference runs");
            assert!(spec.mask(&output).is_some(), "{}", spec.name);
        }
        let mut tract = runtime::tract_only(&MEDIAPIPE);
        assert_eq!(tract.run(MEDIAPIPE.input(&mut frame())).unwrap().len(), MEDIAPIPE.width * MEDIAPIPE.height);
    }
}
