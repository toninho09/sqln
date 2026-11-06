// Root of one test binary: settings, key maps, and connection targets. Each
// mod below was one test target before; scripts/check-test-files.sh keeps
// every file in a root.

mod common;

mod keymap;
mod query_parameters;
mod shortcuts;
mod target;
mod target_arg;
