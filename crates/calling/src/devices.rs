use std::sync::OnceLock;

use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt;

pub const DEFAULT_DEVICE_LABEL: &str = "Default device";
const SYSTEM_ALIAS_PREFIXES: [&str; 3] = ["default", "communication", "sysdefault"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub index: u16,
    pub guid: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DeviceChoice {
    #[default]
    SystemDefault,
    Device(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEntry {
    pub choice: DeviceChoice,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceLists {
    pub inputs: Vec<AudioDevice>,
    pub outputs: Vec<AudioDevice>,
}

impl AudioDevice {
    pub fn key(&self) -> String {
        if self.guid.is_empty() {
            self.name.clone()
        } else {
            self.guid.clone()
        }
    }

    fn is_system_alias(&self) -> bool {
        let name = self.name.to_lowercase();
        let guid = self.guid.to_lowercase();
        SYSTEM_ALIAS_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix) || guid == *prefix)
    }
}

pub fn menu_entries(devices: &[AudioDevice]) -> Vec<DeviceEntry> {
    let default_entry = DeviceEntry {
        choice: DeviceChoice::SystemDefault,
        label: DEFAULT_DEVICE_LABEL.to_owned(),
    };
    let named = devices
        .iter()
        .filter(|device| !device.is_system_alias() && !device.name.trim().is_empty())
        .map(|device| DeviceEntry {
            choice: DeviceChoice::Device(device.key()),
            label: device.name.clone(),
        });
    std::iter::once(default_entry).chain(named).collect()
}

impl DeviceLists {
    pub fn input_entries(&self) -> Vec<DeviceEntry> {
        menu_entries(&self.inputs)
    }

    pub fn output_entries(&self) -> Vec<DeviceEntry> {
        menu_entries(&self.outputs)
    }
}

pub fn shared_factory() -> PeerConnectionFactory {
    static FACTORY: OnceLock<PeerConnectionFactory> = OnceLock::new();
    FACTORY.get_or_init(PeerConnectionFactory::default).clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Capture,
    Render,
}

/// Endpoint id of the Windows default communications device, which the ADM reports as the device guid.
#[cfg(windows)]
pub fn system_default_id(flow: Flow) -> Option<String> {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let id = default_endpoint_id(flow);
    if initialized {
        unsafe { CoUninitialize() };
    }
    id
}

#[cfg(windows)]
fn default_endpoint_id(flow: Flow) -> Option<String> {
    use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eCommunications, eRender};
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
    let data_flow = match flow {
        Flow::Capture => eCapture,
        Flow::Render => eRender,
    };
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let device = enumerator.GetDefaultAudioEndpoint(data_flow, eCommunications).ok()?;
        let id = device.GetId().ok()?;
        let text = id.to_string().ok();
        CoTaskMemFree(Some(id.0 as *const _));
        text
    }
}

#[cfg(not(windows))]
pub fn system_default_id(_flow: Flow) -> Option<String> {
    None
}

pub fn resolve_index(devices: &[AudioDevice], choice: &DeviceChoice, system_default: Option<&str>) -> Option<u16> {
    match choice {
        DeviceChoice::SystemDefault => system_default
            .and_then(|id| devices.iter().find(|device| device.guid.eq_ignore_ascii_case(id)))
            .or_else(|| devices.iter().find(|device| device.is_system_alias()))
            .or(devices.first())
            .map(|device| device.index),
        DeviceChoice::Device(key) => devices
            .iter()
            .find(|device| device.key() == *key)
            .map(|device| device.index),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedDevice {
    pub index: u16,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reselect {
    Keep,
    Select(u16),
    FallBack(u16),
}

pub fn applied_device(devices: &[AudioDevice], choice: &DeviceChoice, system_default: Option<&str>) -> Option<AppliedDevice> {
    let index = resolve_index(devices, choice, system_default)?;
    device_at(devices, index)
}

fn device_at(devices: &[AudioDevice], index: u16) -> Option<AppliedDevice> {
    devices
        .iter()
        .find(|device| device.index == index)
        .map(|device| AppliedDevice { index, key: device.key() })
}

pub fn reselect(applied: Option<&AppliedDevice>, devices: &[AudioDevice], choice: &DeviceChoice, system_default: Option<&str>) -> Reselect {
    let unchanged = |index: u16| device_at(devices, index).as_ref() == applied;
    match resolve_index(devices, choice, system_default) {
        Some(index) if unchanged(index) => Reselect::Keep,
        Some(index) => Reselect::Select(index),
        None => match resolve_index(devices, &DeviceChoice::SystemDefault, system_default) {
            Some(index) if !unchanged(index) && matches!(choice, DeviceChoice::Device(_)) => Reselect::FallBack(index),
            _ => Reselect::Keep,
        },
    }
}

pub fn disconnect_notice(previous: &[AudioDevice], choice: &DeviceChoice, fallback: Option<&AudioDevice>) -> String {
    let lost = previous
        .iter()
        .find(|device| matches!(choice, DeviceChoice::Device(key) if device.key() == *key))
        .map_or("Device", |device| device.name.as_str());
    match fallback {
        Some(device) => format!("{lost} disconnected. Using {}.", device.name),
        None => format!("{lost} disconnected."),
    }
}

fn enumerate(factory: &PeerConnectionFactory) -> DeviceLists {
    let inputs = (0..factory.recording_devices().max(0) as u16)
        .map(|index| AudioDevice {
            index,
            guid: factory.recording_device_guid(index),
            name: factory.recording_device_name(index),
        })
        .collect();
    let outputs = (0..factory.playout_devices().max(0) as u16)
        .map(|index| AudioDevice {
            index,
            guid: factory.playout_device_guid(index),
            name: factory.playout_device_name(index),
        })
        .collect();
    DeviceLists { inputs, outputs }
}

pub fn list_devices(factory: &PeerConnectionFactory) -> DeviceLists {
    if !factory.acquire_platform_adm() {
        return DeviceLists::default();
    }
    let lists = enumerate(factory);
    factory.release_platform_adm();
    lists
}

fn default_for(choice: &DeviceChoice, flow: Flow) -> Option<String> {
    (*choice == DeviceChoice::SystemDefault).then(|| system_default_id(flow)).flatten()
}

pub fn select_input(factory: &PeerConnectionFactory, choice: &DeviceChoice) -> bool {
    let Some(index) = resolve_index(&enumerate(factory).inputs, choice, default_for(choice, Flow::Capture).as_deref()) else {
        return false;
    };
    select_input_index(factory, index)
}

pub fn select_input_index(factory: &PeerConnectionFactory, index: u16) -> bool {
    factory.stop_recording();
    let selected = factory.set_recording_device(index);
    factory.init_recording();
    factory.start_recording();
    selected
}

pub fn select_output(factory: &PeerConnectionFactory, choice: &DeviceChoice) -> bool {
    let Some(index) = resolve_index(&enumerate(factory).outputs, choice, default_for(choice, Flow::Render).as_deref()) else {
        return false;
    };
    select_output_index(factory, index)
}

pub fn select_output_index(factory: &PeerConnectionFactory, index: u16) -> bool {
    factory.stop_playout();
    let selected = factory.set_playout_device(index);
    factory.init_playout();
    factory.start_playout();
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(index: u16, name: &str, guid: &str) -> AudioDevice {
        AudioDevice {
            index,
            guid: guid.into(),
            name: name.into(),
        }
    }

    #[test]
    fn default_device_comes_first_and_aliases_are_dropped() {
        let devices = [
            device(0, "Headset Microphone", "{b}"),
            device(1, "Default - Headset Microphone", "{a}"),
            device(2, "Communication - Headset Microphone", "{c}"),
            device(3, "Webcam Microphone", "{w}"),
        ];
        let entries = menu_entries(&devices);
        let labels: Vec<&str> = entries.iter().map(|entry| entry.label.as_str()).collect();
        assert_eq!(labels, ["Default device", "Headset Microphone", "Webcam Microphone"]);
        assert_eq!(entries[0].choice, DeviceChoice::SystemDefault);
        assert_eq!(entries[1].choice, DeviceChoice::Device("{b}".into()));
    }

    #[test]
    fn no_devices_still_offers_the_default_entry() {
        let entries = menu_entries(&[]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, DEFAULT_DEVICE_LABEL);
    }

    #[test]
    fn linux_default_names_are_aliases() {
        let devices = [device(0, "default", "default"), device(1, "sysdefault:CARD=PCH", "x"), device(2, "USB Audio", "usb")];
        let labels: Vec<String> = menu_entries(&devices).into_iter().map(|entry| entry.label).collect();
        assert_eq!(labels, ["Default device", "USB Audio"]);
    }

    #[test]
    fn devices_without_a_guid_are_keyed_by_name() {
        let devices = [device(0, "default: Speakers", ""), device(1, "Speakers", ""), device(2, "Headset", "")];
        let entries = menu_entries(&devices);
        assert_eq!(entries[1].choice, DeviceChoice::Device("Speakers".into()));
        assert_eq!(resolve_index(&devices, &DeviceChoice::Device("Headset".into()), None), Some(2));
        assert_eq!(resolve_index(&devices, &DeviceChoice::Device("Gone".into()), None), None);
    }

    #[test]
    fn the_system_default_resolves_to_the_alias_or_the_first_device() {
        let with_alias = [device(0, "USB", "u"), device(1, "default: USB", "")];
        assert_eq!(resolve_index(&with_alias, &DeviceChoice::SystemDefault, None), Some(1));
        let without_alias = [device(0, "Speakers", "s"), device(1, "Headset", "h")];
        assert_eq!(resolve_index(&without_alias, &DeviceChoice::SystemDefault, None), Some(0));
        assert_eq!(resolve_index(&[], &DeviceChoice::SystemDefault, None), None);
    }

    #[test]
    fn the_system_default_follows_the_platform_default_endpoint() {
        let devices = [
            device(0, "Speakers (Realtek)", "{0.0.0.00000000}.{aaaa}"),
            device(1, "Headset Earphone", "{0.0.0.00000000}.{BBBB}"),
        ];
        let headset = Some("{0.0.0.00000000}.{bbbb}");
        assert_eq!(resolve_index(&devices, &DeviceChoice::SystemDefault, headset), Some(1));
        assert_eq!(resolve_index(&devices, &DeviceChoice::SystemDefault, Some("{0.0.0.00000000}.{gone}")), Some(0));
        assert_eq!(resolve_index(&devices, &DeviceChoice::Device("{0.0.0.00000000}.{aaaa}".into()), headset), Some(0));
    }

    fn headset_lists() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
        let before = vec![device(0, "Speakers", "{s}")];
        let after = vec![device(0, "Speakers", "{s}"), device(1, "Headset", "{h}")];
        (before, after)
    }

    #[test]
    fn the_default_moving_to_a_new_headset_selects_it() {
        let (before, after) = headset_lists();
        let applied = applied_device(&before, &DeviceChoice::SystemDefault, Some("{s}"));
        assert_eq!(reselect(applied.as_ref(), &after, &DeviceChoice::SystemDefault, Some("{h}")), Reselect::Select(1));
    }

    #[test]
    fn nothing_changed_keeps_the_device_running() {
        let (_, after) = headset_lists();
        let applied = applied_device(&after, &DeviceChoice::SystemDefault, Some("{h}"));
        assert_eq!(reselect(applied.as_ref(), &after, &DeviceChoice::SystemDefault, Some("{h}")), Reselect::Keep);
        let explicit = DeviceChoice::Device("{h}".into());
        let applied = applied_device(&after, &explicit, Some("{s}"));
        assert_eq!(reselect(applied.as_ref(), &after, &explicit, Some("{s}")), Reselect::Keep);
    }

    #[test]
    fn unplugging_the_selected_headset_falls_back_and_returns_when_it_comes_back() {
        let (before, after) = headset_lists();
        let explicit = DeviceChoice::Device("{h}".into());
        let applied = applied_device(&after, &explicit, Some("{h}"));
        assert_eq!(reselect(applied.as_ref(), &before, &explicit, Some("{s}")), Reselect::FallBack(0));
        let fallen_back = applied_device(&before, &DeviceChoice::SystemDefault, Some("{s}"));
        assert_eq!(reselect(fallen_back.as_ref(), &before, &explicit, Some("{s}")), Reselect::Keep);
        assert_eq!(reselect(fallen_back.as_ref(), &after, &explicit, Some("{s}")), Reselect::Select(1));
    }

    #[test]
    fn a_shifted_index_with_the_same_key_is_reselected() {
        let explicit = DeviceChoice::Device("{h}".into());
        let before = [device(0, "Speakers", "{s}"), device(1, "Headset", "{h}")];
        let after = [device(0, "Dock", "{d}"), device(1, "Speakers", "{s}"), device(2, "Headset", "{h}")];
        let applied = applied_device(&before, &explicit, None);
        assert_eq!(reselect(applied.as_ref(), &after, &explicit, None), Reselect::Select(2));
    }

    #[test]
    fn a_removed_default_selects_the_new_default() {
        let (before, after) = headset_lists();
        let applied = applied_device(&after, &DeviceChoice::SystemDefault, Some("{h}"));
        assert_eq!(reselect(applied.as_ref(), &before, &DeviceChoice::SystemDefault, Some("{s}")), Reselect::Select(0));
    }

    #[test]
    fn the_notice_names_both_devices() {
        let (before, after) = headset_lists();
        let explicit = DeviceChoice::Device("{h}".into());
        assert_eq!(disconnect_notice(&after, &explicit, before.first()), "Headset disconnected. Using Speakers.");
    }

    #[test]
    fn unnamed_devices_are_hidden() {
        let entries = menu_entries(&[device(0, "  ", "g")]);
        assert_eq!(entries.len(), 1);
    }
}
