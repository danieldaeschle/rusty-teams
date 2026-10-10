use calling::devices::{list_devices, menu_entries, shared_factory};

fn main() {
    let lists = list_devices(&shared_factory());
    for (kind, devices) in [("input", &lists.inputs), ("output", &lists.outputs)] {
        println!("{kind} devices: {}", devices.len());
        for device in devices {
            println!("  {}: {:?} guid {:?}", device.index, device.name, device.guid);
        }
        let labels: Vec<String> = menu_entries(devices).into_iter().map(|entry| entry.label).collect();
        println!("  menu: {labels:?}");
    }
}
