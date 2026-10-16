use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-cdylib-link-arg=/EXPORT:OrtGetApiBase");
    // The xwin SDK names these libraries all lower case; ort-sys asks for mixed case on a case-sensitive file system.
    let Some(sdk) = std::env::var("LIB").ok().and_then(|paths| paths.split(';').map(PathBuf::from).find(|path| path.join("pathcch.lib").exists())) else { return };
    let shims = PathBuf::from(std::env::var("OUT_DIR").expect("out dir")).join("case-shims");
    std::fs::create_dir_all(&shims).expect("shim dir");
    for (wanted, existing) in [("DirectML.lib", "directml.lib"), ("PathCch.lib", "pathcch.lib")] {
        let link = shims.join(wanted);
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink(sdk.join(existing), &link).expect("shim link");
    }
    println!("cargo::rustc-link-search=native={}", shims.display());
}
