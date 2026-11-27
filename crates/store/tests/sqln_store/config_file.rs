use std::fs;

use sqln_store::store::config::{self, Preferences};
use sqln_store::store::{Root, Scope};
use sqln_testkit::temp_dir;

#[cfg(unix)]
#[test]
fn a_save_writes_through_a_linked_configuration() {
    let dir = temp_dir!("config-link");
    let dotfiles = dir.join("dotfiles");
    fs::create_dir_all(&dotfiles).unwrap();
    let target = dotfiles.join("config.json");
    fs::write(&target, "{\"version\": 1}\n").unwrap();
    let root = Root::global(dir.join("global"));
    fs::create_dir_all(root.config_file().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, root.config_file()).unwrap();
    let preferences = Preferences {
        page_size: Some(777),
        ..Preferences::default()
    };
    let (_, known) = config::preferences_and_hash(&root, Scope::Global).unwrap();
    config::change_preferences(&root, Scope::Global, preferences, known).unwrap();
    let link = fs::symlink_metadata(root.config_file()).unwrap();
    assert!(link.file_type().is_symlink(), "the link stays");
    let text = fs::read_to_string(&target).unwrap();
    assert!(text.contains("777"), "{text}");
}

#[test]
fn the_max_scan_of_the_project_wins_over_the_global_value() {
    let dir = temp_dir!("config-max-scan");
    let store = sqln_store::store::Store {
        global: Root::global(dir.join("global")),
        project: Some(Root::project(&dir.join("repo"))),
    };
    let write = |root: &Root, text: &str| {
        fs::create_dir_all(&root.dir).unwrap();
        fs::write(root.config_file(), text).unwrap();
    };
    let project = store.project.clone().unwrap();
    write(
        &store.global,
        r#"{"version":1,"preferences":{"maxScan":300}}"#,
    );
    write(&project, r#"{"version":1}"#);
    assert_eq!(config::load(&store).config.unwrap().max_scan, Some(300));
    write(&project, r#"{"version":1,"preferences":{"maxScan":200}}"#);
    assert_eq!(config::load(&store).config.unwrap().max_scan, Some(200));
    write(&project, r#"{"version":1,"preferences":{"maxScan":0}}"#);
    let error = config::load(&store).config.unwrap_err();
    assert!(error.message.contains("maxScan"), "{}", error.message);
}
