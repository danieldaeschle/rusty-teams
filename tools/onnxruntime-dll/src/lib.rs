//! ONNX Runtime with DirectML as a DLL built against the dynamic C runtime. teams.exe links the static C runtime
//! (libwebrtc is /MT), so it cannot link pyke's /MD static library; it embeds this DLL and loads it at run time.
#[used]
static KEEP_ORT_ENTRY: unsafe extern "system" fn() -> *const ort_sys::OrtApiBase = ort_sys::OrtGetApiBase;
