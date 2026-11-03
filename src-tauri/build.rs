use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
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
        if std::path::Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    tauri_build::build();
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
