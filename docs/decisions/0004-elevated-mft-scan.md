# ADR 0004 — MFT fast scan through an elevated helper

- **Status:** Accepted
- **Date:** 2026-11-03

## Context

Walking a whole drive takes minutes. On NTFS the master file table (MFT) lists every file and
folder with its size, so reading it takes seconds, as WizTree does. The MFT can only be read
through a raw handle on the volume (`\\.\C:`), and Windows gives that handle to administrators
only. Folio must not run as an administrator: drag and drop from Explorer stops working in an
elevated window, and every bug in the app would run with full rights.

## Decision

1. **Opt-in.** Settings › Storage has "Fast scan"; it is off by default and says what it does.
2. **Only the scan is elevated.** Folio re-launches its own executable with `--mft-helper` through
   `ShellExecuteEx` with the `runas` verb. Windows shows its own consent prompt, every time a
   scan starts. Nothing stays elevated afterwards and no service is installed.
3. **The helper reads, never writes.** It opens the volume read-only, parses the MFT, and
   sends a compact summary (folder totals, files of 1 MB and over) to Folio over a named pipe.
   It touches no file and runs no code from the volume.
4. **The pipe is private.** Folio creates it with a random 128-bit name, a single instance, and
   remote clients rejected. The helper accepts only a drive written `X:` and a pipe name of
   Folio's own form, and refuses to write unless the pipe's server is the process whose id it was
   given **and** that process runs the same program file as the helper itself. A process of
   another program cannot make the helper talk to it, even if the person approves a prompt
   it started.
5. **Fallback, never failure.** Declining the prompt, a drive that is not NTFS, a record layout the
   reader does not handle, or a helper that stops early all fall back to the normal walker, and
   the scan reports why in a note.
6. **Same cache.** The helper's result becomes the same tree the walker builds, so every view of
   the sizes (Storage, Cleanup, folder sizes) works the same whichever scan made it.

## Consequences

- A cancelled scan cannot terminate the elevated helper (an unelevated process may not). It stops
  because the pipe closes: the helper checks its writes and exits on the first failed one.
- Anyone who can start Folio can ask for the prompt; they cannot get past it, and the helper
  sends its answer only to a Folio process (the same file) that is the pipe's server.
- The prompt names the program and not the drive or folder, and the helper is Folio's own file:
  where Folio is installed in a folder the user can write, other software running as that user
  could replace the file before the prompt is approved. A signed build under Program Files
  closes this; until then it is the same exposure as any per-user installed program that asks
  for administrator rights.
- Cancelling while Windows' prompt is open takes effect once the prompt is answered. If the
  helper then starts, it finds no pipe to write to and exits.
- Differences from the walker: the MFT reader counts a hard-linked file once, ignores
  alternate data streams, and leaves out the NTFS system files (`$MFT` and the like). Links and
  junctions are not followed, as in the walker. It also lists folders the walker can't open
  (`System Volume Information`, other users' profiles) with their sizes, since the table holds
  them: they show in Storage totals. Cleanup keeps its protected-path rules whatever the cache
  holds, because it checks each item again from disk before moving it (ADR 0003).
- The helper sends paths relative to the folder scanned and Folio joins them onto the folder as
  it was asked for, so the tree is spelled one way whatever the capitals on disk.
- Not covered: files whose size lives only in an extension record are looked up; an MFT that is
  itself described by an attribute list is not read (the scan falls back).
- macOS and Linux have no equivalent; the setting is hidden there.
