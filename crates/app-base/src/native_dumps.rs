use std::path::PathBuf;

#[cfg(windows)]
pub fn install(directory: PathBuf, crash_log: PathBuf) {
    windows_impl::install(directory, crash_log);
}

#[cfg(not(windows))]
pub fn install(_directory: PathBuf, _crash_log: PathBuf) {}

#[cfg(windows)]
pub fn heartbeat() {
    windows_impl::heartbeat();
}

#[cfg(not(windows))]
pub fn heartbeat() {}

#[cfg(any(windows, test))]
mod policy {
    use std::time::{Duration, SystemTime};

    pub struct StallDetector {
        threshold: Duration,
        reported: bool,
    }

    impl StallDetector {
        pub fn new(threshold: Duration) -> Self {
            Self {
                threshold,
                reported: false,
            }
        }

        pub fn observe(&mut self, heartbeat_age: Option<Duration>) -> Option<Duration> {
            let age = heartbeat_age?;
            if age < self.threshold {
                self.reported = false;
                return None;
            }
            if self.reported {
                return None;
            }
            self.reported = true;
            Some(age)
        }
    }

    pub fn dumps_to_delete(
        mut dumps: Vec<(String, SystemTime)>,
        maximum_after_write: usize,
    ) -> Vec<String> {
        let keep = maximum_after_write.saturating_sub(1);
        if dumps.len() <= keep {
            return Vec::new();
        }
        dumps.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
        let surplus = dumps.len() - keep;
        dumps.into_iter().take(surplus).map(|dump| dump.0).collect()
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::fs::{self, File, OpenOptions};
    use std::io::Write;
    use std::os::windows::io::AsRawHandle;
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::{Duration, Instant, SystemTime};

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Diagnostics::Debug::{
        EXCEPTION_POINTERS, MINIDUMP_EXCEPTION_INFORMATION, MINIDUMP_TYPE,
        MiniDumpWithIndirectlyReferencedMemory, MiniDumpWithThreadInfo,
        MiniDumpWithUnloadedModules, MiniDumpWriteDump, SetUnhandledExceptionFilter,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId,
    };
    use windows::core::BOOL;

    use super::policy::{StallDetector, dumps_to_delete};

    const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
    const HANG_THRESHOLD: Duration = Duration::from_secs(10);
    const MAX_DUMP_FILES: usize = 5;
    const WATCHDOG_INTERVAL: Duration = Duration::from_secs(1);

    struct Paths {
        directory: PathBuf,
        crash_log: PathBuf,
    }

    static PATHS: OnceLock<Paths> = OnceLock::new();
    static STARTED: OnceLock<Instant> = OnceLock::new();
    static LAST_HEARTBEAT_MILLIS: AtomicU64 = AtomicU64::new(0);
    static WRITING: AtomicBool = AtomicBool::new(false);

    pub fn install(directory: PathBuf, crash_log: PathBuf) {
        if fs::create_dir_all(&directory).is_err() {
            return;
        }
        if PATHS
            .set(Paths {
                directory,
                crash_log,
            })
            .is_err()
        {
            return;
        }
        STARTED.get_or_init(Instant::now);
        unsafe {
            SetUnhandledExceptionFilter(Some(exception_filter));
            libc::signal(libc::SIGABRT, abort_handler as *const () as usize);
        }
        let _ = std::thread::Builder::new()
            .name("hang-watchdog".to_owned())
            .spawn(watch_for_hangs);
    }

    pub fn heartbeat() {
        let elapsed = STARTED.get_or_init(Instant::now).elapsed();
        LAST_HEARTBEAT_MILLIS.store(elapsed.as_millis() as u64 + 1, Ordering::Relaxed);
    }

    fn heartbeat_age() -> Option<Duration> {
        let stored = LAST_HEARTBEAT_MILLIS.load(Ordering::Relaxed);
        if stored == 0 {
            return None;
        }
        let elapsed = STARTED.get()?.elapsed();
        Some(elapsed.saturating_sub(Duration::from_millis(stored - 1)))
    }

    fn watch_for_hangs() {
        let mut detector = StallDetector::new(HANG_THRESHOLD);
        loop {
            std::thread::sleep(WATCHDOG_INTERVAL);
            let Some(stalled_for) = detector.observe(heartbeat_age()) else {
                continue;
            };
            if let Some(file_name) = write_dump("hang", None) {
                append_crash_log(&format!("hang {}s dump {file_name}", stalled_for.as_secs()));
            }
        }
    }

    unsafe extern "system" fn exception_filter(pointers: *const EXCEPTION_POINTERS) -> i32 {
        let code = unsafe {
            pointers
                .as_ref()
                .and_then(|pointers| pointers.ExceptionRecord.as_ref())
                .map(|record| record.ExceptionCode.0)
                .unwrap_or_default()
        };
        if let Some(file_name) = write_dump("crash", Some(pointers)) {
            append_crash_log(&format!("native crash 0x{code:08x} dump {file_name}"));
        }
        EXCEPTION_CONTINUE_SEARCH
    }

    extern "C" fn abort_handler(_signal: i32) {
        if let Some(file_name) = write_dump("abort", None) {
            append_crash_log(&format!("abort dump {file_name}"));
        }
        unsafe {
            libc::signal(libc::SIGABRT, libc::SIG_DFL);
            libc::abort();
        }
    }

    fn write_dump(prefix: &str, pointers: Option<*const EXCEPTION_POINTERS>) -> Option<String> {
        let paths = PATHS.get()?;
        if WRITING.swap(true, Ordering::SeqCst) {
            return None;
        }
        let file_name = write_dump_file(paths, prefix, pointers);
        WRITING.store(false, Ordering::SeqCst);
        file_name
    }

    fn write_dump_file(
        paths: &Paths,
        prefix: &str,
        pointers: Option<*const EXCEPTION_POINTERS>,
    ) -> Option<String> {
        prune_dumps(&paths.directory);
        let file_name = format!(
            "{prefix}-{}.dmp",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        );
        let file = File::create(paths.directory.join(&file_name)).ok()?;
        let mut exception_information = pointers.map(|pointers| MINIDUMP_EXCEPTION_INFORMATION {
            ThreadId: unsafe { GetCurrentThreadId() },
            ExceptionPointers: pointers as *mut EXCEPTION_POINTERS,
            ClientPointers: BOOL(0),
        });
        let exception_parameter = exception_information
            .as_mut()
            .map(|information| information as *const MINIDUMP_EXCEPTION_INFORMATION);
        let dump_type = MINIDUMP_TYPE(
            MiniDumpWithThreadInfo.0
                | MiniDumpWithUnloadedModules.0
                | MiniDumpWithIndirectlyReferencedMemory.0,
        );
        let written = unsafe {
            MiniDumpWriteDump(
                GetCurrentProcess(),
                GetCurrentProcessId(),
                HANDLE(file.as_raw_handle()),
                dump_type,
                exception_parameter,
                None,
                None,
            )
        };
        written.ok().map(|_| file_name)
    }

    fn prune_dumps(directory: &Path) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        let dumps = entries
            .flatten()
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "dmp")
            })
            .map(|entry| {
                let modified = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (entry.file_name().to_string_lossy().into_owned(), modified)
            })
            .collect();
        for file_name in dumps_to_delete(dumps, MAX_DUMP_FILES) {
            let _ = fs::remove_file(directory.join(file_name));
        }
    }

    fn append_crash_log(detail: &str) {
        let Some(paths) = PATHS.get() else {
            return;
        };
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&paths.crash_log)
        {
            let _ = writeln!(
                file,
                "=== {} build {} {detail}",
                chrono::Utc::now().to_rfc3339(),
                crate::version::build_version(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::policy::{StallDetector, dumps_to_delete};

    fn seconds(value: u64) -> Option<Duration> {
        Some(Duration::from_secs(value))
    }

    #[test]
    fn no_report_before_first_heartbeat() {
        let mut detector = StallDetector::new(Duration::from_secs(10));
        assert_eq!(detector.observe(None), None);
    }

    #[test]
    fn reports_once_after_threshold() {
        let mut detector = StallDetector::new(Duration::from_secs(10));
        assert_eq!(detector.observe(seconds(9)), None);
        assert_eq!(detector.observe(seconds(10)), seconds(10));
        assert_eq!(detector.observe(seconds(11)), None);
        assert_eq!(detector.observe(seconds(30)), None);
    }

    #[test]
    fn rearms_after_heartbeats_resume() {
        let mut detector = StallDetector::new(Duration::from_secs(10));
        assert_eq!(detector.observe(seconds(12)), seconds(12));
        assert_eq!(detector.observe(seconds(1)), None);
        assert_eq!(detector.observe(seconds(15)), seconds(15));
    }

    fn dump(name: &str, age: u64) -> (String, SystemTime) {
        (
            name.to_owned(),
            SystemTime::UNIX_EPOCH + Duration::from_secs(age),
        )
    }

    #[test]
    fn keeps_room_for_the_new_dump() {
        let dumps = vec![
            dump("e.dmp", 5),
            dump("a.dmp", 1),
            dump("c.dmp", 3),
            dump("b.dmp", 2),
            dump("d.dmp", 4),
        ];
        assert_eq!(dumps_to_delete(dumps, 5), vec!["a.dmp".to_owned()]);
    }

    #[test]
    fn deletes_nothing_below_the_limit() {
        let dumps = vec![dump("a.dmp", 1), dump("b.dmp", 2)];
        assert!(dumps_to_delete(dumps, 5).is_empty());
    }

    #[test]
    fn deletes_all_surplus_oldest_first() {
        let dumps = (1..=8)
            .map(|age| dump(&format!("{age}.dmp"), age))
            .collect();
        assert_eq!(
            dumps_to_delete(dumps, 5),
            vec!["1.dmp", "2.dmp", "3.dmp", "4.dmp"]
        );
    }
}
