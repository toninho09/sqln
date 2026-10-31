# Fork of EdTUI 0.11.7

SQLN uses this copy through `[patch.crates-io]` in its `Cargo.toml`.
The history of EdTUI is private, and the public API cannot measure or remove entries.

Changes from the published version:

- `src/state/undo.rs`: `Stack` counts its bytes. `EditorState` gets `set_undo_limit` and `take_undo_skipped`.
- `src/state.rs`: the fields of that limit.
- The folders `.github`, `assets`, and `resources` are removed.

`cargo install sqln` ignores `[patch]`. Before the release, send this change to the
original project, or publish the fork with another name. See `PLAN.md`, section 17.
