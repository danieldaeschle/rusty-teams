use std::collections::HashMap;
use std::io::Cursor;
use std::sync::OnceLock;

use ort::environment::Environment;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor as OrtTensor;
use tract_onnx::prelude::*;

use super::model::ModelSpec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Cpu { threads: usize },
    Gpu,
}

pub trait Runner: Send {
    fn run(&mut self, input: Vec<f32>) -> std::result::Result<Vec<f32>, String>;
}

static ENVIRONMENT: OnceLock<Option<Environment>> = OnceLock::new();

/// The process-wide ONNX Runtime, or `None` when it cannot load; then only tract runs.
pub fn environment() -> Option<&'static Environment> {
    ENVIRONMENT.get_or_init(|| builder().and_then(|builder| builder.with_name("segmentation").build().ok())).as_ref()
}

#[cfg(not(windows))]
fn builder() -> Option<ort::environment::EnvironmentBuilder> {
    Some(ort::init())
}

#[cfg(windows)]
fn builder() -> Option<ort::environment::EnvironmentBuilder> {
    let path = super::embedded::onnxruntime_path()?;
    ort::init_from(path).ok()
}

pub fn gpu_available() -> bool {
    cfg!(windows) && environment().is_some()
}

struct OrtRunner {
    session: Session,
    shape: [usize; 4],
}

impl Runner for OrtRunner {
    fn run(&mut self, input: Vec<f32>) -> std::result::Result<Vec<f32>, String> {
        let tensor = OrtTensor::from_array((self.shape.to_vec(), input.into_boxed_slice())).map_err(|error| error.to_string())?;
        let outputs = self.session.run(ort::inputs![tensor]).map_err(|error| error.to_string())?;
        let (_, values) = outputs[0].try_extract_tensor::<f32>().map_err(|error| error.to_string())?;
        Ok(values.to_vec())
    }
}

fn ort_runner(spec: &ModelSpec, device: Device) -> std::result::Result<OrtRunner, String> {
    let environment = environment().ok_or("onnx runtime unavailable")?;
    let failed = |error: ort::Error<_>| error.to_string();
    let mut builder = Session::builder(environment).map_err(|error| error.to_string())?.with_optimization_level(GraphOptimizationLevel::Level3).map_err(failed)?;
    builder = match device {
        Device::Cpu { threads } => builder.with_intra_threads(threads).map_err(failed)?.with_execution_providers([ort::ep::CPU::default().build().error_on_failure()]).map_err(failed)?,
        #[cfg(windows)]
        Device::Gpu => builder
            .with_parallel_execution(false)
            .map_err(failed)?
            .with_memory_pattern(false)
            .map_err(failed)?
            .with_execution_providers([ort::ep::DirectML::default().build().error_on_failure()])
            .map_err(failed)?,
        #[cfg(not(windows))]
        Device::Gpu => return Err("no gpu runtime on this platform".to_owned()),
    };
    let session = builder.commit_from_memory(spec.bytes()).map_err(|error| error.to_string())?;
    Ok(OrtRunner { session, shape: [1, 3, spec.height, spec.width] })
}

struct TractRunner {
    plan: std::sync::Arc<TypedRunnableModel>,
    shape: [usize; 4],
}

impl Runner for TractRunner {
    fn run(&mut self, input: Vec<f32>) -> std::result::Result<Vec<f32>, String> {
        let tensor = Tensor::from_shape(&self.shape, &input).map_err(|error| error.to_string())?;
        let outputs = self.plan.run(tvec!(tensor.into())).map_err(|error| error.to_string())?;
        let values = outputs[0].to_plain_array_view::<f32>().map_err(|error| error.to_string())?;
        Ok(values.iter().copied().collect())
    }
}

fn tract_runner(spec: &ModelSpec) -> TractResult<TractRunner> {
    let model = tract_onnx::onnx().model_for_read(&mut Cursor::new(spec.bytes()))?;
    let symbols: HashMap<_, _> = ["batch_size", "height", "width"]
        .into_iter()
        .zip([1, spec.height, spec.width])
        .map(|(name, value)| (model.symbols.sym(name), TDim::from(value as i64)))
        .collect();
    let plan = model.into_typed()?.set_symbols(&symbols)?.into_optimized()?.into_runnable()?;
    Ok(TractRunner { plan, shape: [1, 3, spec.height, spec.width] })
}

pub fn runner(spec: &ModelSpec, device: Device) -> std::result::Result<Box<dyn Runner>, String> {
    match ort_runner(spec, device) {
        Ok(runner) => Ok(Box::new(runner)),
        Err(_) if environment().is_none() && device != Device::Gpu => tract_runner(spec).map(|runner| Box::new(runner) as Box<dyn Runner>).map_err(|error| error.to_string()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
pub fn tract_only(spec: &ModelSpec) -> Box<dyn Runner> {
    Box::new(tract_runner(spec).expect("tract loads the model"))
}
