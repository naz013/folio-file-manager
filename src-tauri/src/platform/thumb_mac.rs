//! Thumbnails on macOS, from Quick Look: the same pictures Finder shows, for every kind of file
//! Quick Look knows. `qlmanage -t` writes one as a PNG; it needs no entitlement and no unsafe
//! code, and runs in the thumbnail lane, which has a time limit.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn thumbnail_png(path: &Path, size: u32) -> Result<Vec<u8>, String> {
    // The path is one argument, never a shell line; being absolute it can't pass for an option.
    if !path.is_absolute() {
        return Err("not an absolute path".into());
    }
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() && !meta.is_dir() {
        return Err("not a file or folder".into());
    }
    let out = std::env::temp_dir().join(format!(
        "folio-ql-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&out).map_err(|e| e.to_string())?;
    let made = render(path, size, &out);
    // The folder held only this one picture.
    let _ = std::fs::remove_dir_all(&out);
    made
}

fn render(path: &Path, size: u32, out: &Path) -> Result<Vec<u8>, String> {
    let mut child = Command::new("/usr/bin/qlmanage")
        .arg("-t")
        .arg("-s")
        .arg(size.to_string())
        .arg("-o")
        .arg(out)
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Quick Look could not start: {e}"))?;
    // The caller stops waiting after `slow::THUMBNAIL`, but this thread would go on waiting for
    // a hung Quick Look; it is stopped here, with some margin, so nothing is left running.
    let deadline = Instant::now() + crate::slow::THUMBNAIL + Duration::from_secs(1);
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Quick Look took too long".into());
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };
    if !status.success() {
        return Err("Quick Look failed".into());
    }
    // It names the picture after the file, plus `.png`; with no thumbnail it writes none.
    let name = format!(
        "{}.png",
        path.file_name().ok_or("no file name")?.to_string_lossy()
    );
    std::fs::read(out.join(name)).map_err(|_| "Quick Look has no thumbnail".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_is_refused() {
        assert!(thumbnail_png(Path::new("-t"), 48).is_err());
    }

    #[test]
    fn draws_a_text_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, "hello").unwrap();
        let png = thumbnail_png(&file, 48).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }
}
