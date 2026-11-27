// The move of a script to the folder of another scope, and the creation of its files.

use std::fs;

use sqln_store::store::file::{Expect, WriteError};
use sqln_store::store::scripts::{self, ScriptMeta};
use sqln_store::store::{Root, Scope};
use sqln_testkit::temp_dir;

fn meta() -> ScriptMeta {
    ScriptMeta::new(
        scripts::new_id(),
        "moved".into(),
        "c1".into(),
        Scope::Project,
    )
}

fn sql_path(root: &Root, id: &str) -> std::path::PathBuf {
    root.consoles_dir().join(format!("{id}.sql"))
}

fn meta_path(root: &Root, id: &str) -> std::path::PathBuf {
    root.consoles_dir().join(format!("{id}.json"))
}

#[test]
fn a_text_file_that_cannot_be_read_stays_in_its_folder() {
    let from = Root::global(temp_dir!("move-unreadable-from"));
    let to = Root::global(temp_dir!("move-unreadable-to"));
    let meta = meta();
    scripts::create(&from, &meta, "SELECT 1").unwrap();
    // Text in Latin-1 is not UTF-8, so the read fails.
    let original = b"SELECT 'caf\xe9'".to_vec();
    fs::write(sql_path(&from, &meta.id), &original).unwrap();

    let error = scripts::move_to(&from, &to, &meta, Expect::Any).unwrap_err();

    assert!(error.to_string().contains("stays in its folder"), "{error}");
    assert_eq!(fs::read(sql_path(&from, &meta.id)).unwrap(), original);
    assert!(meta_path(&from, &meta.id).exists());
    assert!(!sql_path(&to, &meta.id).exists());
    assert!(!meta_path(&to, &meta.id).exists());
}

#[test]
fn a_script_without_a_text_file_moves_as_empty_text() {
    let from = Root::global(temp_dir!("move-absent-from"));
    let to = Root::global(temp_dir!("move-absent-to"));
    let meta = meta();
    scripts::write_meta(&from, &meta, sqln_store::store::file::Expect::Missing).unwrap();

    scripts::move_to(&from, &to, &meta, Expect::Any).unwrap();

    assert_eq!(scripts::read_text(&to, &meta.id).unwrap().0, "");
    assert!(!meta_path(&from, &meta.id).exists());
}

#[test]
fn a_failed_text_write_removes_the_new_identity_file_and_a_retry_works() {
    let from = Root::global(temp_dir!("move-retry-from"));
    let to = Root::global(temp_dir!("move-retry-to"));
    let meta = meta();
    scripts::create(&from, &meta, "SELECT 1").unwrap();
    // A text file in the way makes the write of the copy fail.
    fs::create_dir_all(to.consoles_dir()).unwrap();
    fs::write(sql_path(&to, &meta.id), "other").unwrap();

    assert!(scripts::move_to(&from, &to, &meta, Expect::Any).is_err());
    assert!(!meta_path(&to, &meta.id).exists());
    assert_eq!(scripts::read_text(&from, &meta.id).unwrap().0, "SELECT 1");

    fs::remove_file(sql_path(&to, &meta.id)).unwrap();
    scripts::move_to(&from, &to, &meta, Expect::Any).unwrap();
    assert_eq!(scripts::read_text(&to, &meta.id).unwrap().0, "SELECT 1");
    assert!(!sql_path(&from, &meta.id).exists());
    assert!(!meta_path(&from, &meta.id).exists());
}

#[test]
fn a_text_that_another_program_changed_stays_in_its_folder() {
    let from = Root::global(temp_dir!("move-changed-from"));
    let to = Root::global(temp_dir!("move-changed-to"));
    let meta = meta();
    let known = scripts::create(&from, &meta, "SELECT 1").unwrap();
    fs::write(sql_path(&from, &meta.id), "SELECT 2").unwrap();

    let error = scripts::move_to(&from, &to, &meta, Expect::Hash(known)).unwrap_err();

    assert_eq!(error, WriteError::Changed);
    assert_eq!(scripts::read_text(&from, &meta.id).unwrap().0, "SELECT 2");
    assert!(!sql_path(&to, &meta.id).exists());
    assert!(!meta_path(&to, &meta.id).exists());

    // A text file that another program created, or removed, also stops the move.
    let error = scripts::move_to(&from, &to, &meta, Expect::Missing).unwrap_err();
    assert_eq!(error, WriteError::Changed);
    fs::remove_file(sql_path(&from, &meta.id)).unwrap();
    let error = scripts::move_to(&from, &to, &meta, Expect::Hash(known)).unwrap_err();
    assert_eq!(error, WriteError::Removed);
    assert!(meta_path(&from, &meta.id).exists());
}
