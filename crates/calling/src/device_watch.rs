use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::time::Instant;

pub const DEVICE_CHANGE_QUIET: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
pub struct Debounce {
    deadline: Option<Instant>,
}

impl Debounce {
    pub fn signal(&mut self, now: Instant) {
        self.deadline = Some(now + DEVICE_CHANGE_QUIET);
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub fn take_due(&mut self, now: Instant) -> bool {
        let due = self.deadline.is_some_and(|deadline| now >= deadline);
        if due {
            self.deadline = None;
        }
        due
    }
}

pub struct DeviceWatcher {
    #[cfg(windows)]
    _stop: std::sync::mpsc::Sender<()>,
}

pub fn watch() -> (DeviceWatcher, UnboundedReceiver<()>) {
    let (sender, receiver) = unbounded_channel();
    (platform::start(sender), receiver)
}

#[cfg(windows)]
mod platform {
    use tokio::sync::mpsc::UnboundedSender;
    use windows::Win32::Media::Audio::{
        DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, eCommunications,
    };
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
    use windows::core::PCWSTR;
    use windows_core::implement;

    use super::DeviceWatcher;

    #[implement(IMMNotificationClient)]
    struct Client {
        sender: UnboundedSender<()>,
    }

    impl IMMNotificationClient_Impl for Client_Impl {
        fn OnDeviceStateChanged(&self, _device_id: &PCWSTR, _state: DEVICE_STATE) -> windows::core::Result<()> {
            let _ = self.sender.send(());
            Ok(())
        }

        fn OnDeviceAdded(&self, _device_id: &PCWSTR) -> windows::core::Result<()> {
            let _ = self.sender.send(());
            Ok(())
        }

        fn OnDeviceRemoved(&self, _device_id: &PCWSTR) -> windows::core::Result<()> {
            let _ = self.sender.send(());
            Ok(())
        }

        fn OnDefaultDeviceChanged(&self, _flow: EDataFlow, role: ERole, _device_id: &PCWSTR) -> windows::core::Result<()> {
            if role == eCommunications {
                let _ = self.sender.send(());
            }
            Ok(())
        }

        fn OnPropertyValueChanged(&self, _device_id: &PCWSTR, _key: &PROPERTYKEY) -> windows::core::Result<()> {
            Ok(())
        }
    }

    pub fn start(sender: UnboundedSender<()>) -> DeviceWatcher {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            if let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) {
                let client: IMMNotificationClient = Client { sender }.into();
                if enumerator.RegisterEndpointNotificationCallback(&client).is_ok() {
                    let _ = stopped.recv();
                    let _ = enumerator.UnregisterEndpointNotificationCallback(&client);
                }
            }
            if initialized {
                CoUninitialize();
            }
        });
        DeviceWatcher { _stop: stop }
    }
}

#[cfg(not(windows))]
mod platform {
    use tokio::sync::mpsc::UnboundedSender;

    use super::DeviceWatcher;

    pub fn start(_sender: UnboundedSender<()>) -> DeviceWatcher {
        DeviceWatcher {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_of_events_fires_once_after_the_quiet_period() {
        let start = Instant::now();
        let mut debounce = Debounce::default();
        assert!(!debounce.take_due(start));
        debounce.signal(start);
        debounce.signal(start + Duration::from_millis(300));
        assert!(!debounce.take_due(start + Duration::from_millis(600)));
        assert!(debounce.take_due(start + Duration::from_millis(800)));
        assert!(!debounce.take_due(start + Duration::from_millis(900)));
        assert_eq!(debounce.deadline(), None);
    }
}
