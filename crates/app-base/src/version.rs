use std::sync::OnceLock;

static BUILD_VERSION: OnceLock<&'static str> = OnceLock::new();

pub fn set_build_version(version: &'static str) {
    let _ = BUILD_VERSION.set(version);
}

pub fn build_version() -> &'static str {
    BUILD_VERSION.get().copied().unwrap_or("unknown")
}

static RELEASE_TAG: OnceLock<&'static str> = OnceLock::new();

pub fn set_release_tag(tag: &'static str) {
    let _ = RELEASE_TAG.set(tag);
}

/// Set only in builds of a `v*` tag, so local and deployed dev builds never pull a GitHub release.
pub fn release_tag() -> Option<&'static str> {
    RELEASE_TAG.get().copied()
}
