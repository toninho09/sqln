// The saved state of one console: when it writes, and how it reacts to a change of the
// files by another program.

mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::temp_dir;
use sqln::script_file::{RETRY_DELAY, SAVE_DELAY, ScriptFile, Trouble};
use sqln::store::file;
use sqln::store::scripts::{self, LineEnding, ScriptMeta};
use sqln::store::{Root, Scope};

fn root(name: &str) -> Root {
    Root::global(temp_dir(name))
}

fn meta(id: &str) -> ScriptMeta {
    ScriptMeta::new(id.into(), "console-1".into(), "c1".into(), Scope::Global)
}

fn new_file(root: &Root, id: &str) -> ScriptFile {
    let lock = scripts::lock(root, id).unwrap();
    ScriptFile::new(Scope::Global, meta(id), lock, 0)
}

fn on_disk(root: &Root, id: &str) -> String {
    scripts::read_text(root, id).unwrap().0
}

#[test]
fn the_write_comes_500_ms_after_the_last_edit() {
    let root = root("sf-timing");
    let mut file = new_file(&root, "s1");
    let t0 = Instant::now();
    // The first look at a new revision starts the wait.
    assert!(!file.due(1, t0));
    assert!(!file.due(1, t0 + SAVE_DELAY - Duration::from_millis(1)));
    assert!(file.due(1, t0 + SAVE_DELAY));
    // Another edit restarts the wait.
    assert!(!file.due(2, t0 + Duration::from_millis(600)));
    assert!(!file.due(2, t0 + Duration::from_millis(1_099)));
    assert!(file.due(2, t0 + Duration::from_millis(1_100)));
}

#[test]
fn a_clean_console_is_never_due() {
    let root = root("sf-clean");
    let mut file = new_file(&root, "s1");
    let t0 = Instant::now();
    assert!(!file.is_dirty(0));
    assert!(!file.due(0, t0));
    assert!(!file.due(0, t0 + Duration::from_secs(60)));
}

#[test]
fn the_first_save_creates_both_files_and_later_saves_replace_the_text() {
    let root = root("sf-save");
    let mut file = new_file(&root, "s1");
    let now = Instant::now();
    file.save(&root, "SELECT 1;", 1, now).unwrap();
    assert!(!file.is_dirty(1));
    assert_eq!(on_disk(&root, "s1"), "SELECT 1;");
    assert_eq!(scripts::list(&root, Scope::Global).len(), 1);
    file.save(&root, "SELECT 2;", 2, now).unwrap();
    assert_eq!(on_disk(&root, "s1"), "SELECT 2;");
    assert!(file.is_dirty(3));
}

#[test]
fn a_file_with_crlf_keeps_its_line_ending_on_save() {
    let root = root("sf-crlf");
    scripts::create(&root, &meta("s1"), "a\r\nb").unwrap();
    let (raw, hash) = scripts::read_text(&root, "s1").unwrap();
    let (text, ending) = scripts::decode(&raw);
    assert_eq!(ending, LineEnding::Crlf);
    assert_eq!(text, "a\nb");
    let lock = scripts::lock(&root, "s1").unwrap();
    let mut file = ScriptFile::open(Scope::Global, meta("s1"), lock, 0, Some((hash, ending)), &root);
    file.save(&root, "a\nb\nc", 1, Instant::now()).unwrap();
    assert_eq!(on_disk(&root, "s1"), "a\r\nb\r\nc");
}

#[test]
fn a_write_refuses_a_file_that_another_program_changed() {
    let root = root("sf-changed");
    let mut file = new_file(&root, "s1");
    file.save(&root, "mine", 1, Instant::now()).unwrap();
    fs::write(root.consoles_dir().join("s1.sql"), "theirs").unwrap();
    let result = file.save(&root, "mine again", 2, Instant::now());
    assert_eq!(result, Err(Trouble::Changed));
    assert_eq!(on_disk(&root, "s1"), "theirs");
    assert_eq!(file.trouble, Some(Trouble::Changed));
    // The autosave waits for a choice.
    assert!(!file.due(3, Instant::now() + Duration::from_secs(60)));
}

#[test]
fn a_write_refuses_a_file_that_another_program_removed() {
    let root = root("sf-removed");
    let mut file = new_file(&root, "s1");
    file.save(&root, "mine", 1, Instant::now()).unwrap();
    fs::remove_file(root.consoles_dir().join("s1.sql")).unwrap();
    assert_eq!(file.save(&root, "again", 2, Instant::now()), Err(Trouble::Removed));
    assert!(!root.consoles_dir().join("s1.sql").exists());
}

#[test]
fn a_new_file_that_appears_before_the_first_write_is_a_change() {
    let root = root("sf-appears");
    let mut file = new_file(&root, "s1");
    fs::create_dir_all(root.consoles_dir()).unwrap();
    fs::write(root.consoles_dir().join("s1.json"), "{}").unwrap();
    assert!(file.save(&root, "mine", 1, Instant::now()).is_err());
    assert!(!root.consoles_dir().join("s1.sql").exists());
}

#[test]
fn overwrite_writes_the_text_over_the_file_and_clears_the_problem() {
    let root = root("sf-overwrite");
    let mut file = new_file(&root, "s1");
    file.save(&root, "mine", 1, Instant::now()).unwrap();
    fs::write(root.consoles_dir().join("s1.sql"), "theirs").unwrap();
    let _ = file.save(&root, "mine 2", 2, Instant::now());
    file.overwrite(&root, "mine 2", 2).unwrap();
    assert_eq!(file.trouble, None);
    assert_eq!(on_disk(&root, "s1"), "mine 2");
    assert!(!file.is_dirty(2));
    // The file is writable again with the new hash.
    file.save(&root, "mine 3", 3, Instant::now()).unwrap();
}

#[test]
fn overwrite_recreates_a_removed_script_with_its_identity_file() {
    let root = root("sf-recreate");
    let mut file = new_file(&root, "s1");
    file.save(&root, "mine", 1, Instant::now()).unwrap();
    scripts::remove(&root, "s1").unwrap();
    file.poll(&root);
    assert_eq!(file.trouble, Some(Trouble::Removed));
    file.overwrite(&root, "mine", 1).unwrap();
    assert_eq!(scripts::list(&root, Scope::Global).len(), 1);
    assert_eq!(on_disk(&root, "s1"), "mine");
}

#[test]
fn reload_reads_the_file_and_clears_the_problem() {
    let root = root("sf-reload");
    let mut file = new_file(&root, "s1");
    file.save(&root, "mine", 1, Instant::now()).unwrap();
    fs::write(root.consoles_dir().join("s1.sql"), "line 1\r\nline 2").unwrap();
    file.poll(&root);
    assert_eq!(file.trouble, Some(Trouble::Changed));
    let text = file.reload(&root).unwrap();
    assert_eq!(text, "line 1\nline 2");
    file.adopt(0);
    assert_eq!(file.trouble, None);
    assert!(!file.is_dirty(0));
    // The next save uses the new hash and the line ending of the file.
    file.save(&root, "line 1\nline 2\nline 3", 1, Instant::now()).unwrap();
    assert_eq!(on_disk(&root, "s1"), "line 1\r\nline 2\r\nline 3");
}

#[test]
fn a_poll_sees_a_change_only_when_the_content_differs() {
    let root = root("sf-poll");
    let mut file = new_file(&root, "s1");
    file.save(&root, "same", 1, Instant::now()).unwrap();
    file.poll(&root);
    assert_eq!(file.trouble, None);
    // A touch changes the time and keeps the content.
    let path = root.consoles_dir().join("s1.sql");
    let later = std::time::SystemTime::now() + Duration::from_secs(5);
    fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
    file.poll(&root);
    assert_eq!(file.trouble, None);
    fs::write(&path, "other").unwrap();
    file.poll(&root);
    assert_eq!(file.trouble, Some(Trouble::Changed));
}

#[test]
fn a_failed_write_keeps_the_text_and_tries_again_later() {
    let root = root("sf-failed");
    // A file in the place of the folder makes every write fail.
    fs::write(root.dir.join("consoles"), "not a folder").unwrap();
    let mut file = new_file(&root, "s1");
    let t0 = Instant::now();
    assert!(!file.due(1, t0));
    assert!(file.due(1, t0 + SAVE_DELAY));
    let result = file.save(&root, "mine", 1, t0 + SAVE_DELAY);
    assert!(matches!(result, Err(Trouble::Failed(_))), "{result:?}");
    assert!(file.is_dirty(1));
    // The retry waits, and it does not need a choice.
    assert!(!file.trouble.as_ref().unwrap().needs_choice());
    assert!(!file.due(1, t0 + SAVE_DELAY + RETRY_DELAY - Duration::from_millis(1)));
    assert!(file.due(1, t0 + SAVE_DELAY + RETRY_DELAY));
    // The folder comes back, and the next write works.
    fs::remove_file(root.dir.join("consoles")).unwrap();
    file.save(&root, "mine", 1, t0 + SAVE_DELAY + RETRY_DELAY).unwrap();
    assert_eq!(file.trouble, None);
    assert_eq!(on_disk(&root, "s1"), "mine");
}

#[test]
fn rebase_marks_the_new_editor_clean_or_dirty() {
    let root = root("sf-rebase");
    let mut file = new_file(&root, "s1");
    file.save(&root, "x", 5, Instant::now()).unwrap();
    file.rebase(0, false);
    assert!(!file.is_dirty(0));
    file.rebase(0, true);
    assert!(file.is_dirty(0));
}

#[test]
fn the_hash_function_is_stable_inside_a_process() {
    assert_eq!(file::hash(b"abc"), file::hash(b"abc"));
    assert_ne!(file::hash(b"abc"), file::hash(b"abd"));
}
