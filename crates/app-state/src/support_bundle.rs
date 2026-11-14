use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use app_base::support::{self, ClientInfo};
use app_call_model::report::scrub::Scrubber;
use app_call_model::report::report_directory;
use chrono::Utc;

const KEEP_CALL_REPORTS: usize = 3;
const DIAGNOSTICS_NAME: &str = "diagnostics.log";
const CRASH_NAME: &str = "crash.log";
const TIMELINE_NAME: &str = "last-call-timeline.log";
const DIAGNOSTICS_MERGE_CAP: u64 = 1024 * 1024;
const CRASH_MERGE_CAP: u64 = 128 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportBundle {
    pub directory: PathBuf,
    pub folder_name: String,
}

pub fn write_support_bundle(
    data_dir: &Path,
    client: &ClientInfo,
    _diagnostics_path: &Path,
) -> io::Result<SupportBundle> {
    let folder_name = support::bundle_folder_name(Utc::now());
    let directory = support::support_bundles_directory(data_dir).join(&folder_name);
    fs::create_dir_all(&directory)?;
    fs::write(directory.join("client.md"), client.markdown())?;
    fs::write(directory.join("README.md"), support::readme_text())?;
    write_scrubbed_text(
        &app_base::diagnostics::read_merged(DIAGNOSTICS_MERGE_CAP),
        &directory.join(DIAGNOSTICS_NAME),
    )?;
    write_scrubbed_text(
        &app_base::crash_log::read_merged(CRASH_MERGE_CAP),
        &directory.join(CRASH_NAME),
    )?;
    copy_recent_call_reports(&directory)?;
    copy_latest_call_timeline(&directory)?;
    Ok(SupportBundle {
        directory,
        folder_name,
    })
}

fn write_scrubbed_text(raw: &str, destination: &Path) -> io::Result<()> {
    if raw.trim().is_empty() {
        return Ok(());
    }
    let mut scrubber = Scrubber::default();
    let scrubbed = scrubber.scrub(raw);
    fs::write(destination, scrubbed)
}

fn copy_recent_call_reports(bundle: &Path) -> io::Result<()> {
    let reports = report_directory();
    if !reports.is_dir() {
        return Ok(());
    }
    let mut markdown: Vec<PathBuf> = fs::read_dir(&reports)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    markdown.sort();
    let start = markdown.len().saturating_sub(KEEP_CALL_REPORTS);
    for path in &markdown[start..] {
        let Some(name) = path.file_name() else {
            continue;
        };
        fs::copy(path, bundle.join(name))?;
        let json = path.with_extension("json");
        if json.is_file() {
            if let Some(json_name) = json.file_name() {
                fs::copy(&json, bundle.join(json_name))?;
            }
        }
    }
    Ok(())
}

fn copy_latest_call_timeline(bundle: &Path) -> io::Result<()> {
    let Some(dumps) = support::call_dumps_dir() else {
        return Ok(());
    };
    if !dumps.is_dir() {
        return Ok(());
    }
    let mut dirs: Vec<PathBuf> = fs::read_dir(&dumps)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    let Some(latest) = dirs.last() else {
        return Ok(());
    };
    let timeline = latest.join("timeline.txt");
    if !timeline.is_file() {
        return Ok(());
    }
    let raw = fs::read_to_string(timeline)?;
    write_scrubbed_text(&raw, &bundle.join(TIMELINE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_client() -> ClientInfo {
        ClientInfo {
            version: "0.1.0".into(),
            build: "test".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            session: "wayland".into(),
            ui_locale: "en".into(),
            timezone: "UTC+0".into(),
            signed_in: true,
            call_reports: true,
            debug_logging: true,
            audio_input: "Default device".into(),
            audio_output: "Default device".into(),
            camera: "none".into(),
            logs_root: "/tmp/data".into(),
            support_bundles_root: "/tmp/data/support-bundles".into(),
        }
    }

    #[test]
    fn support_bundle_scrubs_secrets_and_skips_store() {
        let root = tempdir().unwrap();
        let data = root.path().join("data");
        fs::create_dir_all(&data).unwrap();
        let diagnostics = data.join(DIAGNOSTICS_NAME);
        fs::write(
            &diagnostics,
            "ok line\nbearer eyJhbGciOiJIUzI1NiJ9.aaa.bbb\nemail user@example.com\n",
        )
        .unwrap();
        fs::write(data.join("store.db"), "secret-store").unwrap();

        // Install diagnostics path so merge can find the file we wrote under data.
        app_base::diagnostics::install(diagnostics.clone());
        app_base::diagnostics::set_enabled(true);
        // Re-write through the live path if install used a different OnceLock path already.
        if let Some(live) = app_base::diagnostics::path() {
            let _ = fs::write(
                &live,
                "ok line\nbearer eyJhbGciOiJIUzI1NiJ9.aaa.bbb\nemail user@example.com\n",
            );
        }

        let bundle = write_support_bundle(&data, &sample_client(), &diagnostics).unwrap();
        let client = fs::read_to_string(bundle.directory.join("client.md")).unwrap();
        assert!(client.contains("### Client"));
        assert!(client.contains("Debug logging:"));
        let log_path = bundle.directory.join(DIAGNOSTICS_NAME);
        if log_path.is_file() {
            let log = fs::read_to_string(&log_path).unwrap();
            assert!(!log.contains("eyJhbGciOiJIUzI1NiJ9"));
            assert!(!log.contains("user@example.com"));
        }
        assert!(!bundle.directory.join("store.db").exists());
        assert!(bundle.directory.join("README.md").is_file());
    }

    #[test]
    fn support_bundle_copies_matching_call_report_json() {
        let root = tempdir().unwrap();
        let data = root.path().join("data");
        fs::create_dir_all(&data).unwrap();
        // Dependency `report_directory()` is the live path (cfg(test) only applies in
        // app-call-model's own tests). Use a stem that sorts last so KEEP_CALL_REPORTS
        // still includes it among any pre-existing reports.
        let target = report_directory();
        fs::create_dir_all(&target).unwrap();
        let stem = format!(
            "zzz-{}-bundle-test",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ")
        );
        let md_name = format!("{stem}.md");
        let json_name = format!("{stem}.json");
        fs::write(target.join(&md_name), "# report\n").unwrap();
        fs::write(target.join(&json_name), "{\"schema\":1}\n").unwrap();

        let diagnostics = data.join(DIAGNOSTICS_NAME);
        fs::write(&diagnostics, "line\n").unwrap();
        let bundle = write_support_bundle(&data, &sample_client(), &diagnostics).unwrap();
        assert!(
            bundle.directory.join(&md_name).is_file(),
            "missing {md_name} in {:?}",
            bundle.directory
        );
        assert!(bundle.directory.join(&json_name).is_file());
        let _ = fs::remove_file(target.join(&md_name));
        let _ = fs::remove_file(target.join(&json_name));
    }
}
