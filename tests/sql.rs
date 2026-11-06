// Root of one test binary: SQL text, statements, plans, and structure. Each
// mod below was one test target before; scripts/check-test-files.sh keeps
// every file in a root.

mod common;

mod definition;
mod format;
mod generate;
mod plan;
mod sql_statements;
mod structure;
mod tree_filter;
mod tree_objects;
mod visible_schemas;
