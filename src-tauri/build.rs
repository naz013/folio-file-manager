use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

// The rule that makes a plugin's digest is the app's own: one file, used by both.
#[path = "src/plugin_digest.rs"]
mod plugin_digest;

fn main() {
    bundled_plugin_digests();
    // What the About dialog shows, so a bug report says which build it is about. A build
    // outside a git checkout (a source tarball) has no commit to name.
    let commit = std::env::var("FOLIO_COMMIT")
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| git(&["rev-parse", "--short=10", "HEAD"]))
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=FOLIO_COMMIT={commit}");
    println!("cargo:rustc-env=FOLIO_BUILD_DATE={}", build_date());
    println!("cargo:rerun-if-env-changed=FOLIO_COMMIT");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    // A new commit moves the branch's ref (or rewrites the packed refs), a switch moves HEAD.
    // In a worktree `.git` is a file and the refs live in the common git directory, so ask
    // git where they are, and only name paths that exist: a missing one reruns every build.
    let git_dir = git(&["rev-parse", "--git-dir"]);
    let common_dir = git(&["rev-parse", "--git-common-dir"]);
    let watched = [
        git_dir.map(|d| format!("{d}/HEAD")),
        common_dir.clone().map(|d| format!("{d}/refs/heads")),
        common_dir.map(|d| format!("{d}/packed-refs")),
    ];
    for path in watched.into_iter().flatten() {
        if Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    drop_stale_plugin_copy();
    tauri_build::build();
}

/// Hashes each plugin folder under `resources/plugins/` with the app's own digest rule and
/// writes the result for the app to include. A bundled plugin whose files on disk later differ
/// (someone with write access to the install folder changed them) no longer matches it.
fn bundled_plugin_digests() {
    let root = Path::new("resources/plugins");
    println!("cargo:rerun-if-changed=resources/plugins");
    let mut digests = Vec::new();
    if let Ok(read) = std::fs::read_dir(root) {
        for entry in read.flatten() {
            if !entry
                .file_type()
                .is_ok_and(|t| t.is_dir() && !t.is_symlink())
            {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let (digest, _) = plugin_digest::snapshot(&entry.path())
                .unwrap_or_else(|why| panic!("bundled plugin {id}: {why}"));
            digests.push((id, digest));
        }
    }
    digests.sort();
    let mut code =
        String::from("/// The digest of each bundled plugin, fixed when the app was built.\n");
    code.push_str("pub const BUNDLED_DIGESTS: &[(&str, &str)] = &[\n");
    for (id, digest) in &digests {
        // Writing to a String can't fail.
        let _ = writeln!(code, "    ({id:?}, {digest:?}),");
    }
    code.push_str("];\n");
    let out = std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    std::fs::write(Path::new(&out).join("bundled_digests.rs"), code)
        .expect("bundled_digests.rs can be written");
}

/// In a dev or test run the app reads its bundled plugins from a copy next to the executable
/// (`target/<profile>/plugins`). The copy adds and overwrites files but never removes one deleted
/// or renamed in the source, and a stale file would make the folder differ from the digest fixed
/// above. The copy is a build product: it is removed here and made again just after.
fn drop_stale_plugin_copy() {
    let Ok(out) = std::env::var("OUT_DIR") else {
        return;
    };
    // OUT_DIR is `target/<profile>/build/<crate>-<hash>/out`.
    if let Some(profile) = Path::new(&out).ancestors().nth(3) {
        let copy = profile.join("plugins");
        if copy.is_dir() {
            let _ = std::fs::remove_dir_all(copy);
        }
    }
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (out.status.success() && !text.is_empty()).then_some(text)
}

/// Today's date in UTC as `YYYY-MM-DD`; `SOURCE_DATE_EPOCH` makes a build repeatable.
fn build_date() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
        });
    // Days since 1970 to a calendar date (Howard Hinnant's civil-from-days).
    let days = secs.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
