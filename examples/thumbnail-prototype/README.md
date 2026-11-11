# Thumbnail prototype (issue #121)

Measures a plugin drawing a file's thumbnail in a hidden sandboxed frame. See
[ADR 0006](../../docs/decisions/0006-plugin-thumbnails.md) for what it found.

- `thumb-csv.js`: the page a bundled CSV viewer would ship. It reads the start of the file with
  `read`, draws the first rows on an `OffscreenCanvas` and answers with the PNG bytes.
- `harness.html`: plays Folio's part. It runs the page in a `sandbox="allow-scripts"` frame, answers
  its `read` calls, checks each answer (bytes, at most 32 KB, PNG signature, `IHDR` size) and times
  the cases: one frame serial, five in flight, a frame per file, hostile pages and a page that spins.
- `serve.py`: serves this folder on `127.0.0.1:8765` with CORS on.

```bash
python serve.py
```

Open `http://127.0.0.1:8765/harness.html`; results fill in over about half a minute and are in
`window.results`. It is a measurement aid, not part of the app: nothing here is built or tested by CI.
