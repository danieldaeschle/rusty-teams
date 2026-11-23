use std::sync::RwLock;

pub type CaptureHook = fn(channel_index: usize, samples: &mut [f32]);

static CAPTURE_HOOK: RwLock<Option<CaptureHook>> = RwLock::new(None);

pub fn set_capture_hook(hook: Option<CaptureHook>) {
    *CAPTURE_HOOK.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = hook;
}

fn capture_post_process(channel_index: usize, samples: &mut [f32]) {
    let hook = *CAPTURE_HOOK.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(hook) = hook else {
        return;
    };
    // C++ calls this on the capture thread: a panic crossing the bridge aborts the whole app, so a
    // failing hook is dropped and the samples go out unprocessed instead.
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        hook(channel_index, samples);
    }));
    if ran.is_err() {
        set_capture_hook(None);
    }
}

#[cxx::bridge(namespace = "livekit_ffi")]
pub mod ffi {
    extern "Rust" {
        fn capture_post_process(channel_index: usize, samples: &mut [f32]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failing_hook(_channel_index: usize, _samples: &mut [f32]) {
        panic!("hook failed");
    }

    #[test]
    fn a_panicking_hook_is_dropped_instead_of_unwinding_into_cpp() {
        set_capture_hook(Some(failing_hook));
        let mut samples = [0.5f32; 4];
        capture_post_process(0, &mut samples);
        assert!(CAPTURE_HOOK.read().unwrap().is_none());
        assert_eq!(samples, [0.5; 4]);
    }
}
