//! Plugins on disk (ADR 0005): finding them, and serving their files to their sandboxed pages.
//!
//! A plugin is a folder under the app data directory, named by its id, with a `plugin.json`.
//! This module only reads: it lists the manifests (the page checks and understands them) and
//! answers `plugin://` requests with files from inside one plugin's folder.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::http::{Request, Response, StatusCode};

use crate::plugin_digest::{Pin, mime_of, snapshot};

pub const SCHEME: &str = "plugin";

include!(concat!(env!("OUT_DIR"), "/bundled_digests.rs"));

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
    connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'; \
    sandbox allow-scripts";

/// What a person is told about a bundled plugin whose files are not the ones the app shipped.
const DAMAGED: &str = "This built-in plugin's files don't match the ones Folio shipped with, so it is turned off. Reinstall Folio to repair it.";

/// The folders plugins live in, and the files approved to be served; managed state. `path` is the
/// person's plugins folder (`None` when the system has no app data folder); `bundled` holds the
/// plugins shipped with the app. A plugin of the same id in the person's folder replaces the
/// bundled one, which then has to be approved on its own files.
pub struct PluginsDir {
    pub path: Option<PathBuf>,
    bundled: Option<PathBuf>,
    /// The digest each bundled plugin had when the app was built. A bundled plugin whose files
    /// differ, or that is not in this list, is damaged: it is listed with an error, can't be
    /// turned on and serves nothing.
    expected: HashMap<String, String>,
    /// A plugin serves nothing until the page pins it with the digest the person approved, and
    /// then only files whose bytes still hash to what the pin saw.
    pins: Mutex<HashMap<String, Pin>>,
}

impl PluginsDir {
    pub fn new(path: Option<PathBuf>, bundled: Option<PathBuf>) -> Self {
        let expected = BUNDLED_DIGESTS
            .iter()
            .map(|(id, digest)| ((*id).to_owned(), (*digest).to_owned()))
            .collect();
        Self::with_digests(path, bundled, expected)
    }

    /// Like `new`, with the digests the bundled plugins are expected to have (tests; the app
    /// uses those fixed when it was built).
    pub fn with_digests(
        path: Option<PathBuf>,
        bundled: Option<PathBuf>,
        expected: HashMap<String, String>,
    ) -> Self {
        Self {
            path,
            bundled,
            expected,
            pins: Mutex::new(HashMap::new()),
        }
    }

    /// Whether the plugin `id` in `dir` is a bundled one, with the files it was built with.
    fn bundled_matches(&self, id: &str, now: &str) -> bool {
        self.expected.get(id).is_some_and(|d| d == now)
    }

    /// Whether `dir` is the folder of the plugins shipped with the app.
    fn is_bundled_dir(&self, dir: &Path) -> bool {
        self.bundled.as_deref() == Some(dir)
    }

    /// The folder that holds plugin `id`: the person's, else the app's own. A link there counts
    /// as holding it, so that it can't be walked around (see `plugin_home`).
    fn root_of(&self, id: &str) -> Option<&Path> {
        if !valid_id(id) {
            return None;
        }
        [self.path.as_deref(), self.bundled.as_deref()]
            .into_iter()
            .flatten()
            .find(|root| std::fs::symlink_metadata(root.join(id)).is_ok())
    }

    /// Every plugin to offer: the person's, and the bundled ones they haven't replaced.
    pub fn list_all(&self) -> Vec<Found> {
        let mut all = self.path.as_deref().map(list).unwrap_or_default();
        if let Some(bundled) = self.bundled.as_deref() {
            all.extend(list(bundled).into_iter().filter_map(|f| {
                let replaced = self
                    .path
                    .as_deref()
                    .is_some_and(|p| std::fs::symlink_metadata(p.join(&f.id)).is_ok());
                (!replaced).then(|| self.checked(f))
            }));
        }
        all.sort_by(|a, b| a.id.cmp(&b.id));
        all
    }

    /// A bundled plugin as it is offered: as found if its files are the ones the app was built
    /// with, else damaged (no manifest, an error saying so), so it can't be turned on.
    fn checked(&self, found: Found) -> Found {
        let ok = found
            .digest
            .as_deref()
            .is_some_and(|d| self.bundled_matches(&found.id, d));
        if ok || found.manifest.is_none() {
            return Found {
                bundled: true,
                ..found
            };
        }
        Found {
            id: found.id,
            manifest: None,
            error: Some(DAMAGED.to_owned()),
            digest: None,
            bundled: true,
        }
    }

    /// Approves plugin `id` as it is on disk now, if that is exactly the code with `digest` (what
    /// the person agreed to). The files are read once, and the hashes kept are the ones of the
    /// bytes that made the digest. A plugin that no longer matches is unpinned and serves nothing.
    pub fn pin(&self, id: &str, digest: &str) -> Result<(), String> {
        let dir = self.root_of(id).ok_or("There is no such plugin")?;
        let home = plugin_home(dir, id).ok_or("Not a plugin folder")?;
        let result = snapshot(&home);
        // A bundled plugin is approved only as the app was built with it.
        let result = match result {
            Ok((now, _)) if self.is_bundled_dir(dir) && !self.bundled_matches(id, &now) => {
                Err(DAMAGED.to_owned())
            }
            other => other,
        };
        let Ok(mut pins) = self.pins.lock() else {
            return Err("Plugins are unavailable".to_owned());
        };
        match result {
            Ok((now, pin)) if now == digest => {
                pins.insert(id.to_owned(), pin);
                Ok(())
            }
            Ok(_) => {
                pins.remove(id);
                Err("The plugin's files changed after they were approved".to_owned())
            }
            Err(why) => {
                pins.remove(id);
                Err(why)
            }
        }
    }

    /// Whether `bytes` is the file `rel` of plugin `id` as it was pinned.
    fn is_pinned(&self, id: &str, rel: &str, bytes: &[u8]) -> bool {
        let Ok(pins) = self.pins.lock() else {
            return false;
        };
        pins.get(id)
            .and_then(|pin| pin.get(rel))
            .is_some_and(|hash| *hash == blake3::hash(bytes).to_hex().as_str())
    }
}

/// One folder found under the plugins directory.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Found {
    /// The folder's name, which is the plugin's id.
    pub id: String,
    /// The manifest as written; the page checks it.
    pub manifest: Option<String>,
    /// Why there is no manifest to read.
    pub error: Option<String>,
    /// A hash of the manifest and every file the plugin may serve, so an approval can be tied to
    /// the code that was approved and not only to its version.
    pub digest: Option<String>,
    /// Shipped with the app (in its resources), not put in the plugins folder by the person.
    pub bundled: bool,
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
                    digest: None,
                    bundled: false,
                };
            }
            let read = read_manifest(&e.path().join(MANIFEST))
                .and_then(|text| digest(&e.path()).map(|d| (text, d)));
            match read {
                Ok((text, digest)) => Found {
                    id,
                    manifest: Some(text),
                    error: None,
                    digest: Some(digest),
                    bundled: false,
                },
                Err(why) => Found {
                    id,
                    manifest: None,
                    error: Some(why),
                    digest: None,
                    bundled: false,
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

/// A hash of the files of the plugin in `root` that could be served (links are not followed),
/// by relative path and content. Changing, adding or removing such a file changes it.
pub fn digest(root: &Path) -> Result<String, String> {
    snapshot(root).map(|(digest, _)| digest)
}

/// The folder of plugin `id` under `dir`, if it is a real folder there and not a link (or a
/// Windows junction) to somewhere else.
fn plugin_home(dir: &Path, id: &str) -> Option<PathBuf> {
    if !valid_id(id) {
        return None;
    }
    let home = dir.join(id).canonicalize().ok()?;
    (home == dir.canonicalize().ok()?.join(id)).then_some(home)
}

/// The file `rel` of plugin `id`, if it is a plain file inside that plugin's folder. `rel` is a
/// forward-slash path made only of ordinary names.
pub fn resolve(dir: &Path, id: &str, rel: &str) -> Option<PathBuf> {
    if rel.is_empty() || rel.contains(['\\', ':']) {
        return None;
    }
    let home = plugin_home(dir, id)?;
    let mut path = dir.join(id);
    for part in Path::new(rel).components() {
        match part {
            Component::Normal(name) => path.push(name),
            _ => return None,
        }
    }
    // Resolved on disk: a link inside the folder that leads outside it ends up elsewhere.
    let target = path.canonicalize().ok()?;
    (target.starts_with(&home) && target.is_file()).then_some(target)
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
    let raw = request.uri().path().trim_start_matches('/');
    let Ok(path) = percent_encoding::percent_decode_str(raw).decode_utf8() else {
        return status(StatusCode::BAD_REQUEST);
    };
    let Some((id, rel)) = path.split_once('/') else {
        return status(StatusCode::NOT_FOUND);
    };
    let Some(dir) = plugins.root_of(id) else {
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
    // The key the pin knows the file by: its path inside the plugin's folder.
    let Some(key) = plugin_home(dir, id)
        .and_then(|home| file.strip_prefix(home).ok().map(Path::to_path_buf))
        .map(|rel| {
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
    else {
        return status(StatusCode::NOT_FOUND);
    };
    match std::fs::read(&file) {
        // What is served is what was approved, byte for byte; a file changed since is refused.
        Ok(bytes) if !plugins.is_pinned(id, &key, &bytes) => status(StatusCode::FORBIDDEN),
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
        let plugins = PluginsDir::new(Some(dir.path().to_path_buf()), None);
        // Approved as it is, the way the page does when it opens the plugin.
        plugins.pin("demo", &digest(&demo).unwrap()).unwrap();
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
    fn the_digest_follows_the_servable_files_and_nothing_else() {
        let (dir, _) = setup();
        let demo = dir.path().join("demo");
        let before = digest(&demo).unwrap();
        assert_eq!(before, digest(&demo).unwrap());
        // A file that is never served doesn't count.
        std::fs::write(demo.join("notes.exe"), "x").unwrap();
        assert_eq!(before, digest(&demo).unwrap());
        // Changed, added and removed served files do.
        std::fs::write(demo.join("index.html"), "<p>other</p>").unwrap();
        let changed = digest(&demo).unwrap();
        assert_ne!(before, changed);
        std::fs::write(demo.join("extra.js"), "2").unwrap();
        let added = digest(&demo).unwrap();
        assert_ne!(changed, added);
        std::fs::remove_file(demo.join("extra.js")).unwrap();
        assert_eq!(changed, digest(&demo).unwrap());
        assert_eq!(
            list(dir.path())[0].digest.as_deref(),
            Some(changed.as_str())
        );
    }

    /// Makes `link` lead to `target`: a symlink on Unix, a junction on Windows (needs no rights).
    fn link_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).is_ok()
        }
        #[cfg(windows)]
        {
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .is_ok_and(|o| o.status.success())
        }
    }

    #[test]
    fn a_plugin_folder_that_is_a_link_serves_nothing() {
        let (dir, _) = setup();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("index.html"), "<p>x</p>").unwrap();
        if !link_dir(outside.path(), &dir.path().join("linked")) {
            return;
        }
        assert!(resolve(dir.path(), "linked", "index.html").is_none());
        assert!(resolve(dir.path(), "demo", "index.html").is_some());
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
        assert!(
            csp.contains("sandbox allow-scripts"),
            "sandboxed even outside its frame"
        );
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
        let none = PluginsDir::new(None, None);
        assert_eq!(
            handle(&none, &request("/demo/index.html")).status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn nothing_is_served_until_the_plugin_is_pinned_to_what_was_approved() {
        let (dir, plugins) = setup();
        let demo = dir.path().join("demo");
        let fresh = PluginsDir::new(Some(dir.path().to_path_buf()), None);
        let code = |p: &PluginsDir| handle(p, &request("/demo/index.html")).status();
        assert_eq!(code(&fresh), StatusCode::FORBIDDEN, "never pinned");
        // A pin for other code than what is on disk is refused, and serves nothing.
        assert!(fresh.pin("demo", "0000").is_err());
        assert_eq!(code(&fresh), StatusCode::FORBIDDEN);
        assert!(fresh.pin("demo", &digest(&demo).unwrap()).is_ok());
        assert_eq!(code(&fresh), StatusCode::OK);
        // A file swapped after the approval is no longer served; the others still are.
        std::fs::write(demo.join("index.html"), "<p>swapped</p>").unwrap();
        assert_eq!(code(&plugins), StatusCode::FORBIDDEN);
        assert_eq!(
            handle(&plugins, &request("/demo/sub/app.js")).status(),
            StatusCode::OK
        );
        // A file added after the approval is not served either.
        std::fs::write(demo.join("new.js"), "evil()").unwrap();
        assert_eq!(
            handle(&plugins, &request("/demo/new.js")).status(),
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn a_failed_pin_removes_the_old_one() {
        let (dir, plugins) = setup();
        let demo = dir.path().join("demo");
        let approved = digest(&demo).unwrap();
        std::fs::write(demo.join("index.html"), "<p>swapped</p>").unwrap();
        // The page still pins with the digest the person approved, which no longer matches.
        assert!(plugins.pin("demo", &approved).is_err());
        assert_eq!(
            handle(&plugins, &request("/demo/index.html")).status(),
            StatusCode::FORBIDDEN
        );
        assert!(plugins.pin("../demo", &approved).is_err());
    }

    /// The app's own plugins: `text`, with a page, in a folder of their own.
    fn bundle() -> tempfile::TempDir {
        let b = tempfile::tempdir().unwrap();
        std::fs::create_dir(b.path().join("text")).unwrap();
        std::fs::write(b.path().join("text/plugin.json"), r#"{"id":"text"}"#).unwrap();
        std::fs::write(b.path().join("text/index.html"), "<p>built in</p>").unwrap();
        // The same id as the person's `demo`.
        std::fs::create_dir(b.path().join("demo")).unwrap();
        std::fs::write(b.path().join("demo/plugin.json"), r#"{"id":"demo"}"#).unwrap();
        std::fs::write(b.path().join("demo/index.html"), "<p>bundled demo</p>").unwrap();
        b
    }

    /// Plugins with the person's folder `dir` and the bundle `b`, whose digests are the ones the
    /// bundled plugins have now, as the build script would have fixed them.
    fn built(dir: &Path, b: &Path) -> PluginsDir {
        let expected = std::fs::read_dir(b)
            .unwrap()
            .flatten()
            .map(|e| {
                let id = e.file_name().to_string_lossy().into_owned();
                let d = digest(&e.path()).unwrap();
                (id, d)
            })
            .collect();
        PluginsDir::with_digests(Some(dir.to_path_buf()), Some(b.to_path_buf()), expected)
    }

    #[test]
    fn bundled_plugins_are_listed_marked_and_served_like_any_other() {
        let (dir, _) = setup();
        let b = bundle();
        let plugins = built(dir.path(), b.path());
        let all = plugins.list_all();
        let seen: Vec<(&str, bool)> = all.iter().map(|f| (f.id.as_str(), f.bundled)).collect();
        // The person's `demo` replaces the bundled one of the same name.
        assert_eq!(seen, [("demo", false), ("text", true)]);
        let code = |p: &str| handle(&plugins, &request(p)).status();
        assert_eq!(
            code("/text/index.html"),
            StatusCode::FORBIDDEN,
            "not pinned yet"
        );
        let digest = all[1].digest.clone().unwrap();
        plugins.pin("text", &digest).unwrap();
        let answer = handle(&plugins, &request("/text/index.html"));
        assert_eq!(answer.status(), StatusCode::OK);
        assert_eq!(answer.body(), b"<p>built in</p>");
        // Swapped after the approval, it is refused here too.
        std::fs::write(b.path().join("text/index.html"), "<p>other</p>").unwrap();
        assert_eq!(code("/text/index.html"), StatusCode::FORBIDDEN);
    }

    #[test]
    fn a_plugin_in_the_persons_folder_hides_the_bundled_one_of_its_name() {
        let (dir, _) = setup();
        let b = bundle();
        let plugins = built(dir.path(), b.path());
        // The bundled `demo`'s digest is not the one on disk in the person's folder.
        let bundled_digest = digest(&b.path().join("demo")).unwrap();
        assert!(plugins.pin("demo", &bundled_digest).is_err());
        assert!(
            plugins
                .pin("demo", &digest(&dir.path().join("demo")).unwrap())
                .is_ok()
        );
        assert_eq!(
            handle(&plugins, &request("/demo/index.html")).body(),
            b"<p>hi</p>"
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

    #[test]
    fn a_bundled_plugin_with_a_changed_byte_is_damaged_and_cannot_be_approved() {
        let (dir, _) = setup();
        let b = bundle();
        let plugins = built(dir.path(), b.path());
        let before = plugins.list_all();
        let text = before.iter().find(|f| f.id == "text").unwrap();
        assert!(text.manifest.is_some() && text.error.is_none() && text.bundled);
        let good = text.digest.clone().unwrap();

        // Someone with write access to the install folder changes one byte.
        std::fs::write(b.path().join("text/index.html"), "<p>built-in</p>").unwrap();
        let after = plugins.list_all();
        let text = after.iter().find(|f| f.id == "text").unwrap();
        assert!(text.manifest.is_none(), "nothing to turn on");
        assert!(text.digest.is_none());
        assert!(text.bundled);
        assert!(
            text.error
                .as_deref()
                .is_some_and(|e| e.contains("don't match")),
            "{:?}",
            text.error
        );
        // Not even the digest of the files as shipped approves it, nor the digest of what is there.
        assert!(plugins.pin("text", &good).is_err());
        assert!(
            plugins
                .pin("text", &digest(&b.path().join("text")).unwrap())
                .is_err()
        );
        assert_eq!(
            handle(&plugins, &request("/text/index.html")).status(),
            StatusCode::FORBIDDEN
        );
        // The plugins that were not touched are still fine.
        assert!(
            after
                .iter()
                .filter(|f| f.id != "text")
                .all(|f| f.error.is_none())
        );

        // Put back as it was built, it is offered again.
        std::fs::write(b.path().join("text/index.html"), "<p>built in</p>").unwrap();
        assert!(plugins.list_all().iter().all(|f| f.error.is_none()));
        assert!(plugins.pin("text", &good).is_ok());
    }

    #[test]
    fn a_file_added_to_or_removed_from_a_bundled_plugin_damages_it_too() {
        let (dir, _) = setup();
        let b = bundle();
        let plugins = built(dir.path(), b.path());
        std::fs::write(b.path().join("text/extra.js"), "alert(1)").unwrap();
        let all = plugins.list_all();
        assert!(all.iter().find(|f| f.id == "text").unwrap().error.is_some());
        std::fs::remove_file(b.path().join("text/extra.js")).unwrap();
        std::fs::remove_file(b.path().join("text/index.html")).unwrap();
        let all = plugins.list_all();
        assert!(all.iter().find(|f| f.id == "text").unwrap().error.is_some());
    }

    #[test]
    fn a_folder_in_the_bundle_that_the_build_did_not_know_is_damaged() {
        let (dir, _) = setup();
        let b = bundle();
        let plugins = built(dir.path(), b.path());
        std::fs::create_dir(b.path().join("extra")).unwrap();
        std::fs::write(b.path().join("extra/plugin.json"), r#"{"id":"extra"}"#).unwrap();
        let all = plugins.list_all();
        let extra = all.iter().find(|f| f.id == "extra").unwrap();
        assert!(extra.manifest.is_none() && extra.error.is_some() && extra.bundled);
        assert!(
            plugins
                .pin("extra", &digest(&b.path().join("extra")).unwrap())
                .is_err()
        );
    }

    #[test]
    fn a_persons_plugin_replaces_a_damaged_bundled_one_and_is_approved_on_its_own_files() {
        let (dir, _) = setup();
        let b = bundle();
        let plugins = built(dir.path(), b.path());
        // Damage the bundled `demo`, which the person's own `demo` replaces.
        std::fs::write(b.path().join("demo/index.html"), "<p>tampered</p>").unwrap();
        let all = plugins.list_all();
        let demo: Vec<_> = all.iter().filter(|f| f.id == "demo").collect();
        assert_eq!(demo.len(), 1);
        assert!(
            !demo[0].bundled && demo[0].error.is_none(),
            "the person's, as it is"
        );
        assert!(
            plugins
                .pin("demo", demo[0].digest.as_deref().unwrap())
                .is_ok()
        );
        assert_eq!(
            handle(&plugins, &request("/demo/index.html")).body(),
            b"<p>hi</p>"
        );
    }

    #[test]
    fn the_digests_fixed_at_build_time_are_those_of_the_plugins_that_ship() {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/plugins");
        let on_disk: Vec<String> = {
            let mut ids: Vec<String> = std::fs::read_dir(&shipped)
                .unwrap()
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            ids.sort();
            ids
        };
        let mut built: Vec<&str> = BUNDLED_DIGESTS.iter().map(|(id, _)| *id).collect();
        built.sort_unstable();
        assert_eq!(
            built, on_disk,
            "every shipped plugin has a digest, and no other does"
        );
        for (id, fixed) in BUNDLED_DIGESTS {
            assert_eq!(&digest(&shipped.join(id)).unwrap(), fixed, "{id}");
        }
        // The app as built offers all of them, undamaged.
        let plugins = PluginsDir::new(None, Some(shipped));
        let all = plugins.list_all();
        assert_eq!(all.len(), on_disk.len());
        assert!(
            all.iter().all(|f| f.bundled && f.error.is_none()),
            "{all:?}"
        );
    }

    #[test]
    fn with_no_digests_from_the_build_every_bundled_plugin_is_damaged() {
        let (dir, _) = setup();
        let b = bundle();
        let plugins = PluginsDir::with_digests(
            Some(dir.path().to_path_buf()),
            Some(b.path().to_path_buf()),
            HashMap::new(),
        );
        let all = plugins.list_all();
        let text = all.iter().find(|f| f.id == "text").unwrap();
        assert!(text.manifest.is_none() && text.error.is_some());
        assert!(
            plugins
                .pin("text", &digest(&b.path().join("text")).unwrap())
                .is_err()
        );
    }
}
