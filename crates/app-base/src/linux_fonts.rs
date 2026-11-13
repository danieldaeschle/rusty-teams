//! Hide a system Noto Color Emoji face before GPUI loads fonts.
//!
//! Cosmic Text on Linux falls back to the family "Noto Color Emoji", and
//! fontdb returns the first face of that name. A COLRv1-only face draws
//! blank. The app bundles a CBDT build of the same name and loads it with
//! `add_fonts`. fontdb only honors `<dir>` entries from `FONTCONFIG_FILE`,
//! so this module rewrites those directories and leaves the system color
//! emoji file out. An existing `FONTCONFIG_FILE` is left alone.
//!
//! This `FONTCONFIG_FILE` is for the app process only. Chrome must not
//! inherit the app-generated `fontconfig-emoji` conf (`browser::launch`
//! strips that path on spawn) — the dirs-only file breaks Segoe/Arial
//! fallback on the Microsoft login page. A user-provided
//! `FONTCONFIG_FILE` is left alone for both the app and Chrome.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const FONT_EXTENSIONS: &[&str] = &["ttf", "ttc", "otf", "otc"];

/// A system color-emoji file that would win the "Noto Color Emoji" lookup.
pub fn is_system_color_emoji_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = lower
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(lower.as_str());
    stem == "notocoloremoji"
        || stem.starts_with("notocoloremoji-")
        || (stem.contains("noto") && stem.contains("colrv1"))
}

/// Point `FONTCONFIG_FILE` at a font list that omits the system color emoji face.
pub fn hide_system_color_emoji() {
    if std::env::var_os("FONTCONFIG_FILE").is_some() {
        log::info!("FONTCONFIG_FILE is already set, leaving the system emoji font alone");
        return;
    }
    let Some(cache) = directories::ProjectDirs::from("", "", store::DATA_DIR_NAME) else {
        return;
    };
    let staging = cache.cache_dir().join("fontconfig-emoji");
    let _ = fs::remove_dir_all(&staging);
    if fs::create_dir_all(&staging).is_err() {
        return;
    }
    let dirs: Vec<PathBuf> = font_roots()
        .into_iter()
        .enumerate()
        .filter_map(|(index, root)| publish_root(&root, &staging, index))
        .collect();
    if dirs.is_empty() {
        return;
    }
    let config = staging.join("fonts.conf");
    if write_config(&config, &dirs).is_err() {
        return;
    }
    unsafe { std::env::set_var("FONTCONFIG_FILE", &config) };
    log::info!("emoji fontconfig {}", config.display());
}

fn font_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let data_dirs =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    for dir in data_dirs.split(':').filter(|dir| !dir.is_empty()) {
        roots.push(PathBuf::from(dir).join("fonts"));
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        roots.push(PathBuf::from(xdg).join("fonts"));
    } else if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/fonts"));
    }
    if let Ok(home) = std::env::var("HOME") {
        roots.push(PathBuf::from(home).join(".fonts"));
    }
    roots.retain(|path| path.is_dir());
    roots.sort();
    roots.dedup();
    roots
}

/// Use `root` as-is when it has no color emoji file. Otherwise mirror it and skip that file.
pub(crate) fn publish_root(root: &Path, staging: &Path, index: usize) -> Option<PathBuf> {
    if !contains_emoji(root) {
        return Some(root.to_path_buf());
    }
    let dest = staging.join(format!("root-{index}"));
    let mut seen = HashSet::new();
    mirror(root, &dest, &mut seen).then_some(dest)
}

fn contains_emoji(dir: &Path) -> bool {
    fn walk(dir: &Path, seen: &mut HashSet<PathBuf>) -> bool {
        let canon = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        if !seen.insert(canon) {
            return false;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                if walk(&path, seen) {
                    return true;
                }
            } else if is_system_color_emoji_file(&entry.file_name().to_string_lossy()) {
                return true;
            }
        }
        false
    }
    walk(dir, &mut HashSet::new())
}

fn mirror(src: &Path, dest: &Path, seen: &mut HashSet<PathBuf>) -> bool {
    let canon = fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    if !seen.insert(canon) {
        return false;
    }
    let Ok(entries) = fs::read_dir(src) else {
        return false;
    };
    let mut wrote = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if contains_emoji(&path) {
                let child = dest.join(&name);
                let _ = fs::create_dir_all(&child);
                wrote |= mirror(&path, &child, seen);
            } else {
                let _ = fs::create_dir_all(dest);
                wrote |= symlink_to(&path, &dest.join(&name)).is_ok();
            }
        } else if is_font_file(&name.to_string_lossy())
            && !is_system_color_emoji_file(&name.to_string_lossy())
        {
            let _ = fs::create_dir_all(dest);
            wrote |= symlink_to(&path, &dest.join(&name)).is_ok();
        }
    }
    wrote
}

fn is_font_file(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        FONT_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    })
}

fn symlink_to(target: &Path, link: &Path) -> std::io::Result<()> {
    let target = fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    std::os::unix::fs::symlink(target, link)
}

fn write_config(path: &Path, dirs: &[PathBuf]) -> std::io::Result<()> {
    let mut body = String::from("<?xml version=\"1.0\"?>\n<fontconfig>\n");
    for dir in dirs {
        body.push_str("  <dir>");
        body.push_str(&xml_escape(&dir.to_string_lossy()));
        body.push_str("</dir>\n");
    }
    body.push_str("</fontconfig>\n");
    fs::write(path, body)
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_emoji_file_names() {
        assert!(is_system_color_emoji_file("NotoColorEmoji.ttf"));
        assert!(is_system_color_emoji_file("Noto-COLRv1.ttf"));
        assert!(is_system_color_emoji_file("NotoColorEmoji-emojicompat.ttf"));
        assert!(!is_system_color_emoji_file("NotoSans-Regular.ttf"));
        assert!(!is_system_color_emoji_file("NotoEmoji-Regular.ttf"));
    }

    #[test]
    fn publish_root_drops_the_color_emoji_file_and_keeps_the_rest() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("fonts");
        let noto = root.join("noto");
        let other = root.join("liberation");
        fs::create_dir_all(&noto).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(noto.join("NotoColorEmoji.ttf"), b"emoji").unwrap();
        fs::write(noto.join("NotoSans-Regular.ttf"), b"sans").unwrap();
        fs::write(other.join("LiberationSans.ttf"), b"serif").unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        let published = publish_root(&root, &staging, 0).unwrap();
        assert!(published.starts_with(&staging));
        assert!(!published.join("noto/NotoColorEmoji.ttf").exists());
        assert_eq!(
            fs::read(published.join("noto/NotoSans-Regular.ttf")).unwrap(),
            b"sans"
        );
        assert_eq!(
            fs::read(published.join("liberation/LiberationSans.ttf")).unwrap(),
            b"serif"
        );
    }

    #[test]
    fn publish_root_keeps_a_tree_without_a_color_emoji_file() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("fonts");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("DejaVuSans.ttf"), b"dejavu").unwrap();
        let published = publish_root(&root, &temp.path().join("staging"), 0).unwrap();
        assert_eq!(published, root);
    }
}
