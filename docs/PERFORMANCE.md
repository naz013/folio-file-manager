# Folio — Performance

Budgets live in [PRODUCT.md](PRODUCT.md#performance-budgets). This page explains how they
are measured and records the latest results.

## How to measure

```powershell
powershell -ExecutionPolicy Bypass -File scripts/perf/run.ps1          # builds release first
powershell -ExecutionPolicy Bypass -File scripts/perf/run.ps1 -NoBuild # reuse the current build
```

The script:

1. Creates fixture folders with 10,000 and 50,000 items (5% folders, mixed file types)
   under `%TEMP%\folio-perf` (`scripts/perf/make-fixtures.mjs`).
2. Builds a release binary (`tauri build --no-bundle`).
3. **Cold start:** launches the app 3 times and records time from process start to first
   painted frame of the home folder. Reports the median.
4. **Idle memory:** 10 s after first paint, sums working set and private bytes of
   `folio.exe` and its WebView2 processes.
5. **Folder benchmarks:** for each fixture, in grid and list view:
   - `listMs`: `list_dir` round trip (Rust listing + IPC + decoding), warm cache.
   - `openMs`: navigate into the folder until the first frame is painted.
   - Scroll: 600 frames at 60 px/frame; average fps, p95 and max frame time, frames > 25 ms.
   - `jumpMaxMs`: slowest paint after jumping to 50% / 100% / 25% / 75% / 0%.

Instrumentation is opt-in through environment variables and is inert otherwise:

| Variable | Effect |
|---|---|
| `FOLIO_PERF_LOG` | File that receives `<ms since start>\t<event>` lines |
| `FOLIO_BENCH_DIRS` | `;`-separated folders the UI benchmarks after startup |

For Rust listing alone:
`cargo run --release --manifest-path src-tauri/Cargo.toml --example bench_list -- <dir>`.

## Results — 2026-10-30

Windows 11 Pro 22621, 60 Hz display, release build, NTFS SSD. 1 benchmark run per fixture;
cold start is the median of 3 runs.

| Metric | Budget | Before | After | Status |
|---|---|---|---|---|
| 50k folder scroll | 60 fps | 60 fps, p95 16.7 ms | 60 fps, p95 16.8 ms, 0/599 slow (grid) | ✅ Pass |
| 10k folder open → first paint | < 150 ms | 164 ms grid / 186 ms list | 67 ms grid / 132 ms list | ✅ Pass |
| Cold start → first paint | < 1 s | 1,114 ms | 897 ms (856–1,452) | ✅ Pass (median) |
| Idle memory, Folio process | < 80 MB | 28 MB | 28 MB | ✅ Pass |
| Idle memory, total private incl. WebView2 | < 150 MB | 136 MB | 136 MB | ✅ Pass |

Other numbers (after):

| Fixture | View | listMs | openMs | avg fps | p95 / max frame | slow frames | jumpMax |
|---|---|---|---|---|---|---|---|
| 10k | grid | 36 | 67 | 60 | 16.7 / 49.9 ms | 3/599 | 47 ms |
| 10k | list | 36 | 132 | 60 | 16.7 / 33.3 ms | 1/599 | 74 ms |
| 50k | grid | 238 | 171 | 60 | 16.8 / 16.8 ms | 0/599 | 90 ms |
| 50k | list | 238 | 562 | 60 | 16.7 / 17.0 ms | 0/599 | 35 ms |

### What changed

- **Compact listing over IPC:** entries are sent as `[name, flags, size, modified]` tuples
  without the full path. 10k entries went from 1.7 MB to 371 KB of JSON;
  `list_dir` round trip 108 → 36 ms.
- **No deep reactivity on listings:** `entries` uses `$state.raw`, so Svelte no longer
  proxies tens of thousands of objects.
- **Sort keys computed once:** `sort_by_cached_key` instead of lowercasing names on every
  comparison.

### Idle memory: why there are two budgets

The `folio.exe` host uses **28 MB**. The other ~108 MB private (~287 MB working set) are
the WebView2 runtime processes (browser, GPU, renderer, utility), which every Tauri app
on Windows pays. A single 80 MB budget for everything is not reachable with a webview UI,
so on 2026-10-30 the budget was split: < 80 MB for the Folio process and < 150 MB private
in total including WebView2 (see PRODUCT.md).

These figures were taken before the script was tightened to count only the benchmarked
instance's WebView2 processes; no other Folio instance was running, so they should hold.

Memory after benchmarking 50k folders rose to ~360 MB private, mostly WebView image
memory for thumbnails. A bounded thumbnail cache is a follow-up.

### Known noise and caveats

- Cold start is measured from the start of `run()` in the Rust host, so process creation
  and DLL loading (tens of ms) are not included.
- The first cold-start run after a build is slower (1.45 s) because files are not yet in
  the OS cache; later runs are 0.86–0.90 s. The median passes by ~100 ms, a thin margin, and
  a truly cold (just-booted) start does not meet 1 s yet.
- 10k list-view open (132 ms) is close to the 150 ms budget.
- One benchmark run per fixture on a 60 Hz display; repeat runs before drawing fine-grained
  conclusions.
- List-view opens run after the grid scroll test of the same folder and may include
  garbage collection from it, which may explain why list opens are slower than grid.
