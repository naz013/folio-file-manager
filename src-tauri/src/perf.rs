//! Opt-in performance instrumentation used by `scripts/perf/run.ps1`.
//!
//! Disabled unless `FOLIO_PERF_LOG` is set. See docs/PERFORMANCE.md.
//! - `FOLIO_PERF_LOG`: file that receives one `<ms since start>\t<event>` line per report.
//! - `FOLIO_BENCH_DIRS`: `;`-separated folders the UI benchmarks after startup.

use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// Call as early as possible; cold-start time is measured from here.
pub fn mark_start() {
    START.get_or_init(Instant::now);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerfConfig {
    pub enabled: bool,
    pub bench_dirs: Vec<String>,
}

pub fn config() -> PerfConfig {
    let bench_dirs = std::env::var("FOLIO_BENCH_DIRS")
        .map(|v| {
            v.split(';')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    PerfConfig {
        enabled: std::env::var_os("FOLIO_PERF_LOG").is_some(),
        bench_dirs,
    }
}

/// Appends `event` to the perf log, prefixed with milliseconds since start.
pub fn report(event: &str) {
    let Some(path) = std::env::var_os("FOLIO_PERF_LOG") else {
        return;
    };
    let elapsed = START.get().map(|s| s.elapsed().as_secs_f64() * 1000.0);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{:.1}\t{event}", elapsed.unwrap_or(0.0));
    }
}
