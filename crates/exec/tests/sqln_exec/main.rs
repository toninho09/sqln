#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.
// Root of the test binary of sqln-exec: jobs, completion, comparisons,
// definitions, and diagrams.
// Each file in this folder is a mod below; scripts/check-test-files.sh enforces it.

mod arrange;
mod compare_schema;
mod complete;
mod definition;
mod diagram;
mod dialect;
mod join_suggestions;
