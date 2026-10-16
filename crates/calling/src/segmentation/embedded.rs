use std::io::Read as _;
use std::path::PathBuf;

#[cfg(embedded_onnxruntime)]
const COMPRESSED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/onnxruntime.dll.deflate"));
#[cfg(not(embedded_onnxruntime))]
const COMPRESSED: &[u8] = &[];

pub fn onnxruntime_path() -> Option<PathBuf> {
    if COMPRESSED.is_empty() {
        return None;
    }
    let name = format!("onnxruntime-{:08x}.dll", fingerprint(COMPRESSED));
    let path = std::env::current_exe().ok()?.with_file_name(&name);
    if path.exists() {
        return Some(path);
    }
    remove_older(&path, &name);
    let mut library = Vec::new();
    flate2::read::DeflateDecoder::new(COMPRESSED).read_to_end(&mut library).ok()?;
    let partial = path.with_extension("partial");
    std::fs::write(&partial, &library).ok()?;
    std::fs::rename(&partial, &path).ok()?;
    Some(path)
}

fn remove_older(path: &std::path::Path, current: &str) {
    let Some(directory) = path.parent() else { return };
    for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("onnxruntime-") && name.ends_with(".dll") && name != current {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn fingerprint(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |hash, &byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193))
}
