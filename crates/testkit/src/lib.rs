//! Shared helpers for the integration tests of the workspace crates: key events,
//! temporary paths, a fixed catalog, a fake connector (`mock`), an editor fixture
//! (`editor`), and a fake HTTP server (`http`).

#[cfg(feature = "editor")]
mod fixture;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "mock")]
pub mod mock;

#[cfg(feature = "editor")]
pub use fixture::Fixture;

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sqln_core::catalog::{Catalog, Columns, Relation, RelationKind, Schema};

/// A fixed catalog for a test without a database.
pub fn sample_catalog() -> Catalog {
    let relation = |name: &str, kind, columns: &[&str]| Relation {
        name: name.to_owned(),
        kind,
        columns: Columns::Loaded(columns.iter().map(|column| (*column).to_owned()).collect()),
        foreign_keys: Vec::new(),
        referencing: Vec::new(),
        notes: Vec::new(),
    };
    Catalog {
        schemas: vec![
            Schema {
                name: "public".to_owned(),
                relations: vec![
                    relation(
                        "users",
                        RelationKind::Table,
                        &["id", "name", "email", "created_at"],
                    ),
                    relation(
                        "orders",
                        RelationKind::Table,
                        &["id", "user_id", "total", "status"],
                    ),
                    relation("active_users", RelationKind::View, &["id", "name"]),
                    relation(
                        "Order Items",
                        RelationKind::Table,
                        &["order_id", "sku", "quantity"],
                    ),
                ],
                partial: false,
                objects: Vec::new(),
            },
            Schema {
                name: "sales".to_owned(),
                relations: vec![relation(
                    "invoices",
                    RelationKind::Table,
                    &["number", "due_date"],
                )],
                partial: false,
                objects: Vec::new(),
            },
        ],
        current_schema: Some("public".to_owned()),
    }
}

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

pub fn alt(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::ALT)
}

pub fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Returns the path of a database of one test. The file may exist.
///
/// The macro reads `CARGO_TARGET_TMPDIR` and `module_path!()` where it is called,
/// so each test module has its own folder in one test binary.
#[macro_export]
macro_rules! temp_db_path {
    ($name:expr) => {
        $crate::db_path_in(env!("CARGO_TARGET_TMPDIR"), module_path!(), $name)
    };
}

/// Returns a path for a database of one test. The file does not exist.
#[macro_export]
macro_rules! temp_db {
    ($name:expr) => {
        $crate::db_in(env!("CARGO_TARGET_TMPDIR"), module_path!(), $name)
    };
}

/// Returns an empty folder that belongs to one test.
#[macro_export]
macro_rules! temp_dir {
    ($name:expr) => {
        $crate::dir_in(env!("CARGO_TARGET_TMPDIR"), module_path!(), $name)
    };
}

#[doc(hidden)]
pub fn db_path_in(tmp: &str, module: &str, name: &str) -> PathBuf {
    module_dir(tmp, module).join(format!("{name}.db"))
}

#[doc(hidden)]
pub fn db_in(tmp: &str, module: &str, name: &str) -> PathBuf {
    let path = db_path_in(tmp, module, name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    path
}

#[doc(hidden)]
pub fn dir_in(tmp: &str, module: &str, name: &str) -> PathBuf {
    let path = module_dir(tmp, module).join(format!("dir-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// Returns the folder of a test module, such as `sqln/app` for `sqln::app`.
fn module_dir(tmp: &str, module: &str) -> PathBuf {
    module
        .split("::")
        .fold(PathBuf::from(tmp), |path, part| path.join(part))
}
