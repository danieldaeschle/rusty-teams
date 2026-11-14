#![cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]

use std::cmp::Ordering;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
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
/// The release asset this platform installs, and the magic bytes it must start with.
#[cfg(not(target_os = "linux"))]
const ASSET_SUFFIX: &str = "-windows-amd64.exe";
#[cfg(not(target_os = "linux"))]
const EXECUTABLE_MAGIC: &[u8] = b"MZ";
#[cfg(target_os = "linux")]
const ASSET_SUFFIX: &str = "-linux-amd64";
#[cfg(target_os = "linux")]
const EXECUTABLE_MAGIC: &[u8] = b"\x7fELF";
const PARTIAL_SUFFIX: &str = ".partial";
const DOWNLOAD_CHUNK: usize = 64 * 1024;
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

/// Whether release `tag` is newer than `running_tag`.
pub fn is_newer(tag: &str, running_tag: &str) -> bool {
    matches!(
        (parse_version(tag), parse_version(running_tag)),
        (Some(tag), Some(running)) if tag > running
    )
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
                .find(|asset| asset.name.ends_with(ASSET_SUFFIX))?;
            Some((version, release, asset))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, release, asset)| (release, asset))
}

pub fn verify_download(asset: &ReleaseAsset, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 != asset.size {
        return Err(format!("size {} instead of {}", bytes.len(), asset.size));
    }
    if !bytes.starts_with(EXECUTABLE_MAGIC) {
        return Err("not an executable for this platform".to_owned());
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

fn partial_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(PARTIAL_SUFFIX);
    path.with_file_name(name)
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
    let partial = partial_path(&paths.update_exe);
    fs::write(&partial, bytes).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&partial, fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    fs::rename(&partial, &paths.update_exe).map_err(|error| error.to_string())?;
    fs::write(&paths.version_file, tag).map_err(|error| error.to_string())
}

/// A newer release this platform can install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub tag: String,
    pub asset: ReleaseAsset,
}

type HttpClient = std::sync::Arc<dyn gpui_kit::http_client::HttpClient>;

/// GETs `url`. `progress` counts the body bytes as they arrive.
async fn get(
    client: &HttpClient,
    url: &str,
    progress: Option<&AtomicU64>,
) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(url, ().into(), true)
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status()));
    }
    let mut body = Vec::new();
    let mut chunk = vec![0; DOWNLOAD_CHUNK];
    loop {
        let read = response
            .body_mut()
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Ok(body);
        }
        body.extend_from_slice(&chunk[..read]);
        if let Some(progress) = progress {
            progress.store(body.len() as u64, AtomicOrdering::Relaxed);
        }
    }
}

/// Looks for a release newer than `running_tag`, without downloading it.
pub fn find_update(
    running_tag: &'static str,
    cx: &App,
) -> Task<Result<Option<AvailableUpdate>, String>> {
    let client = cx.http_client();
    cx.background_executor().spawn(async move {
        let releases: Vec<Release> =
            serde_json::from_slice(&get(&client, RELEASES_URL, None).await?)
                .map_err(|error| error.to_string())?;
        Ok(
            newest_update(running_tag, &releases).map(|(release, asset)| AvailableUpdate {
                tag: release.tag_name.clone(),
                asset: asset.clone(),
            }),
        )
    })
}

/// Downloads and verifies `update` into the update folder, counting bytes in `progress`.
pub fn download_update(
    update: AvailableUpdate,
    paths: UpdatePaths,
    progress: std::sync::Arc<AtomicU64>,
    cx: &App,
) -> Task<Result<(), String>> {
    let client = cx.http_client();
    cx.background_executor().spawn(async move {
        if already_staged(&paths, &update.tag) {
            progress.store(update.asset.size, AtomicOrdering::Relaxed);
            return Ok(());
        }
        let bytes = get(&client, &update.asset.browser_download_url, Some(&progress)).await?;
        verify_download(&update.asset, &bytes)?;
        stage(&paths, &update.tag, &bytes)
    })
}

/// Downloads a newer GitHub release into the update folder; the regular update poll installs it.
pub fn fetch_update(
    running_tag: &'static str,
    paths: UpdatePaths,
    cx: &App,
) -> Task<Result<Option<String>, String>> {
    let find = find_update(running_tag, cx);
    let client = cx.http_client();
    cx.background_executor().spawn(async move {
        let Some(update) = find.await? else {
            return Ok(None);
        };
        if already_staged(&paths, &update.tag) {
            return Ok(None);
        }
        let bytes = get(&client, &update.asset.browser_download_url, None).await?;
        verify_download(&update.asset, &bytes)?;
        stage(&paths, &update.tag, &bytes)?;
        Ok(Some(update.tag))
    })
}

#[cfg(test)]
mod tests {

    use super::*;

    fn release(tag: &str, prerelease: bool) -> Release {
        Release {
            tag_name: tag.to_owned(),
            draft: false,
            prerelease,
            assets: vec![ReleaseAsset {
                name: format!("rusty-teams-{tag}{ASSET_SUFFIX}"),
                browser_download_url: format!("https://example.test/{tag}.exe"),
                size: EXECUTABLE_MAGIC.len() as u64,
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
        assert!(asset.name.ends_with(ASSET_SUFFIX));
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
        let bytes = EXECUTABLE_MAGIC;
        let digest: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let mut asset = release("v0.2.0", false).assets.remove(0);
        assert!(verify_download(&asset, bytes).is_err());
        asset.digest = Some(format!("{SHA256_PREFIX}{digest}"));
        assert!(verify_download(&asset, bytes).is_ok());
        assert!(verify_download(&asset, b"XY").is_err());
        assert!(verify_download(&asset, &[EXECUTABLE_MAGIC, b"!"].concat()).is_err());
    }

    #[test]
    fn newer_compares_release_versions() {
        assert!(is_newer("v0.5.0", "v0.4.0"));
        assert!(is_newer("v0.5.0", "v0.5.0-rc.1"));
        assert!(!is_newer("v0.4.0", "v0.4.0"));
        assert!(!is_newer("v0.3.9", "v0.4.0"));
        assert!(!is_newer("garbage", "v0.4.0"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_takes_the_plain_binary_not_the_tarball() {
        let mut release = release("v0.5.0", false);
        let binary = release.assets.remove(0);
        let tarball = ReleaseAsset {
            name: "rusty-teams-v0.5.0-linux-amd64.tar.gz".to_owned(),
            ..binary.clone()
        };
        release.assets = vec![tarball.clone()];
        assert!(newest_update("v0.4.0", std::slice::from_ref(&release)).is_none());
        release.assets = vec![tarball, binary];
        let (_, asset) = newest_update("v0.4.0", std::slice::from_ref(&release)).unwrap();
        assert_eq!(asset.name, "rusty-teams-v0.5.0-linux-amd64");
    }

    #[test]
    fn staging_writes_the_exe_before_the_version() {
        let directory = tempfile::tempdir().unwrap();
        let paths = UpdatePaths::beside(&directory.path().join("teams"));
        assert!(!already_staged(&paths, "v0.2.0"));
        stage(&paths, "v0.2.0", EXECUTABLE_MAGIC).unwrap();
        assert_eq!(fs::read(&paths.update_exe).unwrap(), EXECUTABLE_MAGIC);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&paths.update_exe)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        assert!(already_staged(&paths, "v0.2.0"));
        assert!(!already_staged(&paths, "v0.3.0"));
        assert!(!partial_path(&paths.update_exe).exists());
    }
}
