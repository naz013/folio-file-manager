# Vendored `drag` 2.1.1

A copy of the `drag` crate (Apache-2.0 OR MIT, CrabNebula Ltd.) with one change: a
`DragMode::CopyOrMove` that allows both effects at once, so the drop target (and the
Ctrl / Shift keys held during the drag) chooses between copy and move, as in Explorer.
Upstream only allows one of the two for the whole drag.

The change is in `lib.rs` (the enum and the macOS conversion) and the three
`platform_impl` modules. Drop this copy and go back to the crates.io release if
upstream gains the same mode.
