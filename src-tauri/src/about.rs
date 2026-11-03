//! What the About dialog tells: which build this is, and what it runs on. Nothing here names
//! a file, a path or anything the person has tagged.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// The commit the build was made from, or "unknown" outside a git checkout.
    pub commit: String,
    /// The day the build was made, `YYYY-MM-DD` in UTC.
    pub build_date: String,
    pub tauri: String,
    /// The web view that draws the window (`WebView2` on Windows), if it can be asked.
    pub webview: Option<String>,
    pub os: String,
    pub arch: String,
}

impl AppInfo {
    pub fn collect(version: &str) -> Self {
        Self {
            version: version.to_owned(),
            commit: env!("FOLIO_COMMIT").to_owned(),
            build_date: env!("FOLIO_BUILD_DATE").to_owned(),
            tauri: tauri::VERSION.to_owned(),
            webview: tauri::webview_version().ok(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_is_named() {
        let info = AppInfo::collect("1.2.3");
        assert_eq!(info.version, "1.2.3");
        assert!(
            info.commit == "unknown" || info.commit.chars().all(|c| c.is_ascii_hexdigit()),
            "{}",
            info.commit
        );
        let parts: Vec<&str> = info.build_date.split('-').collect();
        assert_eq!(parts.len(), 3, "{}", info.build_date);
        assert_eq!(parts[0].len(), 4);
        assert!(parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())));
        assert!(!info.tauri.is_empty() && !info.os.is_empty() && !info.arch.is_empty());
    }
}
