//! Rebuild when locale catalogs change.
//!
//! `rust_i18n::i18n!` embeds YAML at compile time. Without these hints Cargo
//! can keep a stale catalog after editing `locales/*.yml`, so the UI shows
//! raw keys like `auth.banner.connecting`.

fn main() {
    println!("cargo:rerun-if-changed=locales");
    println!("cargo:rerun-if-changed=locales/app.yml");
}
