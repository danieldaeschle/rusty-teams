use std::io::Write as _;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(embedded_onnxruntime)");
    println!("cargo::rerun-if-env-changed=CALLING_ONNXRUNTIME_DLL");
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let Some(library) = std::env::var_os("CALLING_ONNXRUNTIME_DLL").filter(|_| windows) else { return };
    println!("cargo::rerun-if-changed={}", library.to_string_lossy());
    let bytes = std::fs::read(&library).expect("CALLING_ONNXRUNTIME_DLL is readable");
    let output = std::path::Path::new(&std::env::var("OUT_DIR").expect("out dir")).join("onnxruntime.dll.deflate");
    let mut encoder = flate2::write::DeflateEncoder::new(std::fs::File::create(&output).expect("deflate output"), flate2::Compression::best());
    encoder.write_all(&bytes).expect("deflate written");
    encoder.finish().expect("deflate finished");
    println!("cargo::rustc-cfg=embedded_onnxruntime");
}
