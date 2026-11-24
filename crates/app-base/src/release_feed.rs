#![cfg_attr(
    not(any(windows, target_os = "linux", target_os = "macos")),
    allow(dead_code)
)]

use std::cmp::Ordering;
use std::time::Duration;

use futures_util::AsyncReadExt as _;
use gpui_kit::{App, Task};
use serde::Deserialize;

pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const RELEASES_URL: &str =
    "https://api.github.com/repos/danieldaeschle/rusty-teams/releases?per_page=10";
const RELEASE_PAGE: &str = "https://github.com/danieldaeschle/rusty-teams/releases/tag/";
const DOWNLOAD_CHUNK: usize = 64 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
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

/// The browser page for one release of this product line.
pub fn release_page_url(tag: &str) -> String {
    format!("{RELEASE_PAGE}{tag}")
}

/// Pre-releases are offered only to a build that runs a pre-release itself.
/// A newer tag is enough; the page lists the files.
pub fn newest_update<'a>(running_tag: &str, releases: &'a [Release]) -> Option<&'a Release> {
    let running = parse_version(running_tag)?;
    let follows_pre_releases = !running.pre_release.is_empty();
    releases
        .iter()
        .filter(|release| !release.draft && (follows_pre_releases || !release.prerelease))
        .filter_map(|release| {
            let version = parse_version(&release.tag_name).filter(|version| *version > running)?;
            Some((version, release))
        })
        .max_by(|left, right| left.0.cmp(&right.0))
        .map(|(_, release)| release)
}

/// A newer release to tell the user about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub tag: String,
}

type HttpClient = std::sync::Arc<dyn gpui_kit::http_client::HttpClient>;

async fn get(client: &HttpClient, url: &str) -> Result<Vec<u8>, String> {
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
    }
}

/// Looks for a release newer than `running_tag`, without downloading it.
pub fn find_update(
    running_tag: &'static str,
    cx: &App,
) -> Task<Result<Option<AvailableUpdate>, String>> {
    let client = cx.http_client();
    cx.background_executor().spawn(async move {
        let releases: Vec<Release> = serde_json::from_slice(&get(&client, RELEASES_URL).await?)
            .map_err(|error| error.to_string())?;
        Ok(
            newest_update(running_tag, &releases).map(|release| AvailableUpdate {
                tag: release.tag_name.clone(),
            }),
        )
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
        }
    }

    #[test]
    fn release_page_uses_this_repository() {
        assert_eq!(
            release_page_url("v0.5.0"),
            "https://github.com/danieldaeschle/rusty-teams/releases/tag/v0.5.0"
        );
        assert!(RELEASES_URL.contains("danieldaeschle/rusty-teams"));
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
        assert_eq!(
            newest_update("v0.1.0", &releases).unwrap().tag_name,
            "v0.2.0"
        );
        assert!(newest_update("v0.2.0", &releases).is_none());
        assert_eq!(
            newest_update("v0.2.0-rc.1", &releases).unwrap().tag_name,
            "v0.3.0-rc.1"
        );
    }

    #[test]
    fn a_release_without_assets_still_counts_and_a_dev_build_gets_nothing() {
        assert_eq!(
            newest_update("v0.1.0", &[release("v0.2.0", false)])
                .unwrap()
                .tag_name,
            "v0.2.0"
        );
        assert!(newest_update("0356d1d-20261107T101752Z", &[release("v0.2.0", false)]).is_none());
    }

    #[test]
    fn newer_compares_release_versions() {
        assert!(is_newer("v0.5.0", "v0.4.0"));
        assert!(is_newer("v0.5.0", "v0.5.0-rc.1"));
        assert!(!is_newer("v0.4.0", "v0.4.0"));
        assert!(!is_newer("v0.3.9", "v0.4.0"));
        assert!(!is_newer("garbage", "v0.4.0"));
    }
}
