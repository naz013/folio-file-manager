//! `thumb://` protocol: serves OS thumbnails as PNG to the webview.
//!
//! URL shape (built by the frontend with `convertFileSrc(path, "thumb")`):
//! `<origin>/<percent-encoded absolute path>?s=<size>`

use crate::platform;
use std::borrow::Cow;
use std::path::PathBuf;
use tauri::http::{Request, Response, StatusCode, Uri};

pub const SCHEME: &str = "thumb";
const DEFAULT_SIZE: u32 = 96;
const MAX_SIZE: u32 = 512;

pub fn handle(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    match parse(request.uri()).and_then(|(path, size)| platform::thumbnail_png(&path, size)) {
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
