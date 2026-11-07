// Root of one test binary: the editors and their completions. Each mod below
// was one test target before; scripts/check-test-files.sh keeps every file in
// a root.

mod common;

mod complete;
mod complete_syntax;
mod editor_conventional;
mod editor_search;
mod editor_suggestions;
mod editor_undo;
mod editor_vim;
mod external_file;
mod join_suggestions;
mod script_file;
