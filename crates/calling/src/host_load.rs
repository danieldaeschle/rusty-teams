use std::collections::HashMap;

/// One named thread and the CPU time it has used since the process started, in milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadCpu {
    pub name: String,
    pub cpu_ms: u64,
}

pub fn recorded_on_this_platform() -> bool {
    cfg!(target_os = "macos")
}

/// CPU used since the previous sample. The first sample reports the time already used.
pub fn cpu_deltas(
    current: &[ThreadCpu],
    previous: &mut HashMap<String, u64>,
) -> Vec<(String, u64)> {
    let mut seen = Vec::with_capacity(current.len());
    let mut deltas = Vec::with_capacity(current.len());
    for thread in current {
        let before = previous.get(&thread.name).copied().unwrap_or(0);
        deltas.push((thread.name.clone(), thread.cpu_ms.saturating_sub(before)));
        previous.insert(thread.name.clone(), thread.cpu_ms);
        seen.push(thread.name.clone());
    }
    previous.retain(|name, _| seen.iter().any(|kept| kept == name));
    deltas.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    deltas
}

pub fn host_load_line(deltas: &[(String, u64)], effects: &str, thermal: &str) -> String {
    let threads = if deltas.is_empty() {
        "no thread cpu".to_owned()
    } else {
        deltas
            .iter()
            .map(|(name, cpu_ms)| format!("{name} {cpu_ms}ms"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!("{threads}; {effects}; thermal {thermal}")
}

/// Cumulative CPU time per thread. Empty off macOS, and empty when the kernel call fails.
pub fn thread_cpu() -> Vec<ThreadCpu> {
    #[cfg(target_os = "macos")]
    {
        macos::thread_cpu()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

pub fn thermal_state() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        macos::thermal_state()
    }
    #[cfg(not(target_os = "macos"))]
    {
        "unavailable"
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::ThreadCpu;

    pub fn thermal_state() -> &'static str {
        let state = objc2_foundation::NSProcessInfo::processInfo().thermalState();
        match state {
            objc2_foundation::NSProcessInfoThermalState::Nominal => "nominal",
            objc2_foundation::NSProcessInfoThermalState::Fair => "fair",
            objc2_foundation::NSProcessInfoThermalState::Serious => "serious",
            objc2_foundation::NSProcessInfoThermalState::Critical => "critical",
            _ => "nominal",
        }
    }

    pub fn thread_cpu() -> Vec<ThreadCpu> {
        unsafe {
            let task = libc::mach_task_self();
            let mut threads: libc::thread_act_array_t = std::ptr::null_mut();
            let mut count: libc::mach_msg_type_number_t = 0;
            if libc::task_threads(task, &mut threads, &mut count) != libc::KERN_SUCCESS {
                return Vec::new();
            }
            let mut found = Vec::with_capacity(count as usize);
            for index in 0..count {
                let thread = *threads.add(index as usize);
                let mut info: libc::thread_basic_info = std::mem::zeroed();
                let mut info_count = libc::THREAD_BASIC_INFO_COUNT;
                let status = libc::thread_info(
                    thread,
                    libc::THREAD_BASIC_INFO,
                    &mut info as *mut libc::thread_basic_info as libc::thread_info_t,
                    &mut info_count,
                );
                if status != libc::KERN_SUCCESS {
                    continue;
                }
                found.push(ThreadCpu {
                    name: thread_name(thread),
                    cpu_ms: cpu_ms(&info),
                });
            }
            let bytes = count as libc::vm_size_t
                * std::mem::size_of::<libc::thread_act_t>() as libc::vm_size_t;
            let _ = libc::vm_deallocate(task, threads as libc::vm_address_t, bytes);
            found
        }
    }

    fn cpu_ms(info: &libc::thread_basic_info) -> u64 {
        let part = |time: libc::time_value_t| {
            let seconds = i64::from(time.seconds).max(0);
            let micros = i64::from(time.microseconds).max(0);
            seconds.saturating_mul(1000) + micros / 1000
        };
        part(info.user_time).saturating_add(part(info.system_time)) as u64
    }

    fn thread_name(thread: libc::thread_act_t) -> String {
        unsafe {
            let pthread = libc::pthread_from_mach_thread_np(thread);
            if !pthread.is_null() {
                let mut buffer = [0u8; 64];
                if libc::pthread_getname_np(pthread, buffer.as_mut_ptr().cast(), buffer.len()) == 0
                {
                    let end = buffer
                        .iter()
                        .position(|byte| *byte == 0)
                        .unwrap_or(buffer.len());
                    if let Ok(name) = std::str::from_utf8(&buffer[..end]) {
                        let name = name.trim();
                        if !name.is_empty() {
                            return name.to_owned();
                        }
                    }
                }
            }
            format!("tid-{thread}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_load_line_names_the_hot_thread_the_effects_and_the_thermal_state() {
        let mut previous = HashMap::new();
        let first = cpu_deltas(
            &[
                ThreadCpu {
                    name: "camera".into(),
                    cpu_ms: 40,
                },
                ThreadCpu {
                    name: "camera effects".into(),
                    cpu_ms: 5,
                },
            ],
            &mut previous,
        );
        let line = host_load_line(
            &first,
            "blur=off standout=off framing=off brightness=off soft_focus=off",
            "nominal",
        );
        assert_eq!(
            line,
            "camera 40ms, camera effects 5ms; blur=off standout=off framing=off brightness=off soft_focus=off; thermal nominal"
        );
        let second = cpu_deltas(
            &[
                ThreadCpu {
                    name: "camera".into(),
                    cpu_ms: 90,
                },
                ThreadCpu {
                    name: "camera effects".into(),
                    cpu_ms: 5,
                },
            ],
            &mut previous,
        );
        assert_eq!(
            second,
            vec![("camera".into(), 50), ("camera effects".into(), 0)]
        );
    }

    #[test]
    fn host_load_is_recorded_only_on_macos() {
        assert_eq!(recorded_on_this_platform(), cfg!(target_os = "macos"));
        if !recorded_on_this_platform() {
            assert!(thread_cpu().is_empty());
            assert_eq!(thermal_state(), "unavailable");
        }
    }
}
