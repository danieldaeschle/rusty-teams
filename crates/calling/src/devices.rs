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

pub fn resolve_index(devices: &[AudioDevice], choice: &DeviceChoice) -> Option<u16> {
    match choice {
        DeviceChoice::SystemDefault => devices
            .iter()
            .find(|device| device.is_system_alias())
            .or(devices.first())
            .map(|device| device.index),
        DeviceChoice::Device(key) => devices
            .iter()
            .find(|device| device.key() == *key)
            .map(|device| device.index),
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

pub fn select_input(factory: &PeerConnectionFactory, choice: &DeviceChoice) -> bool {
    let Some(index) = resolve_index(&enumerate(factory).inputs, choice) else {
        return false;
    };
    factory.stop_recording();
    let selected = factory.set_recording_device(index);
    factory.init_recording();
    factory.start_recording();
    selected
}

pub fn select_output(factory: &PeerConnectionFactory, choice: &DeviceChoice) -> bool {
    let Some(index) = resolve_index(&enumerate(factory).outputs, choice) else {
        return false;
    };
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
        assert_eq!(resolve_index(&devices, &DeviceChoice::Device("Headset".into())), Some(2));
        assert_eq!(resolve_index(&devices, &DeviceChoice::Device("Gone".into())), None);
    }

    #[test]
    fn the_system_default_resolves_to_the_alias_or_the_first_device() {
        let with_alias = [device(0, "USB", "u"), device(1, "default: USB", "")];
        assert_eq!(resolve_index(&with_alias, &DeviceChoice::SystemDefault), Some(1));
        let without_alias = [device(0, "Speakers", "s"), device(1, "Headset", "h")];
        assert_eq!(resolve_index(&without_alias, &DeviceChoice::SystemDefault), Some(0));
        assert_eq!(resolve_index(&[], &DeviceChoice::SystemDefault), None);
    }

    #[test]
    fn unnamed_devices_are_hidden() {
        let entries = menu_entries(&[device(0, "  ", "g")]);
        assert_eq!(entries.len(), 1);
    }
}
