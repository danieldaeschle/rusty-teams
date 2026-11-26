# webrtc-sys 0.3.48 patches

Upstream: crates.io `webrtc-sys 0.3.48` (livekit/rust-sdks). Wired in via `[patch.crates-io]` in the root `Cargo.toml`. `crates/calling` pins `webrtc-sys = "=0.3.48"` and `libwebrtc = "=0.3.51"` so an update cannot drop the patch silently. 0.3.49 still has both bugs.

## Changed files

| File | Change | Why |
|---|---|---|
| `src/adm_proxy.cpp`, `include/livekit/adm_proxy.h` | `Init()` re-inits terminated sub ADMs; `EnsurePlatformAdmCreated()` re-inits a terminated platform ADM; `InitPlatformAdm()` replays transport, devices, observer, stereo; `Terminate()` stops synthetic playout and clears `playing_` / `recording_` | `WebRtcVoiceEngine::Terminate()` terminates the ADM after every call, `Init()` was a no-op: from the second call on 0 devices, no mic, no sound |
| `src/adm_proxy.cpp` | `BuiltInAECIsAvailable()` returns false on Windows | the voice capture DMO fails `StartRecording` ("Playout must be started before recording") when no remote audio plays yet - mic silent for the whole call (seen in the app log 2026-10-13) |
| `src/adm_proxy.cpp`, `include/livekit/adm_proxy.h` | `SetPlayoutDevice` / `SetRecordingDevice(WindowsDeviceType)` turn `kDefaultCommunicationDevice` into `kDefaultDevice` | `WebRtcVoiceEngine::Init` selects the communications default at every call start; the app's "Default device" means the Windows default device (seen 2026-10-14: speaker came up on the communications default, user picked the default device by hand every call) |
| `src/noise_hook.rs`, `src/noise_hook.cpp`, `include/livekit/noise_hook.h`, `build.rs`, `src/lib.rs`, `src/peer_connection_factory.cpp` | The factory's APM gets a capture post processor that calls a Rust hook (`noise_hook::set_capture_hook`) on the full-band capture buffer | ML noise suppression (RNNoise) on the device track, keeping echo cancellation on the real playout reference |
| `src/vaapi/vaapi_h264_encoder_wrapper.cpp` | `bitstream_put_ui()` no longer shifts a 32-bit word by 32 when a value fills a fresh word | That shift is undefined; built with clang -O2 it wrote the NAL start code of the packed slice header as `00 00 00 00`, and the Intel iHD driver (26.3.5) read past the buffer and segfaulted in `vaRenderPicture` on the first camera frame (2026-11-24, Core Ultra 7 165U). gcc -O0 happened to produce `00 00 00 01`, which hid it |
| `src/mf/mf_h264_encoder_impl.cpp` | The NV12 input buffer is allocated for a frame rounded up to 16x16, the tail zeroed; frame sizes are made even | AMD's H.264 MFT (`amdh264enc64.dll`) copies the input in 16-row/16-column blocks and read past the exact-size buffer: access violation on the encoder thread in a call with a 868 px wide frame (2026-11-26) |
