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
   under `%TEMP%\folio-perf` (`scripts/perf/make-fixtures.mjs`), and a folder of 10,000
   **different** pictures, about 900 MB (`scripts/perf/make-photos.mjs`). The shell caches
   thumbnails by content, so one picture copied 10,000 times would draw once and measure nothing.
2. Builds a release binary (`tauri build --no-bundle`).
3. **Cold start:** launches the app 3 times and records time from process start to first
   painted frame of the home folder. Reports the median.
4. **Idle memory:** 10 s after first paint, sums working set and private bytes of
   `folio.exe` and its WebView2 processes.
5. **Folder benchmarks:** for each of the two folder fixtures, in grid and list view:
   - `listMs`: `list_dir` round trip (Rust listing + IPC + decoding), warm cache.
   - `openMs`: navigate into the folder until the first frame is painted.
   - Scroll: 600 frames at 60 px/frame; average fps, p95 and max frame time, frames > 25 ms.
   - `jumpMaxMs`: slowest paint after jumping to 50% / 100% / 25% / 75% / 0%.
6. **Gallery on 10,000 pictures:** a fresh run on the pictures folder (`photos-*`), in the
   gallery view only, so the shell has not drawn the pictures in another view first. The strip
   scrolls sideways (same numbers along the other axis), and the bench also jumps to the
   middle and the end, and holds the right arrow key for 300 frames (`arrow*`). After the
   scroll, each jump and the hold it waits up to 8 s for every picture tile to show its picture
   (`unpaintedAfterScroll`, `unpaintedAfterJump`, `unpaintedAfterHold`, `scrollSettleMs`,
   `settleMs`): a tile left without its picture is on the plain icon. Memory is sampled 10 s
   after the run, apart from the 50k folders'. The first run draws 10,000 pictures the shell
   has never seen; run again for a warm cache.

Instrumentation is opt-in through environment variables and is inert otherwise:

| Variable           | Effect                                                |
| ------------------ | ----------------------------------------------------- |
| `FOLIO_PERF_LOG`   | File that receives `<ms since start>\t<event>` lines  |
| `FOLIO_BENCH_DIRS` | `;`-separated folders the UI benchmarks after startup |

For Rust listing alone:
`cargo run --release --manifest-path src-tauri/Cargo.toml --example bench_list -- <dir>`.

## Results — 2026-10-30

Windows 11 Pro 22621, 60 Hz display, release build, NTFS SSD. 1 benchmark run per fixture;
cold start is the median of 3 runs.

| Metric                                    | Budget   | Before                    | After                                  | Status           |
| ----------------------------------------- | -------- | ------------------------- | -------------------------------------- | ---------------- |
| 50k folder scroll                         | 60 fps   | 60 fps, p95 16.7 ms       | 60 fps, p95 16.8 ms, 0/599 slow (grid) | ✅ Pass          |
| 10k folder open → first paint             | < 150 ms | 164 ms grid / 186 ms list | 67 ms grid / 132 ms list               | ✅ Pass          |
| Cold start → first paint                  | < 1 s    | 1,114 ms                  | 897 ms (856–1,452)                     | ✅ Pass (median) |
| Idle memory, Folio process                | < 80 MB  | 28 MB                     | 28 MB                                  | ✅ Pass          |
| Idle memory, total private incl. WebView2 | < 150 MB | 136 MB                    | 136 MB                                 | ✅ Pass          |

Other numbers (after):

| Fixture | View | listMs | openMs | avg fps | p95 / max frame | slow frames | jumpMax |
| ------- | ---- | ------ | ------ | ------- | --------------- | ----------- | ------- |
| 10k     | grid | 36     | 67     | 60      | 16.7 / 49.9 ms  | 3/599       | 47 ms   |
| 10k     | list | 36     | 132    | 60      | 16.7 / 33.3 ms  | 1/599       | 74 ms   |
| 50k     | grid | 238    | 171    | 60      | 16.8 / 16.8 ms  | 0/599       | 90 ms   |
| 50k     | list | 238    | 562    | 60      | 16.7 / 17.0 ms  | 0/599       | 35 ms   |

### What changed

- **Compact listing over IPC:** entries are sent as `[name, flags, size, modified]` tuples
  without the full path. 10k entries went from 1.7 MB to 371 KB of JSON;
  `list_dir` round trip 108 → 36 ms.
- **No deep reactivity on listings:** `entries` uses `$state.raw`, so Svelte no longer
  proxies tens of thousands of objects.
- **Sort keys computed once:** `sort_by_cached_key` instead of lowercasing names on every
  comparison.

### What the budgets are measured with

The figures here are taken with a single window and its default tab. A second window adds
its own WebView2 renderer process (roughly 30–60 MB private), which is why PRODUCT.md holds
the 80 MB and 150 MB memory budgets for a single window and allows each extra window a
provisional < 60 MB more. That allowance has not been measured yet: run
`scripts/perf/run.ps1` with a second window open and record the result here.

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

## Gallery on 10,000 pictures — 2026-11-03 (#52, #102)

10,000 different 240 × 160 PNGs (`make-photos.mjs`), gallery view only, release build, Windows 11
Pro 22621, 60 Hz display, one run, cold shell cache (the pictures had never been drawn).

| Metric                                                 | Target   | Measured                                  | Status  |
| ------------------------------------------------------ | -------- | ----------------------------------------- | ------- |
| Open → first paint of the strip (`openMs`)             | < 150 ms | 54 ms                                     | ✅ Pass |
| Scroll the strip, 600 frames at 60 px                  | 60 fps   | 60 fps, p95 16.8 ms, max 16.9, 0/599 slow | ✅ Pass |
| Scrollbar jumps (50%, 100%, 25%, 75%, 0%), slowest     | n/a      | 33.5 ms                                   | ✅      |
| Hold the right arrow key, 300 frames                   | 60 fps   | 60 fps, p95 16.9 ms, max 16.9, 0/299 slow | ✅ Pass |
| Tiles on the plain icon after scroll / jumps / hold    | 0        | 0 / 0 / 0 of 23 tiles                     | ✅ Pass |
| Time for the pictures to arrive after the scroll stops | n/a      | 214 ms (after the hold: 101 ms)           | ✅      |

Folder fixtures in the same run (grid, list): 10k opens in 99 ms / 113 ms (budget < 150 ms)
and scrolls at 60 fps with 0 slow frames; 50k opens in 163 ms / 145 ms and scrolls at 60 fps.

### Memory

The memory budgets are judged on the idle run (a fresh start, before any folder is browsed).
In this run the **idle total was 164 MB private (budget < 150 MB): over**, with the Folio process
at 34 MB (budget < 80 MB: pass). The earlier run on 2026-10-30 measured 136 MB, and nothing
browsed the gallery before this sample, so the gallery did not cause it; the WebView2 runtime
version, other apps and the 7 processes counted may differ from that day. Not yet explained:
repeat the idle run before reading it as a regression, and compare with a build from before
the changes since 2026-10-30.

For scale: 10 s after browsing 10k pictures in the gallery, 343 MB private (host 48 MB). After
the 50k folders, 314 MB. WebView2 keeps decoded pictures, so a bounded cache is still a
follow-up (see above).

### Caveats

- One run, cold shell cache for the pictures; the first cold start of the three took 2,075 ms
  (median 914 ms, budget < 1 s).
- The synthetic arrow key skips the OS's own key-repeat timing; it drives the same handler.
- The pictures are small and simple. Real photos are larger to decode, so thumbnails would
  take longer to arrive; the strip itself stays virtualized (about 23 tiles exist at a time).
