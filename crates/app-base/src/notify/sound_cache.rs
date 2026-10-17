use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gpui_kit::App;

use super::platform;

pub const TEAMS_SOUND_BASE_URL: &str =
    "https://statics.teams.cdn.office.net/evergreen-assets/audio/";
const SOUND_DIRECTORY: &str = "sounds";
const PARTIAL_SUFFIX: &str = ".partial";

pub fn cache_directory() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
        .map(|directories| directories.cache_dir().join(SOUND_DIRECTORY))
}

pub fn is_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3") || matches!(bytes, [0xFF, second, ..] if second & 0xE0 == 0xE0)
}

pub fn read_cached(directory: &Path, file_name: &str) -> Option<Vec<u8>> {
    fs::read(directory.join(file_name))
        .ok()
        .filter(|bytes| is_mp3(bytes))
}

pub fn write_cached(directory: &Path, file_name: &str, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let partial = directory.join(format!("{file_name}{PARTIAL_SUFFIX}"));
    fs::write(&partial, bytes)?;
    fs::rename(&partial, directory.join(file_name))
}

fn load(file_name: &'static str, cx: &App) -> gpui_kit::Task<Option<Vec<u8>>> {
    let client = cx.http_client();
    cx.background_executor().spawn(async move {
        let directory = cache_directory()?;
        if let Some(bytes) = read_cached(&directory, file_name) {
            return Some(bytes);
        }
        use futures_util::AsyncReadExt as _;
        let url = format!("{TEAMS_SOUND_BASE_URL}{file_name}");
        let mut response = client.get(&url, ().into(), true).await.ok()?;
        let mut body = Vec::new();
        response.body_mut().read_to_end(&mut body).await.ok()?;
        if !response.status().is_success() || !is_mp3(&body) {
            log::warn!(
                "teams sound {file_name} not loaded: status {}",
                response.status()
            );
            return None;
        }
        if let Err(error) = write_cached(&directory, file_name, &body) {
            log::warn!("teams sound {file_name} not cached: {error}");
        }
        Some(body)
    })
}

pub fn prefetch(file_names: &[&'static str], cx: &App) {
    for file_name in file_names {
        load(file_name, cx).detach();
    }
}

pub fn play(file_name: &'static str, cx: &App) {
    let loading = load(file_name, cx);
    cx.background_executor()
        .spawn(async move {
            if let Some(bytes) = loading.await {
                platform::play_mp3(bytes);
            }
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp3_files_start_with_a_tag_or_a_frame_sync() {
        assert!(is_mp3(b"ID3\x04\x00"));
        assert!(is_mp3(&[0xFF, 0xFB, 0x90]));
        assert!(!is_mp3(b"<?xml version"));
        assert!(!is_mp3(&[]));
    }

    #[test]
    fn a_cached_sound_round_trips_and_junk_is_ignored() {
        let directory = tempfile::tempdir().unwrap();
        let sounds = directory.path().join("sounds");
        write_cached(&sounds, "end.mp3", b"ID3 tone").unwrap();
        assert_eq!(
            read_cached(&sounds, "end.mp3").as_deref(),
            Some(&b"ID3 tone"[..])
        );
        assert!(!sounds.join("end.mp3.partial").exists());
        write_cached(&sounds, "junk.mp3", b"<Error/>").unwrap();
        assert_eq!(read_cached(&sounds, "junk.mp3"), None);
        assert_eq!(read_cached(&sounds, "missing.mp3"), None);
    }
}
