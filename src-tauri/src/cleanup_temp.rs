//! Temp files and caches: cleanup of a short, fixed list of places where programs keep files
//! they can make again. Everything else, which is nearly all of `AppData` and the rest of the
//! protected places (see [`cleanup::is_protected`](crate::cleanup::is_protected)), stays out
//! of reach.
//!
//! The list is the whole rule, and it is written down here, not read from the environment: a
//! `TEMP` or `XDG_CACHE_HOME` that points somewhere else changes nothing, since it could point
//! at Documents. A file is suggested only if, once its links are resolved, it is a plain file
//! below one of the listed places and has been left alone for that place's minimum time; and
//! it is moved (to the Recycle Bin, as always) only if it is still exactly so when asked, and
//! then the moved path is the resolved one. Nothing the interface sends back is taken on trust.

use crate::cleanup::{MAX_LIST, is_cloud_only, is_partial_name, touched};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Seconds in a day.
const DAY: i64 = 86_400;
/// Most entries looked at in one place; a bigger place is judged by what was seen, and the
/// list says so.
const WALK: usize = 200_000;
/// The longest path (in characters) that is moved: the Recycle Bin of Windows can't be counted
/// on to take a longer one, and a file it can't take would be deleted for good.
const MOVABLE_PATH: usize = 260;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

impl Os {
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Linux
        }
    }

    /// Whether `Ann` and `ann` are one folder. Where a volume may or may not be case-blind,
    /// they are taken as two: a path that differs only by case is then not in a place.
    const fn ignores_case(self) -> bool {
        matches!(self, Self::Windows)
    }
}

/// The folders the places hang from. Given, not looked up, so a test can name its own.
#[derive(Clone, Debug)]
pub struct Env {
    pub os: Os,
    pub home: PathBuf,
    /// The `Local AppData` folder of the user, as the system names it (not the environment
    /// variable). Windows only.
    pub local_app_data: Option<PathBuf>,
}

impl Env {
    pub fn current(home: PathBuf, local_app_data: Option<PathBuf>) -> Self {
        Self {
            os: Os::current(),
            home,
            local_app_data: local_app_data.filter(|_| cfg!(windows)),
        }
    }
}

/// One place on the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// Names the kind of place in a list: "Temporary files", "Chrome cache".
    pub label: &'static str,
    pub root: PathBuf,
    /// The root with its links resolved: what a file's resolved path is compared with.
    real: PathBuf,
    /// How long a file must have been left alone to be suggested.
    pub min_age_days: u32,
}

/// What the list tells the window about a place.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlaceInfo {
    pub label: &'static str,
    pub path: String,
    pub min_age_days: u32,
}

/// A file that can be made again, suggested for the Recycle Bin.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TempFile {
    pub path: String,
    pub size: u64,
    /// When it was last touched (seconds since 1970).
    pub modified: i64,
    /// Which kind of place it is in, for the list. Only a label: where a file is, and so
    /// whether it may move, is found out again from the disk.
    pub place: String,
}

/// What a listing gives the window: the files, and where they were looked for.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub files: Vec<TempFile>,
    pub places: Vec<PlaceInfo>,
    /// False if a place was too big to look through whole.
    pub complete: bool,
}

/// The folders in `dir` that satisfy `keep`, sorted. A folder that can't be read has none, and
/// a link is not a folder here.
fn folders(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| keep(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// A folder of a Chromium browser's profile that holds only what it can fetch again.
const CHROMIUM_CACHES: [&str; 3] = ["Cache", "Code Cache", "GPUCache"];

/// The places on the list for this computer: each one that exists, is a real folder (not a
/// link) and, once its links are resolved, lies strictly below the user's home folder. A
/// place that fails any of those is not on the list at all.
pub fn places(env: &Env) -> Vec<Place> {
    let mut all: Vec<(&'static str, PathBuf, u32)> = Vec::new();
    let mut add = |label: &'static str, root: PathBuf, min_age_days: u32| {
        all.push((label, root, min_age_days));
    };
    match env.os {
        Os::Windows => {
            if let Some(local) = &env.local_app_data {
                // What programs leave in the user's temporary folder, once nothing has touched
                // it for a week. The folder is the one under Local AppData, whatever `TEMP`
                // says.
                add("Temporary files", local.join("Temp"), 7);
                add("Crash dumps", local.join("CrashDumps"), 30);
                add(
                    "Error reports",
                    local.join("Microsoft").join("Windows").join("WER"),
                    30,
                );
                for (label, rel) in [
                    ("Chrome cache", ["Google", "Chrome"]),
                    ("Edge cache", ["Microsoft", "Edge"]),
                    ("Brave cache", ["BraveSoftware", "Brave-Browser"]),
                ] {
                    let data = rel.iter().fold(local.clone(), |p, s| p.join(s));
                    let data = data.join("User Data");
                    for profile in folders(&data, |n| n == "Default" || n.starts_with("Profile ")) {
                        for cache in CHROMIUM_CACHES {
                            add(label, profile.join(cache), 7);
                        }
                    }
                }
                let firefox = local.join("Mozilla").join("Firefox").join("Profiles");
                for profile in folders(&firefox, |_| true) {
                    add("Firefox cache", profile.join("cache2"), 7);
                }
                add("npm cache", local.join("npm-cache"), 30);
                add("pip cache", local.join("pip").join("Cache"), 30);
                add("NuGet cache", local.join("NuGet").join("v3-cache"), 30);
            }
        }
        Os::Mac => {
            // Named caches only: the rest of `Library/Caches` holds things programs rely on.
            let base = env.home.join("Library").join("Caches");
            add("pip cache", base.join("pip"), 30);
            add("Yarn cache", base.join("Yarn"), 30);
            add("Chrome cache", base.join("Google").join("Chrome"), 7);
            for profile in folders(&base.join("Firefox").join("Profiles"), |_| true) {
                add("Firefox cache", profile.join("cache2"), 7);
            }
        }
        Os::Linux => {
            let base = env.home.join(".cache");
            add("pip cache", base.join("pip"), 30);
            add("Yarn cache", base.join("yarn"), 30);
            add("Thumbnails", base.join("thumbnails"), 30);
            add("Shader cache", base.join("mesa_shader_cache"), 30);
            add("Font cache", base.join("fontconfig"), 30);
            add("Go build cache", base.join("go-build"), 30);
        }
    }
    let ignore_case = env.os.ignores_case();
    let home = canonical(&env.home);
    all.into_iter()
        .filter_map(|(label, root, min_age_days)| {
            let meta = fs::symlink_metadata(&root).ok()?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return None;
            }
            let real = canonical(&root)?;
            // Strictly below it: the home folder itself is not a place to clean.
            home.as_ref()
                .is_some_and(|h| inside(&real, h, ignore_case))
                .then_some(Place {
                    label,
                    root,
                    real,
                    min_age_days,
                })
        })
        .collect()
}

/// `path` with every link resolved and the `\\?\` prefix of Windows taken off.
fn canonical(path: &Path) -> Option<PathBuf> {
    let full = fs::canonicalize(path).ok()?;
    let text = full.to_string_lossy();
    Some(match text.strip_prefix(r"\\?\") {
        Some(plain) if !plain.starts_with("UNC\\") => PathBuf::from(plain),
        _ => full,
    })
}

/// Path text that two spellings of one place share, with `/` between parts and no `/` at
/// the end.
fn spelled(path: &Path, ignore_case: bool) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let text = text.trim_end_matches('/');
    if ignore_case {
        text.to_lowercase()
    } else {
        text.to_owned()
    }
}

/// Whether `path` is strictly below `root`.
fn inside(path: &Path, root: &Path, ignore_case: bool) -> bool {
    let (path, root) = (spelled(path, ignore_case), spelled(root, ignore_case));
    path.len() > root.len()
        && path.starts_with(&root)
        && path.as_bytes().get(root.len()) == Some(&b'/')
}

/// Whether `path` is somewhere the list leaves alone even inside a place: what a person may
/// have made or saved there, or Folio's own unfinished work. Outlook keeps the attachments
/// people open in a folder of the internet cache and they edit them there; Explorer unpacks a
/// zip it opens into a `Temp1_name.zip` folder in Temp, where people edit what is in it; and
/// `AutoCAD` keeps its autosaves (`.sv$`) in Temp.
fn left_alone(path: &Path) -> bool {
    path.components().any(|c| {
        let part = c.as_os_str().to_string_lossy().to_lowercase();
        part == "content.outlook"
            || (part.starts_with("temp")
                && Path::new(&part)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("zip")))
            || part.ends_with(".sv$")
            || is_partial_name(&part)
    })
}

/// The place that `path` belongs to and its resolved path, if it is a plain file strictly below
/// one of `places`, with its links resolved: so the folder it is in must not be a link out.
/// `None` for anything else: a folder, a link, a file that is gone, a cloud-only file, a name
/// that is not text, a path too long to be moved safely, something a person may have made
/// (see [`left_alone`]), or a file anywhere that is not on the list.
fn place_of<'a>(path: &Path, places: &'a [Place], env: &Env) -> Option<(&'a Place, PathBuf)> {
    let text = path.to_str()?;
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() || is_cloud_only(path) {
        return None;
    }
    if env.os == Os::Windows && text.chars().count() >= MOVABLE_PATH {
        return None;
    }
    let real = canonical(path)?;
    if left_alone(&real) || real.to_str().is_none() {
        return None;
    }
    let ignore_case = env.os.ignores_case();
    places
        .iter()
        .find(|place| inside(&real, &place.real, ignore_case))
        .map(|place| (place, real))
}

/// The files in the places on the list that have been left alone for their place's minimum
/// time, biggest first. Folders are never listed, only the files in them.
pub fn temp_files(env: &Env, now: i64, limit: usize) -> Listing {
    let places = places(env);
    let mut files: Vec<TempFile> = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut complete = true;
    for place in &places {
        let before = now - i64::from(place.min_age_days) * DAY;
        let mut todo = vec![place.root.clone()];
        let mut seen = 0_usize;
        'place: while let Some(dir) = todo.pop() {
            let Ok(read) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in read.flatten() {
                seen += 1;
                if seen > WALK {
                    complete = false;
                    break 'place;
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                // A link is never followed or listed.
                if kind.is_symlink() {
                    continue;
                }
                let path = entry.path();
                if left_alone(&path) {
                    continue;
                }
                if kind.is_dir() {
                    todo.push(path);
                } else if kind.is_file() {
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };
                    if touched(&meta) > before {
                        continue;
                    }
                    let Some((owner, resolved)) = place_of(&path, &places, env) else {
                        continue;
                    };
                    // Two places that overlap would list a file twice.
                    let Some(text) = resolved.to_str() else {
                        continue;
                    };
                    if seen_paths.insert(text.to_owned()) {
                        files.push(TempFile {
                            path: text.to_owned(),
                            size: meta.len(),
                            modified: touched(&meta),
                            place: owner.label.to_owned(),
                        });
                    }
                }
            }
        }
    }
    files.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    files.truncate(limit.min(MAX_LIST));
    Listing {
        files,
        places: places
            .iter()
            .map(|p| PlaceInfo {
                label: p.label,
                path: p.root.to_string_lossy().into_owned(),
                min_age_days: p.min_age_days,
            })
            .collect(),
        complete,
    }
}

/// The items of a list shown earlier that are still, right now, files in a place on the list
/// that have been left alone for its place's minimum time, exactly as listed (same size, not
/// touched since). Which place an item is in is found out again here, not read from the item,
/// and each one comes back with its resolved path: that is what is moved, so a link put in the
/// way after this look leads nowhere.
pub fn still_listed(items: &[TempFile], env: &Env, now: i64) -> Vec<TempFile> {
    let places = places(env);
    items
        .iter()
        .filter_map(|item| {
            let (place, real) = place_of(Path::new(&item.path), &places, env)?;
            let meta = fs::symlink_metadata(&real).ok()?;
            let before = now - i64::from(place.min_age_days) * DAY;
            let at = touched(&meta);
            (meta.len() == item.size && at <= item.modified && at <= before).then(|| TempFile {
                path: real.to_string_lossy().into_owned(),
                size: item.size,
                modified: item.modified,
                place: place.label.to_owned(),
            })
        })
        .collect()
}

impl From<&TempFile> for crate::scan::BigFile {
    fn from(f: &TempFile) -> Self {
        Self {
            path: f.path.clone(),
            size: f.size,
            modified: f.modified,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    /// A fake home with the folders of a Windows profile, and an `Env` for it.
    struct Fixture {
        _dir: tempfile::TempDir,
        env: Env,
        temp: PathBuf,
    }

    fn fixture(os: Os) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = fs::canonicalize(dir.path()).unwrap().join("me");
        fs::create_dir_all(&home).unwrap();
        let home = canonical(&home).unwrap();
        let local = home.join("AppData").join("Local");
        let temp = local.join("Temp");
        fs::create_dir_all(&temp).unwrap();
        Fixture {
            env: Env {
                os,
                home,
                local_app_data: Some(local),
            },
            temp,
            _dir: dir,
        }
    }

    /// Writes a file and makes it look `days` old.
    fn file(path: &Path, days: u64, bytes: usize) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![b'x'; bytes]).unwrap();
        let then = SystemTime::now() - Duration::from_secs(days * 86_400);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(then)
            .unwrap();
        path.to_path_buf()
    }

    fn now() -> i64 {
        i64::try_from(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        )
        .unwrap()
    }

    /// Everything listed, with the clock far ahead: a file's creation time counts as touching
    /// it and is today's in a test, so how long a file has been left alone is set by the time
    /// asked about.
    fn listed(f: &Fixture) -> Vec<String> {
        temp_files(&f.env, i64::MAX / 4, 1000)
            .files
            .into_iter()
            .map(|t| t.path)
            .collect()
    }

    fn item(p: &Path, size: u64) -> TempFile {
        TempFile {
            path: p.to_string_lossy().into_owned(),
            size,
            modified: fs::symlink_metadata(p).map_or(0, |m| touched(&m)),
            place: "Temporary files".into(),
        }
    }

    #[test]
    fn only_the_places_on_the_list_are_looked_in() {
        let f = fixture(Os::Windows);
        let local = f.env.local_app_data.clone().unwrap();
        let in_temp = file(&f.temp.join("a.tmp"), 30, 10);
        let dumps = file(&local.join("CrashDumps").join("x.dmp"), 60, 10);
        // Everything else under AppData, and the rest of the profile, is not on the list.
        file(&local.join("Programs").join("App").join("data.db"), 90, 10);
        file(
            &f.env.home.join("AppData/Roaming/App/settings.json"),
            90,
            10,
        );
        file(&f.env.home.join("Documents/report.docx"), 90, 10);
        file(
            &local.join("Microsoft/Windows/Explorer/thumbcache_32.db"),
            90,
            10,
        );
        file(&local.join("Microsoft/Windows/INetCache/a/b.js"), 90, 10);
        let found = listed(&f);
        assert!(found.iter().any(|p| Path::new(p) == in_temp), "{found:?}");
        assert!(found.iter().any(|p| Path::new(p) == dumps), "{found:?}");
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "macOS moves the creation date back when the change date is set to the past"
    )]
    fn a_file_must_have_been_left_alone_for_its_places_minimum() {
        let f = fixture(Os::Windows);
        let local = f.env.local_app_data.clone().unwrap();
        file(&f.temp.join("a.tmp"), 30, 10);
        file(&local.join("CrashDumps").join("x.dmp"), 30, 10);
        let at = |days: i64| temp_files(&f.env, now() + days * DAY, 100).files.len();
        assert_eq!(at(0), 0, "made moments ago");
        assert_eq!(at(3), 0, "under a week");
        assert_eq!(
            at(8),
            1,
            "temporary files go after 7 days, dumps wait for 30"
        );
        assert_eq!(at(31), 2);
    }

    #[test]
    fn browser_caches_are_found_per_profile_and_only_their_cache_folders() {
        let f = fixture(Os::Windows);
        let data = f
            .env
            .local_app_data
            .clone()
            .unwrap()
            .join("Google/Chrome/User Data");
        let cached = file(&data.join("Default/Cache/Cache_Data/f_000001"), 30, 10);
        let code = file(&data.join("Profile 2/Code Cache/js/a"), 30, 10);
        file(&data.join("Default/Cookies"), 30, 10);
        file(&data.join("Default/Login Data"), 30, 10);
        file(&data.join("Default/Bookmarks"), 30, 10);
        file(&data.join("Local State"), 30, 10);
        file(&data.join("Guest Profile/Cache/x"), 30, 10);
        let found = listed(&f);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|p| Path::new(p) == cached));
        assert!(found.iter().any(|p| Path::new(p) == code));
    }

    #[test]
    fn what_a_person_may_have_made_or_saved_there_is_not_listed() {
        let f = fixture(Os::Windows);
        let plain = file(&f.temp.join("scratch.tmp"), 30, 10);
        file(&f.temp.join("Temp1_photos.zip/edited.psd"), 30, 10);
        file(&f.temp.join("Content.Outlook/AB12/contract.docx"), 30, 10);
        file(&f.temp.join("drawing.sv$"), 30, 10);
        file(&f.temp.join("big.zip.folio-partial"), 30, 10);
        let found = listed(&f);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(Path::new(&found[0]), plain);
        // Asking for them by hand does not get them moved either.
        let kept = still_listed(
            &[
                item(&f.temp.join("Temp1_photos.zip/edited.psd"), 10),
                item(&f.temp.join("Content.Outlook/AB12/contract.docx"), 10),
            ],
            &f.env,
            i64::MAX / 4,
        );
        assert_eq!(kept, []);
    }

    #[test]
    fn a_link_out_of_a_place_is_never_followed_or_listed() {
        let f = fixture(Os::Windows);
        let outside = f.env.home.join("Documents");
        let secret = file(&outside.join("keep.txt"), 90, 10);
        let link = f.temp.join("shortcut");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(&outside, &link);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, &link);
        #[cfg(not(any(windows, unix)))]
        let made: std::io::Result<()> = Ok(());
        if made.is_err() {
            eprintln!("skipped: no permission to make a link");
            return;
        }
        let found = listed(&f);
        assert!(
            found.iter().all(|p| Path::new(p) != secret),
            "followed a link: {found:?}"
        );
        let by_hand = item(&link.join("keep.txt"), 10);
        assert_eq!(still_listed(&[by_hand], &f.env, i64::MAX / 4), []);
    }

    /// A junction needs no privilege to make, so unlike a symbolic link this always runs.
    #[cfg(windows)]
    #[test]
    fn a_junction_is_not_followed_and_one_swapped_in_after_listing_leads_nowhere() {
        let f = fixture(Os::Windows);
        let outside = f.env.home.join("Documents");
        let secret = file(&outside.join("keep.txt"), 90, 10);
        let real = f.temp.join("sub");
        file(&real.join("a.tmp"), 30, 10);
        let junction = |link: &Path, target: &Path| {
            std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .output()
                .unwrap()
                .status
                .success()
        };
        // Listed while it is a real folder.
        let listing = temp_files(&f.env, i64::MAX / 4, 10);
        assert_eq!(listing.files.len(), 1);
        // Then the folder is replaced by a junction to a folder with a file of the same name
        // and size.
        fs::remove_dir_all(&real).unwrap();
        file(&outside.join("a.tmp"), 90, 10);
        assert!(junction(&real, &outside), "mklink /J failed");
        let still = still_listed(&listing.files, &f.env, i64::MAX / 4);
        assert_eq!(still, [], "moved through a junction: {still:?}");
        // And a junction already in the place is not walked into.
        let found = listed(&f);
        assert!(found.iter().all(|p| Path::new(p) != secret), "{found:?}");
        assert!(found.iter().all(|p| !p.contains("Documents")), "{found:?}");
    }

    #[test]
    fn only_listed_files_can_be_moved_whatever_the_window_sends() {
        let f = fixture(Os::Windows);
        let ok = file(&f.temp.join("old.tmp"), 30, 10);
        let elsewhere = file(&f.env.home.join("Documents/report.docx"), 90, 10);
        let appdata = file(&f.env.home.join("AppData/Roaming/App/state.json"), 90, 10);
        let folder = f.temp.join("sub");
        fs::create_dir_all(&folder).unwrap();
        let items = [
            item(&ok, 10),
            item(&elsewhere, 10),
            item(&appdata, 10),
            item(&folder, 0),
            item(&f.temp.join("gone.tmp"), 10),
            item(Path::new("C:\\Windows\\System32\\cmd.exe"), 10),
            // The right place but the wrong size: it changed since it was listed.
            item(&ok, 11),
        ];
        let kept = still_listed(&items, &f.env, i64::MAX / 4);
        let paths: Vec<&str> = kept.iter().map(|t| t.path.as_str()).collect();
        assert_eq!(paths, [ok.to_string_lossy().as_ref()], "{paths:?}");
    }

    #[test]
    fn what_is_moved_is_the_resolved_path_not_the_one_sent() {
        let f = fixture(Os::Windows);
        let ok = file(&f.temp.join("old.tmp"), 30, 10);
        // The same file spelled with a `.` step in it.
        let spelled = f.temp.join(".").join("old.tmp");
        let sent = TempFile {
            path: spelled.to_string_lossy().into_owned(),
            ..item(&ok, 10)
        };
        let kept = still_listed(&[sent], &f.env, i64::MAX / 4);
        assert_eq!(kept.len(), 1);
        assert_eq!(Path::new(&kept[0].path), ok);
    }

    #[test]
    fn a_path_too_long_for_the_recycle_bin_is_not_offered() {
        let f = fixture(Os::Windows);
        let mut deep = f.temp.clone();
        while deep.to_string_lossy().chars().count() < MOVABLE_PATH {
            deep = deep.join("long-folder-name-to-reach-the-limit");
        }
        let long = file(&deep.join("a.tmp"), 30, 10);
        let found = listed(&f);
        assert!(found.iter().all(|p| Path::new(p) != long), "{found:?}");
        let kept = still_listed(&[item(&long, 10)], &f.env, i64::MAX / 4);
        assert_eq!(kept, []);
    }

    #[test]
    fn a_file_touched_since_it_was_listed_is_left_alone() {
        let f = fixture(Os::Windows);
        let path = file(&f.temp.join("busy.tmp"), 30, 10);
        let listing = temp_files(&f.env, i64::MAX / 4, 10);
        assert_eq!(listing.files.len(), 1);
        // A program writes to it after the list was made.
        fs::write(&path, vec![b'y'; 25]).unwrap();
        assert_eq!(still_listed(&listing.files, &f.env, i64::MAX / 4), []);
    }

    #[test]
    fn the_temporary_folder_is_the_one_under_local_appdata_whatever_the_environment_says() {
        let f = fixture(Os::Windows);
        let temps: Vec<Place> = places(&f.env)
            .into_iter()
            .filter(|p| p.label == "Temporary files")
            .collect();
        assert_eq!(temps.len(), 1);
        assert_eq!(temps[0].root, f.temp);
        // The user's own folders are never a place, whatever is asked for.
        for p in places(&f.env) {
            for kept in ["Documents", "Desktop", "Downloads", "OneDrive", "Roaming"] {
                assert!(!p.root.to_string_lossy().contains(kept), "{:?}", p.root);
            }
        }
        // And without the system's answer for Local AppData there is no such place.
        let env = Env::current(f.env.home.clone(), None);
        assert!(places(&env).iter().all(|p| p.label != "Temporary files"));
    }

    #[test]
    fn a_place_outside_the_home_folder_is_not_on_the_list() {
        let mut f = fixture(Os::Windows);
        let elsewhere = tempfile::tempdir().unwrap();
        let other = fs::canonicalize(elsewhere.path()).unwrap();
        fs::create_dir_all(other.join("Temp")).unwrap();
        file(&other.join("Temp").join("a.tmp"), 30, 10);
        f.env.local_app_data = Some(other);
        assert_eq!(places(&f.env), []);
    }

    #[test]
    fn places_that_do_not_exist_are_not_listed() {
        let f = fixture(Os::Windows);
        let labels: Vec<_> = places(&f.env).iter().map(|p| p.label).collect();
        assert_eq!(labels, ["Temporary files"]);
    }

    #[test]
    fn mac_and_linux_look_only_in_named_caches() {
        let f = fixture(Os::Linux);
        let pip = file(&f.env.home.join(".cache/pip/wheels/a"), 60, 10);
        let thumbs = file(&f.env.home.join(".cache/thumbnails/large/b.png"), 60, 10);
        // Not named, so not touched: models, IDE state, backup indexes, config, keys.
        file(&f.env.home.join(".cache/huggingface/hub/model.bin"), 60, 10);
        file(&f.env.home.join(".cache/JetBrains/idea/log"), 60, 10);
        file(&f.env.home.join(".config/app/settings"), 60, 10);
        file(&f.env.home.join(".ssh/id_ed25519"), 60, 10);
        let found = listed(&f);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|p| Path::new(p) == pip));
        assert!(found.iter().any(|p| Path::new(p) == thumbs));

        let mac = fixture(Os::Mac);
        let yarn = file(&mac.env.home.join("Library/Caches/Yarn/v6/x"), 60, 10);
        let chrome = file(
            &mac.env
                .home
                .join("Library/Caches/Google/Chrome/Default/Cache/y"),
            60,
            10,
        );
        file(&mac.env.home.join("Library/Caches/com.app/data"), 60, 10);
        file(
            &mac.env.home.join("Library/Caches/Homebrew/downloads/z"),
            60,
            10,
        );
        file(
            &mac.env.home.join("Library/Application Support/App/db"),
            60,
            10,
        );
        file(
            &mac.env.home.join("Library/Preferences/com.app.plist"),
            60,
            10,
        );
        let found = listed(&mac);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|p| Path::new(p) == yarn));
        assert!(found.iter().any(|p| Path::new(p) == chrome));
    }

    #[test]
    fn the_list_is_biggest_first_and_capped() {
        let f = fixture(Os::Windows);
        file(&f.temp.join("small"), 30, 10);
        file(&f.temp.join("big"), 30, 1000);
        file(&f.temp.join("mid"), 30, 100);
        let two = temp_files(&f.env, i64::MAX / 4, 2);
        let sizes: Vec<u64> = two.files.iter().map(|t| t.size).collect();
        assert_eq!(sizes, [1000, 100]);
        assert!(two.complete);
    }

    #[test]
    fn spelling_and_case_do_not_hide_a_place_but_case_counts_where_volumes_may_differ() {
        assert!(inside(
            Path::new("C:/Users/Me/AppData/Local/Temp/a"),
            Path::new("c:\\users\\me\\appdata\\local\\temp\\"),
            true
        ));
        assert!(!inside(
            Path::new("C:/Users/Me/AppData/Local/Temp2/a"),
            Path::new("C:/Users/Me/AppData/Local/Temp"),
            true
        ));
        assert!(!inside(Path::new("/a/B/c"), Path::new("/a/b"), false));
        assert!(!inside(Path::new("/a/b"), Path::new("/a/b"), false));
        assert!(!Os::Mac.ignores_case() && !Os::Linux.ignores_case() && Os::Windows.ignores_case());
    }
}
