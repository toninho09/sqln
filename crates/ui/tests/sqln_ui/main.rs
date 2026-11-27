#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.
// Root of the test binary of sqln-ui: editors, grids, forms, and drawing. Each file in
// this folder is a mod below; scripts/check-test-files.sh enforces it.

mod ai_editor;
mod analysis;
mod browser;
mod compare_data;
mod complete_syntax;
mod connection_form;
mod editor_conventional;
mod editor_search;
mod editor_suggestions;
mod editor_undo;
mod editor_vim;
mod export;
mod format;
mod grid;
mod gutter_preview;
mod kit;
mod messages;
mod palette;
mod result_numbers;
mod run_preview;
mod run_subquery;
mod shortcuts;
mod store;
