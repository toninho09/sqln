// The cache of the list of saved consoles: it reads the folder again only after a change.

use std::fs;
use std::time::{Duration, SystemTime};

use sqln_store::store::scripts::{self, ListCache, ScriptMeta};
use sqln_store::store::{Root, Scope};
use sqln_testkit::temp_dir;

fn create(root: &Root, name: &str) -> String {
    let id = scripts::new_id();
    let meta = ScriptMeta::new(id.clone(), name.into(), "c1".into(), Scope::Global);
    scripts::create(root, &meta, "SELECT 1").unwrap();
    id
}

fn names(cache: &ListCache, root: &Root) -> Vec<String> {
    let mut names: Vec<_> = cache
        .list(root, Scope::Global)
        .into_iter()
        .map(|entry| entry.meta.name)
        .collect();
    names.sort();
    names
}

/// Moves the time of the folder back, as a folder that nobody changed for a while.
#[cfg(unix)]
fn age_folder(root: &Root, at: SystemTime) {
    fs::File::open(root.consoles_dir())
        .unwrap()
        .set_modified(at)
        .unwrap();
}

#[cfg(unix)]
#[test]
fn an_unchanged_folder_keeps_the_list_without_a_read() {
    let root = Root::global(temp_dir!("script-list-cache"));
    let cache = ListCache::default();
    let first = create(&root, "first");
    let old = SystemTime::now() - Duration::from_secs(60);
    age_folder(&root, old);
    assert_eq!(names(&cache, &root), ["first"]);
    // A file that leaves without a change of the folder time shows that the list
    // came from the cache.
    fs::remove_file(root.consoles_dir().join(format!("{first}.json"))).unwrap();
    age_folder(&root, old);
    assert_eq!(names(&cache, &root), ["first"]);
    // A new script changes the folder time, so the list is read again.
    create(&root, "second");
    assert_eq!(names(&cache, &root), ["second"]);
}

#[test]
fn a_recent_folder_time_reads_the_folder_each_time() {
    let root = Root::global(temp_dir!("script-list-recent"));
    let cache = ListCache::default();
    let first = create(&root, "first");
    assert_eq!(names(&cache, &root), ["first"]);
    create(&root, "second");
    assert_eq!(names(&cache, &root), ["first", "second"]);
    fs::remove_file(root.consoles_dir().join(format!("{first}.json"))).unwrap();
    assert_eq!(names(&cache, &root), ["second"]);
}

#[test]
fn a_missing_folder_lists_nothing() {
    let root = Root::global(temp_dir!("script-list-missing"));
    let cache = ListCache::default();
    assert!(names(&cache, &root).is_empty());
    create(&root, "first");
    assert_eq!(names(&cache, &root), ["first"]);
}
