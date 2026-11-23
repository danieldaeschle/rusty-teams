use std::time::Duration;

use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::time::Instant;

pub const DEVICE_CHANGE_QUIET: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
pub struct Debounce {
    deadline: Option<Instant>,
    touched_ids: Vec<String>,
}

impl Debounce {
    pub fn signal(&mut self, now: Instant) {
        self.signal_device(now, None);
    }

    pub fn signal_device(&mut self, now: Instant, device_id: Option<String>) {
        self.deadline = Some(now + DEVICE_CHANGE_QUIET);
        if let Some(id) = device_id
            && !self
                .touched_ids
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&id))
        {
            self.touched_ids.push(id);
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub fn take_due(&mut self, now: Instant) -> bool {
        self.take_due_devices(now).is_some()
    }

    pub fn take_due_devices(&mut self, now: Instant) -> Option<Vec<String>> {
        let due = self.deadline.is_some_and(|deadline| now >= deadline);
        if due {
            self.deadline = None;
            Some(std::mem::take(&mut self.touched_ids))
        } else {
            None
        }
    }
}

pub struct DeviceWatcher {
    #[cfg(windows)]
    _stop: std::sync::mpsc::Sender<()>,
}

pub fn endpoint_delta(previous: &[String], current: &[String]) -> Vec<String> {
    let mut changed = Vec::new();
    for id in previous.iter().chain(current.iter()) {
        let before = previous.iter().any(|key| key.eq_ignore_ascii_case(id));
        let after = current.iter().any(|key| key.eq_ignore_ascii_case(id));
        let seen = changed.iter().any(|key: &String| key.eq_ignore_ascii_case(id));
        if before != after && !seen {
            changed.push(id.clone());
        }
    }
    changed
}

pub fn watch() -> (DeviceWatcher, UnboundedReceiver<Option<String>>) {
    let (sender, receiver) = unbounded_channel();
    (platform::start(sender), receiver)
}

#[cfg(windows)]
mod platform {
    use tokio::sync::mpsc::UnboundedSender;
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::{
        DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
        IMMNotificationClient_Impl, MMDeviceEnumerator, eCommunications, eConsole,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::core::PCWSTR;
    use windows_core::implement;

    use super::DeviceWatcher;

    fn id_string(device_id: &PCWSTR) -> Option<String> {
        if device_id.is_null() {
            return None;
        }
        unsafe { device_id.to_string().ok() }.filter(|id| !id.is_empty())
    }

    #[implement(IMMNotificationClient)]
    struct Client {
        sender: UnboundedSender<Option<String>>,
    }

    impl IMMNotificationClient_Impl for Client_Impl {
        fn OnDeviceStateChanged(
            &self,
            device_id: &PCWSTR,
            _state: DEVICE_STATE,
        ) -> windows::core::Result<()> {
            let _ = self.sender.send(id_string(device_id));
            Ok(())
        }

        fn OnDeviceAdded(&self, device_id: &PCWSTR) -> windows::core::Result<()> {
            let _ = self.sender.send(id_string(device_id));
            Ok(())
        }

        fn OnDeviceRemoved(&self, device_id: &PCWSTR) -> windows::core::Result<()> {
            let _ = self.sender.send(id_string(device_id));
            Ok(())
        }

        fn OnDefaultDeviceChanged(
            &self,
            _flow: EDataFlow,
            role: ERole,
            _device_id: &PCWSTR,
        ) -> windows::core::Result<()> {
            if role == eConsole || role == eCommunications {
                let _ = self.sender.send(None);
            }
            Ok(())
        }

        fn OnPropertyValueChanged(
            &self,
            _device_id: &PCWSTR,
            _key: &PROPERTYKEY,
        ) -> windows::core::Result<()> {
            Ok(())
        }
    }

    pub fn start(sender: UnboundedSender<Option<String>>) -> DeviceWatcher {
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            if let Ok(enumerator) =
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
            {
                let client: IMMNotificationClient = Client { sender }.into();
                if enumerator
                    .RegisterEndpointNotificationCallback(&client)
                    .is_ok()
                {
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

    pub fn start(_sender: UnboundedSender<Option<String>>) -> DeviceWatcher {
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

    #[test]
    fn debounce_collects_unique_touched_device_ids() {
        let start = Instant::now();
        let mut debounce = Debounce::default();
        debounce.signal_device(start, Some("{dev-a}".into()));
        debounce.signal_device(start + Duration::from_millis(100), Some("{DEV-A}".into()));
        debounce.signal_device(start + Duration::from_millis(200), None);
        debounce.signal_device(start + Duration::from_millis(250), Some("{dev-b}".into()));
        let ids = debounce
            .take_due_devices(start + Duration::from_millis(800))
            .expect("due");
        assert_eq!(ids, vec!["{dev-a}".to_owned(), "{dev-b}".to_owned()]);
        assert_eq!(
            debounce.take_due_devices(start + Duration::from_millis(900)),
            None
        );
    }

    #[test]
    fn a_device_that_leaves_and_returns_inside_the_quiet_period_is_one_event() {
        let start = Instant::now();
        let present = vec!["{headset}".to_owned()];
        let gone = Vec::new();
        let mut debounce = Debounce::default();
        for id in endpoint_delta(&present, &gone) {
            debounce.signal_device(start, Some(id));
        }
        for id in endpoint_delta(&gone, &present) {
            debounce.signal_device(start + Duration::from_millis(200), Some(id));
        }
        assert_eq!(
            debounce.take_due_devices(start + DEVICE_CHANGE_QUIET + Duration::from_millis(200)),
            Some(vec!["{headset}".to_owned()])
        );
    }
}
