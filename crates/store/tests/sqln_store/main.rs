#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.
// Root of the test binary of sqln-store: script files and credentials. Each file in this
// folder is a mod below; scripts/check-test-files.sh enforces it.

mod config_file;
mod connection_options;
mod credentials;
mod external_file;
mod script_file;
mod script_list;
mod script_move;
