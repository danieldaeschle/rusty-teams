//! Windows tool for the call end-to-end test.
//! `audio_probe play <render device name part> <mono 16-bit 48 kHz wav> <seconds>` loops the file into that endpoint.
//! `audio_probe record-process <pid> <seconds> <out.wav>` records only what that process plays and prints loudness per 250 ms as JSON.
//! `audio_probe record-mic <seconds> <out.wav>` records the default communications mic the way a call does.

#[cfg(windows)]
fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let outcome = match arguments.as_slice() {
        ["play", device, wav, seconds] => probe::play(device, wav, seconds.parse().expect("seconds")),
        ["record-process", process_id, seconds, out] => probe::record_process(
            process_id.parse().expect("pid"),
            seconds.parse().expect("seconds"),
            out,
        ),
        ["record-mic", seconds, out] => probe::record_mic(seconds.parse().expect("seconds"), out),
        _ => Err("usage: play DEVICE WAV SECONDS | record-process PID SECONDS OUT.wav | record-mic SECONDS OUT.wav".to_owned()),
    };
    if let Err(error) = outcome {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("audio_probe runs on Windows only");
}

#[cfg(windows)]
mod probe {
    use std::time::{Duration, Instant};

    use calling::audio::{FRAME_SAMPLES, SAMPLE_RATE};
    use calling::pcm::SampleQueue;
    use calling::wav_dump::WavDump;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
        AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE_ACTIVE, IAudioClient,
        IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
        eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, STGM_READ,
    };

    const CHANNELS: u16 = 2;
    const BITS: u16 = 16;
    const PCM_TAG: u16 = 1;
    const BUFFER_HNS: i64 = 1_000_000;
    const TICK: Duration = Duration::from_millis(10);
    const WINDOW_MS: usize = 250;
    const WAV_HEADER: usize = 44;

    fn failed(what: &str, error: impl std::fmt::Display) -> String {
        format!("{what}: {error}")
    }

    fn render_device(name_part: &str) -> Result<IMMDevice, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|error| failed("enumerator", error))?;
            let devices = enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                .map_err(|error| failed("endpoints", error))?;
            let mut names = Vec::new();
            for index in 0..devices.GetCount().map_err(|error| failed("count", error))? {
                let device = devices.Item(index).map_err(|error| failed("item", error))?;
                let store = device
                    .OpenPropertyStore(STGM_READ)
                    .map_err(|error| failed("properties", error))?;
                let value = store
                    .GetValue(&PKEY_Device_FriendlyName)
                    .map_err(|error| failed("name", error))?;
                let name = value
                    .Anonymous
                    .Anonymous
                    .Anonymous
                    .pwszVal
                    .to_string()
                    .unwrap_or_default();
                if name.contains(name_part) {
                    return Ok(device);
                }
                names.push(name);
            }
            Err(format!(
                "no render device with \"{name_part}\" in {names:?}"
            ))
        }
    }

    fn read_mono_wav(path: &str) -> Result<Vec<i16>, String> {
        let bytes = std::fs::read(path).map_err(|error| failed("read wav", error))?;
        let header = bytes.get(..WAV_HEADER).ok_or("wav too short")?;
        let channels = u16::from_le_bytes([header[22], header[23]]);
        let rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
        let bits = u16::from_le_bytes([header[34], header[35]]);
        if channels != 1 || rate != SAMPLE_RATE || bits != BITS {
            return Err(format!(
                "need mono 16-bit {SAMPLE_RATE} Hz, got {channels} ch {bits} bit {rate} Hz"
            ));
        }
        let data = bytes
            .windows(4)
            .position(|tag| tag == b"data")
            .ok_or("wav without data chunk")?
            + 8;
        Ok(bytes[data..]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect())
    }

    pub fn play(name_part: &str, wav: &str, seconds: u64) -> Result<(), String> {
        let samples = read_mono_wav(wav)?;
        if samples.is_empty() {
            return Err("wav without samples".to_owned());
        }
        let device = render_device(name_part)?;
        let block_align = CHANNELS * BITS / 8;
        let format = WAVEFORMATEX {
            wFormatTag: PCM_TAG,
            nChannels: CHANNELS,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * u32::from(block_align),
            nBlockAlign: block_align,
            wBitsPerSample: BITS,
            cbSize: 0,
        };
        unsafe {
            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|error| failed("activate", error))?;
            client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    BUFFER_HNS,
                    0,
                    &raw const format,
                    None,
                )
                .map_err(|error| failed("initialize", error))?;
            let buffer_frames = client
                .GetBufferSize()
                .map_err(|error| failed("buffer size", error))?;
            let render: IAudioRenderClient = client
                .GetService()
                .map_err(|error| failed("render service", error))?;
            client.Start().map_err(|error| failed("start", error))?;
            let started = Instant::now();
            let mut position = 0usize;
            println!("{{\"playing\":true}}");
            while started.elapsed() < Duration::from_secs(seconds) {
                let padding = client
                    .GetCurrentPadding()
                    .map_err(|error| failed("padding", error))?;
                let frames = buffer_frames - padding;
                if frames > 0 {
                    let data = render
                        .GetBuffer(frames)
                        .map_err(|error| failed("buffer", error))?;
                    let output = std::slice::from_raw_parts_mut(
                        data.cast::<i16>(),
                        frames as usize * usize::from(CHANNELS),
                    );
                    for pair in output.chunks_exact_mut(usize::from(CHANNELS)) {
                        let sample = samples[position % samples.len()];
                        pair.fill(sample);
                        position += 1;
                    }
                    render
                        .ReleaseBuffer(frames, 0)
                        .map_err(|error| failed("release", error))?;
                }
                std::thread::sleep(TICK);
            }
            let _ = client.Stop();
        }
        Ok(())
    }

    fn rms(samples: &[i16]) -> f64 {
        let sum: f64 = samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum();
        (sum / samples.len().max(1) as f64).sqrt()
    }

    pub fn record_process(process_id: u32, seconds: u64, out: &str) -> Result<(), String> {
        let queue = SampleQueue::default();
        let _capture = calling::start_process_capture(queue.clone(), process_id)
            .map_err(|error| failed("capture", error))?;
        record(&queue, seconds, out)
    }

    pub fn record_mic(seconds: u64, out: &str) -> Result<(), String> {
        let queue = SampleQueue::default();
        let started = calling::sound_capture::start_microphone(
            &calling::DeviceChoice::SystemDefault,
            queue.clone(),
        )
        .map_err(|error| failed("microphone", error))?;
        eprintln!("{}", started.source);
        record(&queue, seconds, out)
    }

    fn record(queue: &SampleQueue, seconds: u64, out: &str) -> Result<(), String> {
        let mut dump =
            WavDump::create(std::path::Path::new(out)).map_err(|error| failed("wav", error))?;
        let frames_per_window = WINDOW_MS * SAMPLE_RATE as usize / 1000 / FRAME_SAMPLES;
        let mut window = Vec::new();
        let mut loudness = Vec::new();
        let started = Instant::now();
        let mut next = Instant::now();
        while started.elapsed() < Duration::from_secs(seconds) {
            let frame = queue.next_frame().unwrap_or_else(|| vec![0; FRAME_SAMPLES]);
            dump.write(&frame);
            window.extend_from_slice(&frame);
            if window.len() >= frames_per_window * FRAME_SAMPLES {
                loudness.push(rms(&window).round());
                window.clear();
            }
            next += TICK;
            std::thread::sleep(next.saturating_duration_since(Instant::now()));
        }
        println!("{{\"window_ms\":{WINDOW_MS},\"rms\":{loudness:?}}}");
        Ok(())
    }
}
