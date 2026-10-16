use std::sync::RwLock;

pub type CaptureHook = fn(channel_index: usize, samples: &mut [f32]);

static CAPTURE_HOOK: RwLock<Option<CaptureHook>> = RwLock::new(None);

pub fn set_capture_hook(hook: Option<CaptureHook>) {
    *CAPTURE_HOOK.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = hook;
}

fn capture_post_process(channel_index: usize, samples: &mut [f32]) {
    let hook = *CAPTURE_HOOK.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(hook) = hook {
        hook(channel_index, samples);
    }
}

#[cxx::bridge(namespace = "livekit_ffi")]
pub mod ffi {
    extern "Rust" {
        fn capture_post_process(channel_index: usize, samples: &mut [f32]);
    }
}
