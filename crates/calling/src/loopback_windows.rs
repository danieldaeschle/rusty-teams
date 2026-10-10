use std::mem::{ManuallyDrop, size_of};
use std::sync::mpsc;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, E_FAIL, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, IAgileObject, IAgileObject_Impl};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{Interface, Ref};
use windows_core::implement;

use crate::audio::SAMPLE_RATE;
use crate::capture::Capture;
use crate::error::{Error, Result};
use crate::pcm::SampleQueue;

const CHANNELS: u16 = 2;
const BITS: u16 = 16;
const ACTIVATION_WAIT: Duration = Duration::from_secs(5);
const EVENT_WAIT_MS: u32 = 100;
const PCM_TAG: u16 = 1;

struct ActivatedClient(IAudioClient);

unsafe impl Send for ActivatedClient {}

#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct ActivationHandler {
    sender: mpsc::Sender<windows::core::Result<ActivatedClient>>,
}

impl IAgileObject_Impl for ActivationHandler_Impl {}

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationHandler_Impl {
    fn ActivateCompleted(&self, operation: Ref<IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let activated = (|| unsafe {
            let operation = operation.ok()?;
            let mut result = windows::core::HRESULT(0);
            let mut unknown = None;
            operation.GetActivateResult(&mut result, &mut unknown)?;
            result.ok()?;
            let unknown: windows::core::IUnknown = unknown.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            unknown.cast::<IAudioClient>().map(ActivatedClient)
        })();
        let _ = self.sender.send(activated);
        Ok(())
    }
}

fn audio_error(what: &str, error: impl std::fmt::Display) -> Error {
    Error::Audio(format!("{what}: {error}"))
}

fn activate_excluding(process_id: u32) -> Result<IAudioClient> {
    let mut parameters = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: process_id,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    let mut variant = ManuallyDrop::new(PROPVARIANT::default());
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob = BLOB {
            cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
            pBlobData: (&raw mut parameters).cast(),
        };
    }
    let (sender, receiver) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler = ActivationHandler { sender }.into();
    let _operation = unsafe {
        ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&raw const *variant), &handler)
    }
    .map_err(|error| audio_error("activate process loopback", error))?;
    receiver
        .recv_timeout(ACTIVATION_WAIT)
        .map_err(|_| Error::Audio("process loopback activation timed out".into()))?
        .map(|client| client.0)
        .map_err(|error| audio_error("process loopback activation", error))
}

fn initialise(client: &IAudioClient) -> Result<(IAudioCaptureClient, HANDLE)> {
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
    let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
        | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    unsafe {
        client
            .Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 0, 0, &raw const format, None)
            .map_err(|error| audio_error("initialize loopback", error))?;
        let event = CreateEventW(None, false, false, None).map_err(|error| audio_error("event", error))?;
        client.SetEventHandle(event).map_err(|error| audio_error("event handle", error))?;
        let capture = client.GetService::<IAudioCaptureClient>().map_err(|error| audio_error("capture service", error))?;
        client.Start().map_err(|error| audio_error("start loopback", error))?;
        Ok((capture, event))
    }
}

fn drain(capture: &IAudioCaptureClient, queue: &SampleQueue) -> windows::core::Result<()> {
    unsafe {
        while capture.GetNextPacketSize()? > 0 {
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            let mono: Vec<i16> = if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                vec![0; frames as usize]
            } else {
                let pairs = std::slice::from_raw_parts(data.cast::<i16>(), frames as usize * usize::from(CHANNELS));
                pairs.chunks_exact(usize::from(CHANNELS)).map(|pair| ((i32::from(pair[0]) + i32::from(pair[1])) / 2) as i16).collect()
            };
            capture.ReleaseBuffer(frames)?;
            queue.push(&mono);
        }
    }
    Ok(())
}

pub fn start_process_loopback(queue: SampleQueue, own_process_id: u32) -> Result<Capture> {
    let (ready_sender, ready) = mpsc::channel::<Result<()>>();
    let capture = Capture::spawn_thread("process-loopback", move |stop| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let started = activate_excluding(own_process_id).and_then(|client| initialise(&client).map(|(capture, event)| (client, capture, event)));
        match started {
            Ok((client, capture, event)) => {
                let _ = ready_sender.send(Ok(()));
                while !stop.load(std::sync::atomic::Ordering::SeqCst) {
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
    .map_err(|error| audio_error("loopback thread", error))?;
    ready.recv().map_err(|_| Error::Audio("loopback thread ended".into()))??;
    Ok(capture)
}
