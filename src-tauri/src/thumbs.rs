//! `thumb://` protocol: serves OS thumbnails as PNG to the webview.
//!
//! URL shape (built by the frontend with `convertFileSrc(path, "thumb")`):
//! `<origin>/<percent-encoded absolute path>?s=<size>`

use crate::{platform, slow};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;
use tauri::http::{Request, Response, StatusCode, Uri};

pub const SCHEME: &str = "thumb";
const DEFAULT_SIZE: u32 = 96;
const MAX_SIZE: u32 = 512;
/// How many bytes of finished pictures are kept, so scrolling back over a folder does not ask
/// the system to draw them all again.
const CACHE_BYTES: usize = 64 * 1024 * 1024;

/// Pictures already drawn, least recently used first to go.
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache::new(CACHE_BYTES)));

/// A picture is the same one only while the file is: a changed file has another time or length.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    size: u32,
    modified: Option<SystemTime>,
    len: u64,
}

struct Cache {
    limit: usize,
    used: usize,
    tick: u64,
    items: HashMap<Key, (Vec<u8>, u64)>,
}

impl Cache {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            used: 0,
            tick: 0,
            items: HashMap::new(),
        }
    }

    fn get(&mut self, key: &Key) -> Option<Vec<u8>> {
        self.tick += 1;
        let tick = self.tick;
        let (png, used) = self.items.get_mut(key)?;
        *used = tick;
        Some(png.clone())
    }

    fn put(&mut self, key: Key, png: Vec<u8>) {
        // One picture bigger than a fifth of the cache would push out too much for its worth.
        if png.len() > self.limit / 5 {
            return;
        }
        self.tick += 1;
        self.used += png.len();
        if let Some((old, _)) = self.items.insert(key, (png, self.tick)) {
            self.used -= old.len();
        }
        while self.used > self.limit {
            let Some(oldest) = self
                .items
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            if let Some((gone, _)) = self.items.remove(&oldest) {
                self.used -= gone.len();
            }
        }
    }
}

fn key_for(path: &Path, size: u32) -> Option<Key> {
    let meta = std::fs::metadata(path).ok()?;
    Some(Key {
        path: path.to_path_buf(),
        size,
        modified: meta.modified().ok(),
        len: meta.len(),
    })
}

/// The picture for `path`: from the cache if the file is as it was, else drawn and kept.
fn picture(path: &Path, size: u32) -> Result<Vec<u8>, String> {
    let key = key_for(path, size);
    if let Some(key) = &key {
        let hit = CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key);
        if let Some(png) = hit {
            return Ok(png);
        }
    }
    let png = platform::thumbnail_png(path, size)?;
    if let Some(key) = key {
        CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .put(key, png.clone());
    }
    Ok(png)
}

/// Answers a thumbnail request. Anything that is not a picture within the time allowed is a
/// 404, which the page turns into the plain file icon.
pub async fn handle(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    // The file is looked at inside the lane: on a share that went away that can block.
    let made = match parse(request.uri()) {
        Ok((path, size)) => slow::THUMBNAILS
            .within(slow::THUMBNAIL, move || picture(&path, size))
            .await
            .unwrap_or_else(|| Err("took too long".into())),
        Err(e) => Err(e),
    };
    match made {
        Ok(png) => Response::builder()
            .header("Content-Type", "image/png")
            .header("Cache-Control", "max-age=600")
            .header("Access-Control-Allow-Origin", "*")
            .body(png)
            .unwrap(),
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap(),
    }
}

fn parse(uri: &Uri) -> Result<(PathBuf, u32), String> {
    let raw = uri.path().trim_start_matches('/');
    let path: Cow<str> = percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .map_err(|e| e.to_string())?;
    if path.is_empty() {
        return Err("empty path".into());
    }
    let size = uri
        .query()
        .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("s=")))
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SIZE)
        .clamp(16, MAX_SIZE);
    Ok((PathBuf::from(path.into_owned()), size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_windows_path_and_size() {
        let uri: Uri = "http://thumb.localhost/C%3A%5CUsers%5Cme%5Ca%20b.png?s=128"
            .parse()
            .unwrap();
        let (path, size) = parse(&uri).unwrap();
        assert_eq!(path, PathBuf::from("C:\\Users\\me\\a b.png"));
        assert_eq!(size, 128);
    }

    /// A file the sync provider has not downloaded is marked offline; it must get no thumbnail,
    /// because making one reads the file.
    #[cfg(windows)]
    #[test]
    fn a_cloud_only_file_gets_no_thumbnail() {
        use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_OFFLINE, SetFileAttributesW};
        use windows::core::HSTRING;

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        assert!(
            platform::thumbnail_png(&file, 48).is_ok(),
            "a local file still gets one"
        );

        // Only OFFLINE can be set from user mode; the RECALL_* bits OneDrive uses are read the
        // same way and need a real placeholder to try.
        // SAFETY: the path is an owned HSTRING that outlives the call.
        unsafe { SetFileAttributesW(&HSTRING::from(file.as_os_str()), FILE_ATTRIBUTE_OFFLINE) }
            .unwrap();
        assert!(platform::thumbnail_png(&file, 48).is_err());
    }

    fn key(name: &str, len: u64) -> Key {
        Key {
            path: PathBuf::from(name),
            size: 96,
            modified: None,
            len,
        }
    }

    #[test]
    fn the_cache_drops_the_least_recently_used_picture_when_full() {
        let mut cache = Cache::new(100);
        cache.put(key("a", 1), vec![0; 15]);
        cache.put(key("b", 1), vec![0; 15]);
        cache.put(key("c", 1), vec![0; 15]);
        // Reading `a` makes `b` the oldest.
        assert!(cache.get(&key("a", 1)).is_some());
        for n in ["d", "e", "f", "g", "h"] {
            cache.put(key(n, 1), vec![0; 15]);
        }
        assert!(cache.used <= 100);
        assert!(cache.get(&key("b", 1)).is_none(), "the oldest went first");
        assert!(cache.get(&key("h", 1)).is_some());
    }

    #[test]
    fn a_changed_file_is_not_served_its_old_picture() {
        let mut cache = Cache::new(100);
        cache.put(key("a", 1), vec![1]);
        assert!(cache.get(&key("a", 2)).is_none());
        assert_eq!(cache.get(&key("a", 1)), Some(vec![1]));
    }

    #[test]
    fn a_picture_too_big_for_its_share_is_not_kept() {
        let mut cache = Cache::new(100);
        cache.put(key("a", 1), vec![0; 21]);
        assert_eq!(cache.used, 0);
    }

    /// The page adds `r=1` when it asks again after a failure; the size is still read.
    #[test]
    fn unknown_query_keys_are_ignored() {
        let uri: Uri = "thumb://localhost/%2Fx.jpg?s=96&r=1".parse().unwrap();
        assert_eq!(parse(&uri).unwrap(), (PathBuf::from("/x.jpg"), 96));
    }

    #[test]
    fn defaults_and_clamps_size() {
        let uri: Uri = "thumb://localhost/%2Fhome%2Fme%2Fx.jpg?s=9999"
            .parse()
            .unwrap();
        assert_eq!(
            parse(&uri).unwrap(),
            (PathBuf::from("/home/me/x.jpg"), MAX_SIZE)
        );
        let uri: Uri = "thumb://localhost/%2Fx".parse().unwrap();
        assert_eq!(parse(&uri).unwrap().1, DEFAULT_SIZE);
    }
}
