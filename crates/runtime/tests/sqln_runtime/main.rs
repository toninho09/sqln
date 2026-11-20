#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.
// Root of the test binary of sqln-runtime: executions, pools, imports, and AI metadata.
// Each file in this folder is a mod below; scripts/check-test-files.sh enforces it.

mod ai_schema;
mod exec;
mod import;
mod pool;
