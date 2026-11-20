//! The decisions of the saved consoles that need no `App`: the age of a change, the row
//! of the list, the consoles of a connection, and the names in use.

use std::time::SystemTime;

use crate::connection::ConnectionKey;
use crate::console::Console;
use crate::exec::{ConnectionId, ConsoleId};
use crate::store::Scope;
use crate::store::scripts::ScriptEntry;
use crate::ui::text;

/// Writes the age of a change, so the list needs no clock zone.
pub(in crate::app) fn age(modified: Option<SystemTime>) -> String {
    age_at(modified, SystemTime::now())
}

fn age_at(modified: Option<SystemTime>, now: SystemTime) -> String {
    let Some(seconds) = modified
        .and_then(|time| now.duration_since(time).ok())
        .map(|elapsed| elapsed.as_secs())
    else {
        return String::new();
    };
    match seconds {
        0..=59 => text::AGE_NOW.to_owned(),
        60..=3_599 => text::age_minutes(seconds / 60),
        3_600..=86_399 => text::age_hours(seconds / 3_600),
        _ => text::age_days(seconds / 86_400),
    }
}

pub(super) fn scope_word(scope: Scope) -> &'static str {
    match scope {
        Scope::Global => text::VALUE_SCOPE_GLOBAL,
        Scope::Project => text::VALUE_SCOPE_PROJECT,
    }
}

/// Writes the row of a script in the list: its name, its age, its scope when the list
/// holds both scopes, and a mark when a console shows it.
pub(super) fn script_row(entry: &ScriptEntry, show_scope: bool, open: bool) -> String {
    let mut row = format!("{:<22} {}", entry.meta.name, age(entry.modified));
    if show_scope {
        row = format!("{row} · {}", scope_word(entry.scope));
    }
    if open {
        row.push_str(text::MARK_OPEN);
    }
    row
}

/// Keeps the saved consoles of one connection.
pub(super) fn saved_of(entries: &[ScriptEntry], key: &ConnectionKey) -> Vec<ScriptEntry> {
    entries
        .iter()
        .filter(|entry| entry.meta.connection_id == key.id)
        .cloned()
        .collect()
}

/// Returns true when a console shows a script.
pub(super) fn shows_script(console: &Console, entry: &ScriptEntry) -> bool {
    console
        .file
        .as_ref()
        .is_some_and(|file| file.scope == entry.scope && file.meta.id == entry.meta.id)
}

/// Returns the names of the other consoles of a connection, open and saved. `skip` is
/// the console that changes its name, and `script` is its saved script.
pub(super) fn names_in_use(
    consoles: &[Console],
    saved: Vec<ScriptEntry>,
    connection: ConnectionId,
    skip: Option<ConsoleId>,
    script: Option<&str>,
) -> Vec<String> {
    let open = consoles
        .iter()
        .filter(|console| console.connection == Some(connection) && Some(console.id) != skip)
        .map(|console| console.name.clone());
    let saved = saved
        .into_iter()
        .filter(|entry| Some(entry.meta.id.as_str()) != script)
        .map(|entry| entry.meta.name);
    open.chain(saved).collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::store::scripts::ScriptMeta;

    fn entry(id: &str, name: &str, connection: &str) -> ScriptEntry {
        let meta = ScriptMeta::new(id.into(), name.into(), connection.into(), Scope::Global);
        ScriptEntry {
            meta,
            scope: Scope::Project,
            modified: None,
        }
    }

    #[test]
    fn the_age_rounds_down_to_its_unit() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let ago = |seconds| age_at(Some(now - Duration::from_secs(seconds)), now);
        assert_eq!(ago(10), text::AGE_NOW);
        assert_eq!(ago(125), text::age_minutes(2));
        assert_eq!(ago(7_200), text::age_hours(2));
        assert_eq!(ago(3 * 86_400), text::age_days(3));
        assert_eq!(age_at(None, now), "");
        assert_eq!(
            age_at(Some(now + Duration::from_secs(5)), now),
            "",
            "a future time"
        );
    }

    #[test]
    fn a_row_shows_the_scope_and_the_open_mark_only_when_asked() {
        let script = entry("s1", "report", "db");
        let plain = script_row(&script, false, false);
        assert!(plain.starts_with("report"));
        assert!(!plain.contains(text::VALUE_SCOPE_PROJECT));
        let full = script_row(&script, true, true);
        assert!(full.contains(text::VALUE_SCOPE_PROJECT), "{full}");
        assert!(full.ends_with(text::MARK_OPEN), "{full}");
    }

    #[test]
    fn a_connection_keeps_only_its_scripts_and_a_rename_skips_its_own_name() {
        let key = ConnectionKey {
            id: "db".to_owned(),
            scope: Scope::Global,
        };
        let entries = [
            entry("s1", "a", "db"),
            entry("s2", "b", "other"),
            entry("s3", "c", "db"),
        ];
        let saved = saved_of(&entries, &key);
        let ids: Vec<_> = saved.iter().map(|entry| entry.meta.id.as_str()).collect();
        assert_eq!(ids, ["s1", "s3"]);
        let names = names_in_use(&[], saved, 1, None, Some("s1"));
        assert_eq!(names, ["c"]);
    }
}
