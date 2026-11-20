#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.
// Root of the test binary of sqln-core: data models and the clipboard. Each file in this
// folder is a mod below; scripts/check-test-files.sh enforces it.

mod chords;
mod clipboard;
mod grids;
mod optimizations;
