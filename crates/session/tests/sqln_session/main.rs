#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.
// Root of the test binary of sqln-session: connection targets. Each file in this
// folder is a mod below; scripts/check-test-files.sh enforces it.

mod kafka_properties;
mod target;
