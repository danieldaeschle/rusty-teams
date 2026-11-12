#![cfg_attr(not(windows), allow(dead_code))]

use std::cmp::Ordering;
use std::fs;
use std::time::Duration;

use futures_util::AsyncReadExt as _;
use gpui_kit::{App, Task};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::updater::UpdatePaths;

pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const RELEASES_URL: &str =
    "https://api.github.com/repos/danieldaeschle/rusty-teams/releases?per_page=10";
const WINDOWS_ASSET_SUFFIX: &str = "-windows-amd64.exe";
const PARTIAL_SUFFIX: &str = ".partial";
const SHA256_PREFIX: &str = "sha256:";

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReleaseVersion {
    core: [u64; 3],
    pre_release: Vec<String>,
}

fn parse_version(tag: &str) -> Option<ReleaseVersion> {
    let text = tag.trim().strip_prefix('v')?;
    let (core_text, pre_text) = match text.split_once('-') {
        Some((core_text, pre_text)) => (core_text, Some(pre_text)),
        None => (text, None),
    };
    let mut parts = core_text.split('.').map(|part| part.parse::<u64>().ok());
    let core = [parts.next()??, parts.next()??, parts.next()??];
    if parts.next().is_some() {
        return None;
    }
    let pre_release = pre_text
        .map(|pre_text| pre_text.split('.').map(str::to_owned).collect())
        .unwrap_or_default();
    Some(ReleaseVersion { core, pre_release })
}

fn compare_identifiers(left: &str, right: &str) -> Ordering {
    match (left.parse::<u64>(), right.parse::<u64>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => left.cmp(right),
    }
}

impl Ord for ReleaseVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| {
            match (self.pre_release.is_empty(), other.pre_release.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self
                    .pre_release
                    .iter()
                    .zip(&other.pre_release)
                    .map(|(left, right)| compare_identifiers(left, right))
                    .find(|ordering| ordering.is_ne())
                    .unwrap_or_else(|| self.pre_release.len().cmp(&other.pre_release.len())),
            }
        })
    }
}

impl PartialOrd for ReleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Pre-releases are offered only to a build that runs a pre-release itself.
pub fn newest_update<'a>(
    running_tag: &str,
    releases: &'a [Release],
) -> Option<(&'a Release, &'a ReleaseAsset)> {
    let running = parse_version(running_tag)?;
    let follows_pre_releases = !running.pre_release.is_empty();
    releases
        .iter()
        .filter(|release| !release.draft && (follows_pre_releases || !release.prerelease))
        .filter_map(|release| {
            let version = parse_version(&release.tag_name).filter(|version| *version > running)?;
            let asset = release
                .assets
                .iter()
                .find(|asset| asset.name.ends_with(WINDOWS_ASSET_SUFFIX))?;
            Some((version, release, asset))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, release, asset)| (release, asset))
}

pub fn verify_download(asset: &ReleaseAsset, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 != asset.size {
        return Err(format!("size {} instead of {}", bytes.len(), asset.size));
    }
    if !bytes.starts_with(b"MZ") {
        return Err("not a Windows executable".to_owned());
    }
    let Some(expected) = asset
        .digest
        .as_deref()
        .and_then(|digest| digest.strip_prefix(SHA256_PREFIX))
    else {
        return Err("release asset has no sha256 digest".to_owned());
    };
    let actual: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if !actual.eq_ignore_ascii_case(expected) {
        return Err("sha256 mismatch".to_owned());
    }
    Ok(())
}

fn already_staged(paths: &UpdatePaths, tag: &str) -> bool {
    paths.update_exe.exists()
        && fs::read_to_string(&paths.version_file).is_ok_and(|staged| staged.trim() == tag)
}

fn stage(paths: &UpdatePaths, tag: &str, bytes: &[u8]) -> Result<(), String> {
    let directory = paths
        .update_exe
        .parent()
        .ok_or_else(|| "no update directory".to_owned())?;
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let _ = fs::remove_file(&paths.version_file);
    let partial = paths
        .update_exe
        .with_extension(format!("exe{PARTIAL_SUFFIX}"));
    fs::write(&partial, bytes).map_err(|error| error.to_string())?;
    fs::rename(&partial, &paths.update_exe).map_err(|error| error.to_string())?;
    fs::write(&paths.version_file, tag).map_err(|error| error.to_string())
}

/// Downloads a newer GitHub release into the update folder; the regular update poll installs it.
pub fn fetch_update(
    running_tag: &'static str,
    paths: UpdatePaths,
    cx: &App,
) -> Task<Result<Option<String>, String>> {
    let client = cx.http_client();
    cx.background_executor().spawn(async move {
        let get = |url: String| {
            let client = client.clone();
            async move {
                let mut response = client
                    .get(&url, ().into(), true)
                    .await
                    .map_err(|error| error.to_string())?;
                let mut body = Vec::new();
                response
                    .body_mut()
                    .read_to_end(&mut body)
                    .await
                    .map_err(|error| error.to_string())?;
                if !response.status().is_success() {
                    return Err(format!("status {}", response.status()));
                }
                Ok::<_, String>(body)
            }
        };
        let releases: Vec<Release> = serde_json::from_slice(&get(RELEASES_URL.to_owned()).await?)
            .map_err(|error| error.to_string())?;
        let Some((release, asset)) = newest_update(running_tag, &releases) else {
            return Ok(None);
        };
        if already_staged(&paths, &release.tag_name) {
            return Ok(None);
        }
        let bytes = get(asset.browser_download_url.clone()).await?;
        verify_download(asset, &bytes)?;
        stage(&paths, &release.tag_name, &bytes)?;
        Ok(Some(release.tag_name.clone()))
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn release(tag: &str, prerelease: bool) -> Release {
        Release {
            tag_name: tag.to_owned(),
            draft: false,
            prerelease,
            assets: vec![ReleaseAsset {
                name: format!("rusty-teams-{tag}{WINDOWS_ASSET_SUFFIX}"),
                browser_download_url: format!("https://example.test/{tag}.exe"),
                size: 2,
                digest: None,
            }],
        }
    }

    #[test]
    fn versions_order_like_semver() {
        let version = |tag| parse_version(tag).unwrap();
        assert!(version("v0.1.0") > version("v0.1.0-rc.2"));
        assert!(version("v0.1.0-rc.10") > version("v0.1.0-rc.2"));
        assert!(version("v0.2.0") > version("v0.1.9"));
        assert!(version("v1.0.0-rc.1") > version("v0.9.0"));
        assert_eq!(parse_version("0.1.0"), None);
        assert_eq!(parse_version("v0.1"), None);
    }

    #[test]
    fn a_stable_build_takes_the_newest_stable_release_only() {
        let releases = [
            release("v0.3.0-rc.1", true),
            release("v0.2.0", false),
            release("v0.1.0", false),
        ];
        let (found, asset) = newest_update("v0.1.0", &releases).unwrap();
        assert_eq!(found.tag_name, "v0.2.0");
        assert!(asset.name.ends_with(WINDOWS_ASSET_SUFFIX));
        assert!(newest_update("v0.2.0", &releases).is_none());
        let (found, _) = newest_update("v0.2.0-rc.1", &releases).unwrap();
        assert_eq!(found.tag_name, "v0.3.0-rc.1");
    }

    #[test]
    fn a_release_without_a_windows_exe_or_a_dev_build_gets_nothing() {
        let mut bare = release("v0.2.0", false);
        bare.assets.clear();
        assert!(newest_update("v0.1.0", &[bare]).is_none());
        assert!(newest_update("0356d1d-20261107T101752Z", &[release("v0.2.0", false)]).is_none());
    }

    #[test]
    fn a_download_must_match_size_header_and_digest() {
        let bytes = b"MZ";
        let digest: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let mut asset = release("v0.2.0", false).assets.remove(0);
        assert!(verify_download(&asset, bytes).is_err());
        asset.digest = Some(format!("{SHA256_PREFIX}{digest}"));
        assert!(verify_download(&asset, bytes).is_ok());
        assert!(verify_download(&asset, b"XY").is_err());
        assert!(verify_download(&asset, b"MZ!").is_err());
    }

    #[test]
    fn staging_writes_the_exe_before_the_version() {
        let directory = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::beside(&directory.path().join("teams.exe"));
        assert!(!already_staged(&paths, "v0.2.0"));
        stage(&paths, "v0.2.0", b"MZ").unwrap();
        assert_eq!(fs::read(&paths.update_exe).unwrap(), b"MZ");
        assert!(already_staged(&paths, "v0.2.0"));
        assert!(!already_staged(&paths, "v0.3.0"));
        assert!(!Path::new(&paths.update_exe.with_extension("exe.partial")).exists());
    }
}
