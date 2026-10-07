use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::error::{Error, Result};

const FNV_OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
const FNV_PRIME: u128 = 0x0000000001000000000000000000013b;
const TEMP_EXTENSION: &str = "part";

struct Entry {
    path: PathBuf,
    size: u64,
    last_used: SystemTime,
}

/// Image files in one directory, evicted least recently used first once the total exceeds the cap.
pub struct ImageFileCache {
    directory: PathBuf,
    max_total_bytes: u64,
    entries: Mutex<HashMap<String, Entry>>,
}

impl ImageFileCache {
    /// Indexes the files already in `directory`, so the cache survives restarts.
    pub fn open(directory: &Path, max_total_bytes: u64) -> Result<Self> {
        fs::create_dir_all(directory)?;
        let mut entries = HashMap::new();
        for file in fs::read_dir(directory)?.flatten() {
            let path = file.path();
            let Some(name) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            if path
                .extension()
                .is_some_and(|extension| extension == TEMP_EXTENSION)
            {
                let _ = fs::remove_file(&path);
                continue;
            }
            let Ok(metadata) = file.metadata() else {
                continue;
            };
            entries.insert(
                name,
                Entry {
                    size: metadata.len(),
                    last_used: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    path,
                },
            );
        }
        Ok(ImageFileCache {
            directory: directory.to_owned(),
            max_total_bytes,
            entries: Mutex::new(entries),
        })
    }

    /// Marks the file as used.
    pub fn path(&self, key: &str) -> Option<PathBuf> {
        let mut entries = self.entries.lock().ok()?;
        let entry = entries.get_mut(&file_stem(key))?;
        if !entry.path.exists() {
            entries.remove(&file_stem(key));
            return None;
        }
        entry.last_used = SystemTime::now();
        touch(&entry.path, entry.last_used);
        Some(entry.path.clone())
    }

    pub fn contains(&self, key: &str) -> bool {
        self.entries
            .lock()
            .is_ok_and(|entries| entries.contains_key(&file_stem(key)))
    }

    /// `extension` without the dot, e.g. `png`.
    pub fn put(&self, key: &str, bytes: &[u8], extension: &str) -> Result<PathBuf> {
        let stem = file_stem(key);
        let path = self.directory.join(format!("{stem}.{extension}"));
        let temporary = self.directory.join(format!("{stem}.{TEMP_EXTENSION}"));
        fs::write(&temporary, bytes)?;
        fs::rename(&temporary, &path)?;
        let mut entries = self.entries.lock().map_err(|_| Error::Poisoned)?;
        if let Some(previous) = entries.get(&stem).filter(|entry| entry.path != path) {
            let _ = fs::remove_file(&previous.path);
        }
        entries.insert(
            stem.clone(),
            Entry {
                path: path.clone(),
                size: bytes.len() as u64,
                last_used: SystemTime::now(),
            },
        );
        self.evict(&mut entries, &stem);
        Ok(path)
    }

    pub fn total_bytes(&self) -> u64 {
        self.entries
            .lock()
            .map_or(0, |entries| entries.values().map(|entry| entry.size).sum())
    }

    fn evict(&self, entries: &mut HashMap<String, Entry>, keep: &str) {
        let mut total: u64 = entries.values().map(|entry| entry.size).sum();
        if total <= self.max_total_bytes {
            return;
        }
        let mut oldest: Vec<(String, SystemTime, u64)> = entries
            .iter()
            .filter(|(stem, _)| stem.as_str() != keep)
            .map(|(stem, entry)| (stem.clone(), entry.last_used, entry.size))
            .collect();
        oldest.sort_by_key(|(stem, last_used, _)| (*last_used, stem.clone()));
        for (stem, _, size) in oldest {
            if total <= self.max_total_bytes {
                break;
            }
            if let Some(entry) = entries.remove(&stem) {
                let _ = fs::remove_file(entry.path);
            }
            total -= size;
        }
    }
}

fn touch(path: &Path, time: SystemTime) {
    if let Ok(file) = fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(time);
    }
}

fn file_stem(key: &str) -> String {
    let hash = key.bytes().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u128::from(byte)).wrapping_mul(FNV_PRIME)
    });
    format!("{hash:032x}")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn put_then_path_returns_the_same_file() {
        let directory = tempfile::tempdir().unwrap();
        let cache = ImageFileCache::open(directory.path(), 1000).unwrap();
        assert!(cache.path("k").is_none());
        let path = cache.put("k", b"abc", "png").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"abc");
        assert_eq!(cache.path("k"), Some(path));
        assert!(cache.contains("k"));
    }

    #[test]
    fn least_recently_used_files_go_first() {
        let directory = tempfile::tempdir().unwrap();
        let cache = ImageFileCache::open(directory.path(), 25).unwrap();
        let first = cache.put("a", &[0; 10], "png").unwrap();
        std::thread::sleep(Duration::from_millis(5));
        cache.put("b", &[0; 10], "png").unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert!(cache.path("a").is_some());
        std::thread::sleep(Duration::from_millis(5));
        cache.put("c", &[0; 10], "png").unwrap();
        assert!(cache.contains("a") && cache.contains("c"));
        assert!(!cache.contains("b"));
        assert!(first.exists());
        assert_eq!(cache.total_bytes(), 20);
    }

    #[test]
    fn an_oversized_new_file_is_kept_and_everything_else_evicted() {
        let directory = tempfile::tempdir().unwrap();
        let cache = ImageFileCache::open(directory.path(), 10).unwrap();
        cache.put("a", &[0; 5], "png").unwrap();
        cache.put("big", &[0; 50], "png").unwrap();
        assert!(cache.contains("big") && !cache.contains("a"));
    }

    #[test]
    fn reopening_indexes_existing_files_and_drops_partial_writes() {
        let directory = tempfile::tempdir().unwrap();
        let cache = ImageFileCache::open(directory.path(), 100).unwrap();
        let path = cache.put("k", b"abc", "jpg").unwrap();
        fs::write(directory.path().join("x.part"), b"zz").unwrap();
        drop(cache);
        let reopened = ImageFileCache::open(directory.path(), 100).unwrap();
        assert_eq!(reopened.path("k"), Some(path));
        assert_eq!(reopened.total_bytes(), 3);
        assert!(!directory.path().join("x.part").exists());
    }

    #[test]
    fn replacing_a_key_with_another_extension_removes_the_old_file() {
        let directory = tempfile::tempdir().unwrap();
        let cache = ImageFileCache::open(directory.path(), 100).unwrap();
        let old = cache.put("k", b"a", "png").unwrap();
        let new = cache.put("k", b"b", "jpg").unwrap();
        assert!(!old.exists() && new.exists());
    }
}
