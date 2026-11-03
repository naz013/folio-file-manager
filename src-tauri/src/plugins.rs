//! Plugins on disk (ADR 0005): finding them, and serving their files to their sandboxed pages.
//!
//! A plugin is a folder under the app data directory, named by its id, with a `plugin.json`.
//! This module only reads: it lists the manifests (the page checks and understands them) and
//! answers `plugin://` requests with files from inside one plugin's folder.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use tauri::http::{Request, Response, StatusCode};

pub const SCHEME: &str = "plugin";

/// The manifest's name inside a plugin folder.
const MANIFEST: &str = "plugin.json";
/// A manifest larger than this is not read.
const MAX_MANIFEST: u64 = 64 << 10;
/// A plugin file larger than this is not served.
const MAX_FILE: u64 = 5 << 20;

/// What the plugin page may load: its own files, never the network. Inline script and style are
/// allowed so a plugin can be one HTML file.
const CSP: &str = "default-src 'none'; script-src 'self' 'unsafe-inline'; \
    style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; \
    connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'";

/// The folder plugins live in; managed state, `None` when the system has no app data folder.
pub struct PluginsDir(pub Option<PathBuf>);

/// One folder found under the plugins directory.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Found {
    /// The folder's name, which is the plugin's id.
    pub id: String,
    /// The manifest as written; the page checks it.
    pub manifest: Option<String>,
    /// Why there is no manifest to read.
    pub error: Option<String>,
}

/// An id is a short name of lower-case letters, digits and hyphens, not starting or ending in one.
pub fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

/// The plugins under `dir`, in id order. A folder with a bad name or no readable manifest is
/// listed with the reason, so a person can see why it isn't offered.
pub fn list(dir: &Path) -> Vec<Found> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<Found> = read
        .flatten()
        // Links are not followed: a link could lead out of the folder.
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
        .map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            if !valid_id(&id) {
                return Found {
                    id,
                    manifest: None,
                    error: Some(
                        "The folder's name must be 1 to 40 letters (a-z), digits or hyphens"
                            .to_owned(),
                    ),
                };
            }
            match read_manifest(&e.path().join(MANIFEST)) {
                Ok(text) => Found {
                    id,
                    manifest: Some(text),
                    error: None,
                },
                Err(why) => Found {
                    id,
                    manifest: None,
                    error: Some(why),
                },
            }
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

fn read_manifest(file: &Path) -> Result<String, String> {
    let meta = std::fs::symlink_metadata(file).map_err(|_| format!("No {MANIFEST}"))?;
    if !meta.is_file() {
        return Err(format!("{MANIFEST} is not a file"));
    }
    if meta.len() > MAX_MANIFEST {
        return Err(format!("{MANIFEST} is too large"));
    }
    std::fs::read_to_string(file).map_err(|_| format!("{MANIFEST} can't be read as text"))
}

/// The file `rel` of plugin `id`, if it is a plain file inside that plugin's folder. `rel` is a
/// forward-slash path made only of ordinary names.
pub fn resolve(dir: &Path, id: &str, rel: &str) -> Option<PathBuf> {
    if !valid_id(id) || rel.is_empty() || rel.contains(['\\', ':']) {
        return None;
    }
    let mut path = dir.join(id);
    for part in Path::new(rel).components() {
        match part {
            Component::Normal(name) => path.push(name),
            _ => return None,
        }
    }
    // Resolved on disk: a link inside the folder that leads outside it ends up elsewhere.
    let home = dir.join(id).canonicalize().ok()?;
    let target = path.canonicalize().ok()?;
    (target.starts_with(&home) && target.is_file()).then_some(target)
}

/// The content type of a file a plugin may serve; other kinds are refused.
fn mime_of(path: &Path) -> Option<&'static str> {
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

/// The answer for a plugin file that is not there (or that plugins are not set up).
pub fn not_found() -> Response<Vec<u8>> {
    status(StatusCode::NOT_FOUND)
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .body(Vec::new())
        .unwrap_or_default()
}

/// Answers a `plugin://<id>/<file>` request.
pub fn handle(plugins: &PluginsDir, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some(dir) = plugins.0.as_deref() else {
        return status(StatusCode::NOT_FOUND);
    };
    let raw = request.uri().path().trim_start_matches('/');
    let Ok(path) = percent_encoding::percent_decode_str(raw).decode_utf8() else {
        return status(StatusCode::BAD_REQUEST);
    };
    let Some((id, rel)) = path.split_once('/') else {
        return status(StatusCode::NOT_FOUND);
    };
    let Some(file) = resolve(dir, id, rel) else {
        return status(StatusCode::NOT_FOUND);
    };
    let Some(mime) = mime_of(&file) else {
        return status(StatusCode::FORBIDDEN);
    };
    if std::fs::metadata(&file).map_or(true, |m| m.len() > MAX_FILE) {
        return status(StatusCode::PAYLOAD_TOO_LARGE);
    }
    match std::fs::read(&file) {
        Ok(bytes) => Response::builder()
            .header("Content-Type", mime)
            .header("Content-Security-Policy", CSP)
            .header("X-Content-Type-Options", "nosniff")
            .header("Cache-Control", "no-store")
            // A sandboxed page has no origin, so its own scripts are fetched cross-origin.
            .header("Access-Control-Allow-Origin", "*")
            .body(bytes)
            .unwrap_or_default(),
        Err(_) => status(StatusCode::NOT_FOUND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::http::Uri;

    fn request(path: &str) -> Request<Vec<u8>> {
        let uri: Uri = format!("http://plugin.localhost{path}").parse().unwrap();
        Request::builder().uri(uri).body(Vec::new()).unwrap()
    }

    /// A plugins folder with one plugin `demo` holding index.html, a script and a secret beside it.
    fn setup() -> (tempfile::TempDir, PluginsDir) {
        let dir = tempfile::tempdir().unwrap();
        let demo = dir.path().join("demo");
        std::fs::create_dir_all(demo.join("sub")).unwrap();
        std::fs::write(demo.join("plugin.json"), r#"{"id":"demo"}"#).unwrap();
        std::fs::write(demo.join("index.html"), "<p>hi</p>").unwrap();
        std::fs::write(demo.join("sub/app.js"), "1").unwrap();
        std::fs::write(demo.join("tool.exe"), "MZ").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "nope").unwrap();
        let plugins = PluginsDir(Some(dir.path().to_path_buf()));
        (dir, plugins)
    }

    #[test]
    fn ids_are_short_lower_case_names() {
        for good in ["a", "word-count", "x1", &"a".repeat(40)] {
            assert!(valid_id(good), "{good}");
        }
        for bad in [
            "",
            "-a",
            "a-",
            "A",
            "a b",
            "a/b",
            "a.b",
            "..",
            &"a".repeat(41),
            "é",
        ] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn plugins_are_listed_with_their_manifest_or_the_reason_they_have_none() {
        let (dir, _) = setup();
        std::fs::create_dir(dir.path().join("empty")).unwrap();
        std::fs::create_dir(dir.path().join("Bad Name")).unwrap();
        std::fs::create_dir(dir.path().join("big")).unwrap();
        std::fs::write(dir.path().join("big/plugin.json"), vec![b' '; 70_000]).unwrap();
        // A file among the folders is not a plugin.
        std::fs::write(dir.path().join("readme.txt"), "x").unwrap();
        let found = list(dir.path());
        let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["Bad Name", "big", "demo", "empty"]);
        assert!(found[2].manifest.as_deref().unwrap().contains("demo"));
        for f in [&found[0], &found[1], &found[3]] {
            assert!(f.manifest.is_none() && f.error.is_some(), "{}", f.id);
        }
        assert_eq!(list(&dir.path().join("missing")), []);
    }

    #[test]
    fn a_file_inside_the_plugin_resolves_and_nothing_outside_it_does() {
        let (dir, _) = setup();
        let d = dir.path();
        assert!(resolve(d, "demo", "index.html").is_some());
        assert!(resolve(d, "demo", "sub/app.js").is_some());
        for rel in [
            "../secret.txt",
            "sub/../../secret.txt",
            "/index.html",
            "sub",
            "missing.html",
            "..\\secret.txt",
            "C:/Windows/win.ini",
            "",
            ".",
        ] {
            assert!(resolve(d, "demo", rel).is_none(), "{rel}");
        }
        assert!(resolve(d, "..", "secret.txt").is_none());
        assert!(resolve(d, "Demo", "index.html").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_leads_out_of_the_plugin_is_not_followed() {
        let (dir, _) = setup();
        std::os::unix::fs::symlink(
            dir.path().join("secret.txt"),
            dir.path().join("demo/link.txt"),
        )
        .unwrap();
        assert!(resolve(dir.path(), "demo", "link.txt").is_none());
    }

    #[test]
    fn a_served_file_has_its_type_and_the_policy_that_shuts_the_network() {
        let (_dir, plugins) = setup();
        let answer = handle(&plugins, &request("/demo/index.html"));
        assert_eq!(answer.status(), StatusCode::OK);
        assert_eq!(answer.body(), b"<p>hi</p>");
        let headers = answer.headers();
        assert_eq!(headers["content-type"], "text/html; charset=utf-8");
        let csp = headers["content-security-policy"].to_str().unwrap();
        assert!(csp.contains("connect-src 'none'") && csp.contains("default-src 'none'"));
        assert_eq!(headers["x-content-type-options"], "nosniff");
    }

    #[test]
    fn requests_for_other_places_and_kinds_are_refused() {
        let (_dir, plugins) = setup();
        let code = |p: &str| handle(&plugins, &request(p)).status();
        assert_eq!(code("/demo/%2e%2e/secret.txt"), StatusCode::NOT_FOUND);
        assert_eq!(code("/demo/../secret.txt"), StatusCode::NOT_FOUND);
        assert_eq!(code("/demo"), StatusCode::NOT_FOUND);
        assert_eq!(code("/"), StatusCode::NOT_FOUND);
        assert_eq!(code("/nope/index.html"), StatusCode::NOT_FOUND);
        assert_eq!(code("/demo/tool.exe"), StatusCode::FORBIDDEN);
        assert_eq!(code("/demo/%ff.html"), StatusCode::BAD_REQUEST);
        let none = PluginsDir(None);
        assert_eq!(
            handle(&none, &request("/demo/index.html")).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn a_file_over_the_limit_is_not_served() {
        let (dir, plugins) = setup();
        std::fs::write(
            dir.path().join("demo/big.js"),
            vec![b'a'; usize::try_from(MAX_FILE).unwrap() + 1],
        )
        .unwrap();
        assert_eq!(
            handle(&plugins, &request("/demo/big.js")).status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
