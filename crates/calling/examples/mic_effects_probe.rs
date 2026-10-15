//! Lists the Windows effects (AEC, noise suppression, AGC, beamforming...) on the default communications microphone.

fn main() {
    #[cfg(windows)]
    match calling::probe_effects(&calling::devices::DeviceChoice::SystemDefault) {
        Ok((device, effects)) => {
            println!("microphone: {device}");
            for effect in effects {
                println!("  {:<28} on={} can_set={}", effect.name(), effect.enabled, effect.can_set);
            }
        }
        Err(error) => println!("probe failed: {error}"),
    }
    #[cfg(not(windows))]
    println!("Windows only");
}
