//! Desktop integration on Linux: the install folder
//! (`~/.local/share/applications/teams`), the `.desktop` entry and the Teams icon,
//! which is downloaded from Microsoft instead of shipped.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use futures_util::AsyncReadExt as _;
use gpui_kit::{App, Task};

/// Matches the window's app id, so KDE pairs the window with the entry.
pub const DESKTOP_ID: &str = "rusty-teams";
/// Name the `.desktop` entry, the tray and `hicolor` use for the icon.
pub const ICON_NAME: &str = "rusty-teams";
pub const BINARY_FILE_NAME: &str = "teams";
/// Same icon the Teams web page shows in its tab.
pub const TEAMS_ICON_URL: &str = "https://teams.public.onecdn.static.microsoft/evergreen-assets/icons/microsoft_teams_logo_refresh_v2025.ico";
/// Fallback when [`TEAMS_ICON_URL`] moves: the page names its current favicon.
pub const TEAMS_PAGE_URL: &str = "https://teams.cloud.microsoft/";
pub const ICON_SIZES: [u32; 5] = [256, 128, 64, 48, 32];
const INSTALL_FOLDER: &str = "teams";
const DONT_ASK_META_KEY: &str = "linux.install.dont_ask";
const FAVICON_ID: &str = "id=\"teams-favicon\"";

/// Cargo marks its target folder with `CACHEDIR.TAG`; a binary in
/// `target/<profile>/` is build output and is copied, not moved, on install.
pub fn is_build_output(executable: &Path) -> bool {
    executable
        .parent()
        .and_then(Path::parent)
        .is_some_and(|target| target.join("CACHEDIR.TAG").is_file())
}

/// Paths below the XDG data folder (`~/.local/share`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    data_dir: PathBuf,
}

impl Layout {
    pub fn new(data_dir: impl Into<PathBuf>) -> Layout {
        Layout {
            data_dir: data_dir.into(),
        }
    }

    pub fn current() -> Option<Layout> {
        directories::BaseDirs::new().map(|base| Layout::new(base.data_dir()))
    }

    pub fn applications_dir(&self) -> PathBuf {
        self.data_dir.join("applications")
    }

    pub fn install_dir(&self) -> PathBuf {
        self.applications_dir().join(INSTALL_FOLDER)
    }

    pub fn installed_binary(&self) -> PathBuf {
        self.install_dir().join(BINARY_FILE_NAME)
    }

    pub fn desktop_file(&self) -> PathBuf {
        self.applications_dir()
            .join(format!("{DESKTOP_ID}.desktop"))
    }

    pub fn icons_dir(&self) -> PathBuf {
        self.data_dir.join("icons").join("hicolor")
    }

    pub fn icon_file(&self, size: u32) -> PathBuf {
        self.icons_dir()
            .join(format!("{size}x{size}"))
            .join("apps")
            .join(format!("{ICON_NAME}.png"))
    }

    pub fn largest_icon(&self) -> PathBuf {
        self.icon_file(ICON_SIZES[0])
    }
}

pub fn is_installed(executable: &Path, layout: &Layout) -> bool {
    let install_dir = layout.install_dir();
    let canonical = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    executable
        .parent()
        .is_some_and(|parent| canonical(parent) == canonical(&install_dir))
}

pub fn should_offer_install(executable: &Path, layout: &Layout, dont_ask: bool) -> bool {
    !dont_ask && !is_installed(executable, layout)
}

pub fn dont_ask_again(store: &store::Store) -> bool {
    store.meta(DONT_ASK_META_KEY).ok().flatten().as_deref() == Some("1")
}

pub fn set_dont_ask_again(store: &store::Store) {
    let _ = store.set_meta(DONT_ASK_META_KEY, "1");
}

/// Brings the install question back on the next start.
pub fn ask_again(store: &store::Store) {
    let _ = store.set_meta(DONT_ASK_META_KEY, "0");
}

/// Removes the menu entry and the installed binary (with a staged update). Sign-in,
/// cache and the downloaded icon stay. A running installed copy keeps running.
pub fn uninstall(layout: &Layout) -> io::Result<()> {
    let remove = |path: PathBuf| match fs::remove_file(&path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    };
    remove(layout.desktop_file())?;
    let install_dir = layout.install_dir();
    remove(layout.installed_binary())?;
    remove(install_dir.join(format!("{BINARY_FILE_NAME}.old")))?;
    if install_dir.join("update").is_dir() {
        fs::remove_dir_all(install_dir.join("update"))?;
    }
    // Only when empty: anything else in there belongs to the user.
    let _ = fs::remove_dir(&install_dir);
    refresh_desktop_caches(layout);
    Ok(())
}

/// Puts the binary into the install folder (copied when it is build output, moved
/// otherwise), writes the `.desktop` entry and returns the new path. An older
/// installed binary is replaced.
pub fn install_binary(executable: &Path, layout: &Layout) -> io::Result<PathBuf> {
    let target = layout.installed_binary();
    fs::create_dir_all(layout.install_dir())?;
    if fs::canonicalize(executable).ok() != fs::canonicalize(&target).ok() {
        if is_build_output(executable) {
            copy_file(executable, &target)?;
        } else {
            move_file(executable, &target)?;
        }
    }
    make_executable(&target)?;
    write_desktop_entry(layout, &target)?;
    refresh_desktop_caches(layout);
    Ok(target)
}

/// `rename`, or copy and delete when the source is on another file system.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_file(from, to)?;
    fs::remove_file(from)
}

/// Copies through a temporary file and renames it into place, so a running older
/// binary at `to` is replaced instead of written over.
fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    let partial = to.with_extension("partial");
    fs::copy(from, &partial)?;
    fs::rename(&partial, to).inspect_err(|_| {
        let _ = fs::remove_file(&partial);
    })
}

fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions)
}

/// Quotes a path for the `Exec` key of a desktop entry.
fn exec_quoted(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for character in path.display().to_string().chars() {
        if matches!(character, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

pub fn desktop_entry(executable: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         GenericName=Microsoft Teams client\n\
         Comment=A fast, native Microsoft Teams chat client\n\
         Exec={exec}\n\
         TryExec={try_exec}\n\
         Icon={ICON_NAME}\n\
         StartupWMClass={DESKTOP_ID}\n\
         Terminal=false\n\
         Categories=Network;InstantMessaging;Chat;\n\
         Keywords=Teams;Chat;Microsoft;\n",
        name = crate::APP_NAME,
        exec = exec_quoted(executable),
        try_exec = executable.display(),
    )
}

pub fn write_desktop_entry(layout: &Layout, executable: &Path) -> io::Result<()> {
    fs::create_dir_all(layout.applications_dir())?;
    fs::write(layout.desktop_file(), desktop_entry(executable))
}

/// Lets the menu and task manager pick up a new entry or icon right away. Every tool
/// is optional; whatever is missing is skipped.
pub fn refresh_desktop_caches(layout: &Layout) {
    let applications = layout.applications_dir();
    let icons = layout.icons_dir();
    std::thread::spawn(move || {
        let run = |program: &str, arguments: &[&std::ffi::OsStr]| {
            let _ = std::process::Command::new(program)
                .args(arguments)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        };
        run("update-desktop-database", &[applications.as_os_str()]);
        run(
            "gtk-update-icon-cache",
            &["-f".as_ref(), "-t".as_ref(), icons.as_os_str()],
        );
        run("kbuildsycoca6", &[]);
        run("kbuildsycoca5", &[]);
    });
}

/// Writes the Teams icon into `hicolor` in every size of [`ICON_SIZES`].
/// `bytes` is an `.ico` or a `.png`.
pub fn install_icon(layout: &Layout, bytes: &[u8]) -> Result<(), String> {
    let decoded = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    for size in ICON_SIZES {
        let resized = decoded.resize_exact(size, size, image::imageops::FilterType::Lanczos3);
        let mut png = Vec::new();
        resized
            .write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|error| error.to_string())?;
        let path = layout.icon_file(size);
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        }
        let partial = path.with_extension("partial");
        fs::write(&partial, &png).map_err(|error| error.to_string())?;
        fs::rename(&partial, &path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn has_icon(layout: &Layout) -> bool {
    layout.largest_icon().is_file()
}

/// The `href` of `<link id="teams-favicon" ...>` in the Teams page.
pub fn favicon_url(html: &str) -> Option<String> {
    let start = html.find(FAVICON_ID)?;
    let tag_start = html[..start].rfind('<')?;
    let tag_end = start + html[start..].find('>')?;
    let tag = &html[tag_start..tag_end];
    let href = tag.find("href=\"")? + "href=\"".len();
    let length = tag[href..].find('"')?;
    let url = &tag[href..href + length];
    url.starts_with("https://").then(|| url.to_owned())
}

/// Downloads the Teams icon into `hicolor` and returns the 256 px PNG.
pub fn download_icon(layout: Layout, cx: &App) -> Task<Result<Vec<u8>, String>> {
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
                    return Err(format!("{url}: status {}", response.status()));
                }
                Ok::<_, String>(body)
            }
        };
        let bytes = match get(TEAMS_ICON_URL.to_owned()).await {
            Ok(bytes) => bytes,
            Err(first) => {
                let page = get(TEAMS_PAGE_URL.to_owned()).await?;
                let url = favicon_url(&String::from_utf8_lossy(&page))
                    .ok_or_else(|| format!("{first}; no favicon on the Teams page"))?;
                get(url).await?
            }
        };
        install_icon(&layout, &bytes)?;
        refresh_desktop_caches(&layout);
        fs::read(layout.largest_icon()).map_err(|error| error.to_string())
    })
}

static TEAMS_ICON: RwLock<Option<&'static [u8]>> = RwLock::new(None);

/// The downloaded Teams icon as PNG, once it is on disk.
pub fn teams_icon() -> Option<&'static [u8]> {
    *TEAMS_ICON
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn set_teams_icon(png: Vec<u8>) {
    if !png.starts_with(b"\x89PNG") {
        return;
    }
    let mut slot = TEAMS_ICON
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Set once or twice per run (start and after a download); the leak is bounded.
    *slot = Some(Box::leak(png.into_boxed_slice()));
}

/// Loads the icon a previous run downloaded.
pub fn load_teams_icon(layout: &Layout) {
    if let Ok(png) = fs::read(layout.largest_icon()) {
        set_teams_icon(png);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(size: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(size, size, image::Rgba([80, 90, 200, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn layout_follows_the_xdg_data_folder() {
        let layout = Layout::new("/home/a/.local/share");
        assert_eq!(
            layout.installed_binary(),
            Path::new("/home/a/.local/share/applications/teams/teams")
        );
        assert_eq!(
            layout.desktop_file(),
            Path::new("/home/a/.local/share/applications/rusty-teams.desktop")
        );
        assert_eq!(
            layout.icon_file(48),
            Path::new("/home/a/.local/share/icons/hicolor/48x48/apps/rusty-teams.png")
        );
    }

    #[test]
    fn the_entry_starts_the_binary_and_matches_the_window() {
        let entry = desktop_entry(Path::new("/home/a/my apps/te\"a$ms"));
        assert!(entry.starts_with("[Desktop Entry]\n"));
        assert!(entry.contains("Exec=\"/home/a/my apps/te\\\"a\\$ms\"\n"));
        assert!(entry.contains("Icon=rusty-teams\n"));
        assert!(entry.contains("StartupWMClass=rusty-teams\n"));
    }

    fn assert_installed(installed: &Path, layout: &Layout, content: &[u8]) {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(installed, layout.installed_binary());
        assert_eq!(fs::read(installed).unwrap(), content);
        assert_eq!(
            fs::metadata(installed).unwrap().permissions().mode() & 0o111,
            0o111
        );
        let entry = fs::read_to_string(layout.desktop_file()).unwrap();
        assert!(entry.contains(&installed.display().to_string()));
        assert!(!should_offer_install(installed, layout, false));
    }

    #[test]
    fn install_moves_a_downloaded_binary_and_writes_the_entry() {
        let home = tempfile::tempdir().unwrap();
        let downloads = home.path().join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        let binary = downloads.join("teams");
        fs::write(&binary, b"v1").unwrap();
        let layout = Layout::new(home.path().join("share"));
        assert!(should_offer_install(&binary, &layout, false));
        assert!(!should_offer_install(&binary, &layout, true));

        let installed = install_binary(&binary, &layout).unwrap();

        assert!(!binary.exists());
        assert_installed(&installed, &layout, b"v1");
        assert_eq!(install_binary(&installed, &layout).unwrap(), installed);
        assert_eq!(fs::read(&installed).unwrap(), b"v1");
    }

    #[test]
    fn install_copies_build_output_and_replaces_an_older_install() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("repo").join("target");
        fs::create_dir_all(target.join("release")).unwrap();
        fs::write(
            target.join("CACHEDIR.TAG"),
            b"Signature: 8a477f597d28d172789f06886806bc55",
        )
        .unwrap();
        let binary = target.join("release").join("teams");
        let layout = Layout::new(home.path().join("share"));
        fs::write(&binary, b"v1").unwrap();
        install_binary(&binary, &layout).unwrap();
        fs::write(&binary, b"v2").unwrap();

        let installed = install_binary(&binary, &layout).unwrap();

        assert!(is_build_output(&binary));
        assert!(binary.exists());
        assert_installed(&installed, &layout, b"v2");
        assert!(!layout.install_dir().join("teams.partial").exists());
    }

    #[test]
    fn uninstall_removes_entry_binary_and_update_but_keeps_other_files() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path().join("share"));
        let downloads = home.path().join("Downloads");
        fs::create_dir_all(&downloads).unwrap();
        fs::write(downloads.join("teams"), b"v1").unwrap();
        install_binary(&downloads.join("teams"), &layout).unwrap();
        fs::create_dir_all(layout.install_dir().join("update")).unwrap();
        fs::write(layout.install_dir().join("update").join("teams"), b"v2").unwrap();

        uninstall(&layout).unwrap();

        assert!(!layout.desktop_file().exists());
        assert!(!layout.install_dir().exists());
        uninstall(&layout).unwrap();

        fs::create_dir_all(layout.install_dir()).unwrap();
        fs::write(layout.install_dir().join("notes.txt"), b"mine").unwrap();
        uninstall(&layout).unwrap();
        assert!(layout.install_dir().join("notes.txt").exists());
    }

    #[test]
    fn the_icon_lands_in_every_size() {
        let home = tempfile::tempdir().unwrap();
        let layout = Layout::new(home.path());
        assert!(!has_icon(&layout));
        install_icon(&layout, &png(300)).unwrap();
        for size in ICON_SIZES {
            let written = image::open(layout.icon_file(size)).unwrap();
            assert_eq!((written.width(), written.height()), (size, size));
        }
        assert!(has_icon(&layout));
        assert!(install_icon(&layout, b"not an image").is_err());
    }

    #[test]
    fn the_favicon_comes_from_the_teams_page() {
        let html = r#"<head><link rel="shortcut icon" type="image/x-icon" id="teams-favicon" href="https://cdn.example/icons/teams.ico"/></head>"#;
        assert_eq!(
            favicon_url(html).as_deref(),
            Some("https://cdn.example/icons/teams.ico")
        );
        assert_eq!(favicon_url("<html></html>"), None);
        assert_eq!(
            favicon_url(r#"<link id="teams-favicon" href="http://insecure/x.ico">"#),
            None
        );
    }
}
