//! The rule that makes a plugin's digest, shared by the app and its build script: `build.rs`
//! includes this file to fix the digest of each bundled plugin in the binary, and `plugins` uses
//! it to check what is on disk against that and against what a person approved. Only the standard
//! library and blake3, so the build script can use it.

use std::collections::HashMap;
use std::path::Path;

/// A plugin with more files than this (or more entries of any kind) is not read.
pub const MAX_FILES: usize = 1000;
/// A plugin whose served files add up to more than this is not read.
pub const MAX_TOTAL: u64 = 64 << 20;

/// What was approved for one plugin: the hash of each file it may serve, by relative path.
pub type Pin = HashMap<String, String>;

/// The digest of the plugin in `root`, and the hash of each file it was made from. Each file is
/// read once, so the two cannot disagree about what was there.
pub fn snapshot(root: &Path) -> Result<(String, Pin), String> {
    let mut files = Vec::new();
    let mut seen = 0usize;
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read = std::fs::read_dir(&dir).map_err(|_| "A folder can't be read".to_owned())?;
        for entry in read.flatten() {
            seen += 1;
            if seen > MAX_FILES * 5 {
                return Err("The plugin has too many files".to_owned());
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && mime_of(&path).is_some() {
                total += entry.metadata().map_or(0, |m| m.len());
                if files.len() >= MAX_FILES || total > MAX_TOTAL {
                    return Err("The plugin has too many or too large files".to_owned());
                }
                let rel = path.strip_prefix(root).unwrap_or(&path);
                files.push((rel.to_string_lossy().replace('\\', "/"), path));
            }
        }
    }
    files.sort();
    let mut hasher = blake3::Hasher::new();
    let mut pin = Pin::new();
    for (rel, path) in files {
        let file = std::fs::File::open(&path).map_err(|_| format!("{rel} can't be read"))?;
        let mut one = blake3::Hasher::new();
        one.update_reader(file)
            .map_err(|_| format!("{rel} can't be read"))?;
        let hash = one.finalize().to_hex().to_string();
        hasher.update(rel.as_bytes());
        hasher.update(&[0]);
        hasher.update(hash.as_bytes());
        hasher.update(&[0]);
        pin.insert(rel, hash);
    }
    Ok((hasher.finalize().to_hex().to_string(), pin))
}

/// The content type of a file a plugin may serve; other kinds are refused.
pub fn mime_of(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "txt" | "md" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        _ => return None,
    })
}
