use std::mem::size_of;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDCLNT_STREAMOPTIONS_NONE, AudioCategory_Communications, AudioClientProperties,
    AUDIO_EFFECT, AUDIO_EFFECT_STATE_OFF, AUDIO_EFFECT_STATE_ON, IAudioCaptureClient, IAudioClient, IAudioClient2,
    IAudioEffectsManager, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eCapture, eCommunications,
};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PropVariantClear};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, STGM_READ,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{BOOL, GUID, HSTRING, Interface};

use crate::audio::SAMPLE_RATE;
use crate::capture::Capture;
use crate::devices::DeviceChoice;
use crate::error::{Error, Result};
use crate::mic_effects::MicEffect;
use crate::pcm::SampleQueue;

const BITS: u16 = 16;
const EVENT_WAIT_MS: u32 = 100;
const PCM_TAG: u16 = 1;

fn audio_error(what: &str, error: impl std::fmt::Display) -> Error {
    Error::Audio(format!("{what}: {error}"))
}

fn open_device(choice: &DeviceChoice) -> Result<IMMDevice> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|error| audio_error("device enumerator", error))?;
        match choice {
            DeviceChoice::SystemDefault => enumerator
                .GetDefaultAudioEndpoint(eCapture, eCommunications)
                .map_err(|error| audio_error("default communications microphone", error)),
            DeviceChoice::Device(key) => enumerator
                .GetDevice(&HSTRING::from(key.as_str()))
                .map_err(|error| audio_error("microphone endpoint", error)),
        }
    }
}

fn friendly_name(device: &IMMDevice) -> String {
    unsafe {
        let Ok(store) = device.OpenPropertyStore(STGM_READ) else { return String::new() };
        let Ok(mut value) = store.GetValue(&PKEY_Device_FriendlyName) else { return String::new() };
        let name = value.Anonymous.Anonymous.Anonymous.pwszVal.to_string().unwrap_or_default();
        let _ = PropVariantClear(&mut value as *mut PROPVARIANT);
        name
    }
}

fn initialise(device: &IMMDevice) -> Result<(IAudioClient, IAudioCaptureClient, HANDLE, Vec<MicEffect>)> {
    let block_align = BITS / 8;
    let format = WAVEFORMATEX {
        wFormatTag: PCM_TAG,
        nChannels: 1,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * u32::from(block_align),
        nBlockAlign: block_align,
        wBitsPerSample: BITS,
        cbSize: 0,
    };
    let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    unsafe {
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(|error| audio_error("activate microphone", error))?;
        let properties = AudioClientProperties {
            cbSize: size_of::<AudioClientProperties>() as u32,
            bIsOffload: BOOL(0),
            eCategory: AudioCategory_Communications,
            Options: AUDCLNT_STREAMOPTIONS_NONE,
        };
        client
            .cast::<IAudioClient2>()
            .and_then(|extended| extended.SetClientProperties(&raw const properties))
            .map_err(|error| audio_error("communications category", error))?;
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 0, 0, &raw const format, None)
            .map_err(|error| audio_error("initialize microphone", error))?;
        let effects = apply_effect_policy(&client);
        let event = CreateEventW(None, false, false, None).map_err(|error| audio_error("event", error))?;
        client.SetEventHandle(event).map_err(|error| audio_error("event handle", error))?;
        let capture = client.GetService::<IAudioCaptureClient>().map_err(|error| audio_error("capture service", error))?;
        client.Start().map_err(|error| audio_error("start microphone", error))?;
        Ok((client, capture, event, effects))
    }
}

fn drain(capture: &IAudioCaptureClient, queue: &SampleQueue) -> windows::core::Result<()> {
    unsafe {
        while capture.GetNextPacketSize()? > 0 {
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            let samples: Vec<i16> = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                vec![0; frames as usize]
            } else {
                std::slice::from_raw_parts(data.cast::<i16>(), frames as usize).to_vec()
            };
            capture.ReleaseBuffer(frames)?;
            queue.push(&samples);
        }
    }
    Ok(())
}

pub fn start_communications_microphone(choice: &DeviceChoice, queue: SampleQueue) -> Result<(Capture, String, Vec<MicEffect>)> {
    let (ready_sender, ready) = mpsc::channel::<Result<(String, Vec<MicEffect>)>>();
    let choice = choice.clone();
    let capture = Capture::spawn_thread("mic-communications", move |stop| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let started = open_device(&choice).and_then(|device| {
            let name = friendly_name(&device);
            initialise(&device).map(|(client, capture, event, effects)| (name, effects, client, capture, event))
        });
        match started {
            Ok((name, effects, client, capture, event)) => {
                let _ = ready_sender.send(Ok((name, effects)));
                while !stop.load(Ordering::SeqCst) {
                    let waited = unsafe { WaitForSingleObject(event, EVENT_WAIT_MS) };
                    if waited == WAIT_OBJECT_0 && drain(&capture, &queue).is_err() {
                        break;
                    }
                }
                unsafe {
                    let _ = client.Stop();
                    let _ = CloseHandle(event);
                }
            }
            Err(error) => {
                let _ = ready_sender.send(Err(error));
            }
        }
        unsafe { CoUninitialize() };
    })
    .map_err(|error| audio_error("microphone thread", error))?;
    let (name, effects) = ready.recv().map_err(|_| Error::Audio("microphone thread ended".into()))??;
    Ok((capture, name, effects))
}

/// Switches the Windows effects on this stream to what our chain expects and returns the resulting effect list.
fn apply_effect_policy(client: &IAudioClient) -> Vec<MicEffect> {
    unsafe {
        let Ok(manager) = client.GetService::<IAudioEffectsManager>() else { return Vec::new() };
        for effect in list_effects(&manager).unwrap_or_default() {
            if let Some(enabled) = effect.change() {
                let state = if enabled { AUDIO_EFFECT_STATE_ON } else { AUDIO_EFFECT_STATE_OFF };
                let _ = manager.SetAudioEffectState(GUID::from_u128(effect.id), state);
            }
        }
        list_effects(&manager).unwrap_or_default()
    }
}

fn list_effects(manager: &IAudioEffectsManager) -> Result<Vec<MicEffect>> {
    unsafe {
        let mut effects: *mut AUDIO_EFFECT = std::ptr::null_mut();
        let mut count = 0u32;
        manager.GetAudioEffects(&mut effects, &mut count).map_err(|error| audio_error("audio effects", error))?;
        if effects.is_null() {
            return Ok(Vec::new());
        }
        let listed = std::slice::from_raw_parts(effects, count as usize)
            .iter()
            .map(|effect| MicEffect { id: effect.id.to_u128(), enabled: effect.state.0 != 0, can_set: effect.canSetState.as_bool() })
            .collect();
        CoTaskMemFree(Some(effects.cast_const().cast()));
        Ok(listed)
    }
}

/// Opens the microphone the way a call does and lists the Windows effects running on that stream.
pub fn probe_effects(choice: &DeviceChoice) -> Result<(String, Vec<MicEffect>)> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let probed = open_device(choice).and_then(|device| {
        let (client, _capture, event, effects) = initialise(&device)?;
        unsafe {
            let _ = client.Stop();
            let _ = CloseHandle(event);
        }
        Ok((friendly_name(&device), effects))
    });
    unsafe { CoUninitialize() };
    probed
}
