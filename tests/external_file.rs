// The link of a console to an external `.sql` file: the read, the explicit writes,
// and the reaction to a change by another program.

use std::fs;

use crate::common::temp_dir;
use sqln::script_file::{ExternalFile, Trouble};

fn file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = temp_dir(name).join("query.sql");
    fs::write(&path, bytes).unwrap();
    path
}

fn open(name: &str, bytes: &[u8]) -> (ExternalFile, String) {
    let read = ExternalFile::read(&file(name, bytes)).unwrap();
    (read.file, read.text)
}

#[test]
fn the_read_takes_the_text_and_the_first_revision_is_clean() {
    let (file, text) = open("ext-read", b"SELECT 1;\nSELECT 2;\n");
    assert_eq!(text, "SELECT 1;\nSELECT 2;\n");
    assert!(!file.is_dirty(0));
    assert!(file.is_dirty(1));
}

#[test]
fn a_save_keeps_the_line_ends_and_the_bom_of_the_file() {
    let bom = "\u{feff}".as_bytes();
    let (mut file, _) = open("ext-keep", &[bom, b"SELECT 1;\r\n"].concat());
    file.save("SELECT 2;\n", 1).unwrap();
    let bytes = fs::read(&file.path).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(text, "\u{feff}SELECT 2;\r\n");
    assert!(!file.is_dirty(1));
}

#[test]
fn a_save_refuses_a_file_that_another_program_changed() {
    let (mut file, _) = open("ext-changed", b"SELECT 1;\n");
    fs::write(&file.path, "SELECT other;\n").unwrap();
    assert_eq!(file.save("SELECT 2;\n", 1), Err(Trouble::Changed));
    // The text of the other program stays, and the trouble waits for a choice.
    assert_eq!(fs::read_to_string(&file.path).unwrap(), "SELECT other;\n");
    file.overwrite("SELECT 2;\n", 1).unwrap();
    assert_eq!(fs::read_to_string(&file.path).unwrap(), "SELECT 2;\n");
}

#[test]
fn a_save_reports_a_removed_file_and_an_overwrite_brings_it_back() {
    let (mut file, _) = open("ext-removed", b"SELECT 1;\n");
    fs::remove_file(&file.path).unwrap();
    assert_eq!(file.save("SELECT 2;\n", 1), Err(Trouble::Removed));
    file.overwrite("SELECT 2;\n", 1).unwrap();
    assert_eq!(fs::read_to_string(&file.path).unwrap(), "SELECT 2;\n");
}

#[test]
fn a_reload_takes_the_new_text_and_clears_the_trouble() {
    let (mut file, _) = open("ext-reload", b"SELECT 1;\n");
    fs::write(&file.path, "SELECT other;\n").unwrap();
    assert_eq!(file.save("SELECT 2;\n", 1), Err(Trouble::Changed));
    assert_eq!(file.reload().unwrap(), "SELECT other;\n");
    assert_eq!(file.trouble, None);
    file.adopt(3);
    assert!(!file.is_dirty(3));
}

#[test]
fn a_discard_stops_the_question_and_keeps_the_file() {
    let (mut file, _) = open("ext-discard", b"SELECT 1;\n");
    fs::write(&file.path, "SELECT other;\n").unwrap();
    assert_eq!(file.save("SELECT 2;\n", 1), Err(Trouble::Changed));
    file.discard(2);
    assert_eq!(file.trouble, None);
    assert!(!file.is_dirty(2));
    assert_eq!(fs::read_to_string(&file.path).unwrap(), "SELECT other;\n");
}

#[test]
fn a_save_does_not_touch_an_existing_temporary_file() {
    let (mut file, _) = open("ext-existing-temp", b"SELECT 1;\n");
    let temporary = file.path.with_file_name("query.sql.tmp");
    fs::write(&temporary, b"other work").unwrap();
    file.save("SELECT 2;\n", 1).unwrap();
    assert_eq!(fs::read(temporary).unwrap(), b"other work");
}

#[cfg(unix)]
#[test]
fn a_save_preserves_permissions_and_follows_a_symlink() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let target = file("ext-link", b"SELECT 1;\n");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let link = target.with_file_name("link.sql");
    symlink(&target, &link).unwrap();
    let mut external = ExternalFile::read(&link).unwrap().file;
    external.save("SELECT 2;\n", 1).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&target).unwrap(), b"SELECT 2;\n");
    assert_eq!(
        fs::metadata(target).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[cfg(unix)]
#[test]
fn a_retargeted_symlink_never_writes_either_destination() {
    use std::os::unix::fs::symlink;

    let target = file("ext-retarget", b"original\n");
    let other = target.with_file_name("other.sql");
    fs::write(&other, b"other\n").unwrap();
    let link = target.with_file_name("link.sql");
    symlink(&target, &link).unwrap();
    let mut external = ExternalFile::read(&link).unwrap().file;
    fs::remove_file(&link).unwrap();
    symlink(&other, &link).unwrap();
    assert!(matches!(
        external.save("edited\n", 1),
        Err(Trouble::Failed(_))
    ));
    assert_eq!(fs::read(target).unwrap(), b"original\n");
    assert_eq!(fs::read(other).unwrap(), b"other\n");
}

#[test]
fn mixed_line_endings_survive_a_save_and_edits() {
    let (mut file, text) = open("ext-mixed", b"first\r\nsecond\nthird\r\n");
    file.save(&text, 0).unwrap();
    assert_eq!(fs::read(&file.path).unwrap(), b"first\r\nsecond\nthird\r\n");
    file.save("new\nfirst\nchanged\nthird\n", 1).unwrap();
    assert_eq!(
        fs::read(&file.path).unwrap(),
        b"new\r\nfirst\r\nchanged\nthird\r\n"
    );
    file.save("changed\n", 2).unwrap();
    assert_eq!(fs::read(&file.path).unwrap(), b"changed\n");
    file.save("changed\n", 2).unwrap();
    assert_eq!(fs::read(&file.path).unwrap(), b"changed\n");
}

#[test]
fn a_file_that_is_not_utf8_is_an_error() {
    let path = file("ext-utf16", b"\xff\xfeS\x00");
    let error = ExternalFile::read(&path).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}
