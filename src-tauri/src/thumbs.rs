//! `thumb://` protocol: serves OS thumbnails as PNG to the webview.
//!
//! URL shape (built by the frontend with `convertFileSrc(path, "thumb")`):
//! `<origin>/<percent-encoded absolute path>?s=<size>`

use crate::{platform, slow};
use std::borrow::Cow;
use std::path::PathBuf;
use tauri::http::{Request, Response, StatusCode, Uri};

pub const SCHEME: &str = "thumb";
const DEFAULT_SIZE: u32 = 96;
const MAX_SIZE: u32 = 512;

/// Answers a thumbnail request. Anything that is not a picture within the time allowed is a
/// 404, which the page turns into the plain file icon.
pub async fn handle(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let made = match parse(request.uri()) {
        Ok((path, size)) => slow::THUMBNAILS
            .within(slow::THUMBNAIL, move || {
                platform::thumbnail_png(&path, size)
            })
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
