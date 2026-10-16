use std::sync::OnceLock;

static BUILD_VERSION: OnceLock<&'static str> = OnceLock::new();

pub fn set_build_version(version: &'static str) {
    let _ = BUILD_VERSION.set(version);
}

pub fn build_version() -> &'static str {
    BUILD_VERSION.get().copied().unwrap_or("unknown")
}
