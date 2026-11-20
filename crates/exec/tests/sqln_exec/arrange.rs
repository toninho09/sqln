//! The order of the saved connections and their folders.

use std::path::PathBuf;
use std::sync::Arc;

use sqln_exec::arrange::{
    self, Arrangement, Item, Target, drop_at_end, drop_on, folder_label, move_by, move_to_folder,
    rename_folder, top_items, ungroup,
};
use sqln_exec::connection::{Connection, ConnectionKey, Definition, Source};
use sqln_session::connector::sqlite::SqliteTarget;
use sqln_sql::sql::Dialect;
use sqln_store::store::Scope;
use sqln_store::store::config::FolderRef;
use sqln_testkit::mock::MockConnector;

/// Builds the connections of a list of `(scope, id, folder)`.
fn connections(items: &[(Scope, &str, Option<&str>)]) -> Vec<Connection> {
    let connector = MockConnector::new(
        Dialect::Sqlite,
        Arc::new(|_, _| Box::pin(async { unreachable!("no statement runs") })),
    );
    items
        .iter()
        .enumerate()
        .map(|(index, (scope, id, folder))| {
            let mut definition = Definition::new(
                *id,
                Source::Sqlite(SqliteTarget::new(PathBuf::from(format!("{id}.db")))),
            );
            definition.folder = folder.map(str::to_owned);
            let mut connection =
                Connection::new(index as u64, (*id).to_owned(), Some(definition), &connector);
            connection.key = Some(ConnectionKey {
                scope: *scope,
                id: (*id).to_owned(),
            });
            connection
        })
        .collect()
}

fn order(arrangement: &Arrangement) -> Vec<(&str, Option<&str>)> {
    arrangement
        .order
        .iter()
        .map(|(id, folder)| (id.as_str(), folder.as_deref()))
        .collect()
}

fn folder(scope: Scope, name: &str) -> FolderRef {
    FolderRef {
        scope,
        name: name.to_owned(),
    }
}

const G: Scope = Scope::Global;
const P: Scope = Scope::Project;

#[test]
fn a_folder_stands_at_its_first_connection_and_gathers_the_others() {
    let list = connections(&[
        (G, "a", Some("f")),
        (G, "b", None),
        (G, "c", Some("f")),
        (P, "d", Some("f")),
    ]);
    assert_eq!(
        top_items(&list),
        vec![
            Item::Folder {
                folder: folder(G, "f"),
                members: vec![0, 2],
            },
            Item::Connection(1),
            Item::Folder {
                folder: folder(P, "f"),
                members: vec![3],
            },
        ]
    );
    assert_eq!(folder_label(&list, &folder(G, "f")), "f (global)");
    assert_eq!(folder_label(&list, &folder(P, "f")), "f (local)");
    assert_eq!(arrange::folder_names(&list, G), vec!["f".to_owned()]);
}

#[test]
fn a_move_swaps_neighbors_at_the_same_level() {
    let list = connections(&[
        (G, "a", None),
        (G, "b", Some("f")),
        (G, "c", Some("f")),
        (G, "d", None),
    ]);
    let down = move_by(&list, &Target::Connection(0), false).unwrap();
    assert_eq!(
        order(&down),
        [("b", Some("f")), ("c", Some("f")), ("a", None), ("d", None)]
    );
    let inside = move_by(&list, &Target::Connection(2), true).unwrap();
    assert_eq!(
        order(&inside),
        [("a", None), ("c", Some("f")), ("b", Some("f")), ("d", None)]
    );
    let block = move_by(&list, &Target::Folder(folder(G, "f")), false).unwrap();
    assert_eq!(
        order(&block),
        [("a", None), ("d", None), ("b", Some("f")), ("c", Some("f"))]
    );
}

#[test]
fn a_move_past_an_edge_refuses_with_a_reason() {
    let list = connections(&[(G, "a", Some("f")), (G, "b", Some("f")), (P, "c", None)]);
    assert_eq!(
        move_by(&list, &Target::Connection(0), true),
        Err(sqln_core::text::MOVE_FOLDER_EDGE)
    );
    assert_eq!(
        move_by(&list, &Target::Folder(folder(G, "f")), false),
        Err(sqln_core::text::MOVE_SCOPE_EDGE)
    );
    assert_eq!(
        move_by(&list, &Target::Connection(2), true),
        Err(sqln_core::text::MOVE_SCOPE_EDGE)
    );
}

#[test]
fn a_connection_enters_and_leaves_a_folder() {
    let list = connections(&[(G, "a", None), (G, "b", Some("f")), (G, "c", None)]);
    let into = move_to_folder(&list, 2, Some("f")).unwrap();
    assert_eq!(
        order(&into),
        [("a", None), ("b", Some("f")), ("c", Some("f"))]
    );
    let new = move_to_folder(&list, 0, Some("g")).unwrap();
    assert_eq!(
        order(&new),
        [("a", Some("g")), ("b", Some("f")), ("c", None)]
    );
    // The folder empties and leaves; the connection takes its place.
    let out = move_to_folder(&list, 1, None).unwrap();
    assert_eq!(order(&out), [("a", None), ("b", None), ("c", None)]);
}

#[test]
fn a_member_that_leaves_goes_after_its_folder() {
    let list = connections(&[(G, "a", Some("f")), (G, "b", Some("f")), (G, "c", None)]);
    let out = move_to_folder(&list, 0, None).unwrap();
    assert_eq!(order(&out), [("b", Some("f")), ("a", None), ("c", None)]);
    let new = move_to_folder(&list, 0, Some("g")).unwrap();
    assert_eq!(
        order(&new),
        [("b", Some("f")), ("a", Some("g")), ("c", None)]
    );
}

#[test]
fn a_rename_to_a_used_name_joins_the_folders() {
    let list = connections(&[(G, "a", Some("f")), (G, "b", None), (G, "c", Some("g"))]);
    let plain = rename_folder(&list, &folder(G, "f"), "h").unwrap();
    assert_eq!(
        order(&plain),
        [("a", Some("h")), ("b", None), ("c", Some("g"))]
    );
    let joined = rename_folder(&list, &folder(G, "g"), "f").unwrap();
    assert_eq!(
        order(&joined),
        [("a", Some("f")), ("c", Some("f")), ("b", None)]
    );
}

#[test]
fn ungroup_keeps_the_connections_in_place() {
    let list = connections(&[(G, "a", None), (G, "b", Some("f")), (G, "c", Some("f"))]);
    let flat = ungroup(&list, &folder(G, "f")).unwrap();
    assert_eq!(order(&flat), [("a", None), ("b", None), ("c", None)]);
}

#[test]
fn a_drop_takes_the_place_of_the_target() {
    let list = connections(&[
        (G, "a", None),
        (G, "b", Some("f")),
        (G, "c", Some("f")),
        (G, "d", None),
    ]);
    let up = drop_on(&list, &Target::Connection(3), &Target::Connection(2)).unwrap();
    assert_eq!(
        order(&up),
        [
            ("a", None),
            ("b", Some("f")),
            ("d", Some("f")),
            ("c", Some("f"))
        ]
    );
    let down = drop_on(&list, &Target::Connection(0), &Target::Connection(3)).unwrap();
    assert_eq!(
        order(&down),
        [("b", Some("f")), ("c", Some("f")), ("d", None), ("a", None)]
    );
    let into = drop_on(
        &list,
        &Target::Connection(0),
        &Target::Folder(folder(G, "f")),
    )
    .unwrap();
    assert_eq!(
        order(&into),
        [
            ("b", Some("f")),
            ("c", Some("f")),
            ("a", Some("f")),
            ("d", None)
        ]
    );
    let block = drop_on(
        &list,
        &Target::Folder(folder(G, "f")),
        &Target::Connection(0),
    )
    .unwrap();
    assert_eq!(
        order(&block),
        [("b", Some("f")), ("c", Some("f")), ("a", None), ("d", None)]
    );
    let same = drop_on(
        &list,
        &Target::Folder(folder(G, "f")),
        &Target::Connection(2),
    )
    .unwrap();
    assert_eq!(
        order(&same),
        [("a", None), ("b", Some("f")), ("c", Some("f")), ("d", None)]
    );
}

#[test]
fn a_drop_on_its_own_folder_takes_a_connection_out_at_the_place_of_the_folder() {
    let list = connections(&[(G, "a", None), (G, "b", Some("f")), (G, "c", Some("f"))]);
    let out = drop_on(
        &list,
        &Target::Connection(2),
        &Target::Folder(folder(G, "f")),
    )
    .unwrap();
    assert_eq!(order(&out), [("a", None), ("c", None), ("b", Some("f"))]);
    // The last member leaves, and the folder goes away.
    let only = connections(&[(G, "a", Some("f")), (G, "b", None)]);
    let out = drop_on(
        &only,
        &Target::Connection(0),
        &Target::Folder(folder(G, "f")),
    )
    .unwrap();
    assert_eq!(order(&out), [("a", None), ("b", None)]);
}

#[test]
fn a_drop_at_the_end_puts_an_item_last_in_its_scope_at_the_top_level() {
    let list = connections(&[
        (G, "a", Some("f")),
        (G, "b", Some("f")),
        (G, "c", None),
        (P, "d", None),
    ]);
    let out = drop_at_end(&list, &Target::Connection(0)).unwrap();
    assert_eq!(out.scope, G);
    assert_eq!(order(&out), [("b", Some("f")), ("c", None), ("a", None)]);
    let block = drop_at_end(&list, &Target::Folder(folder(G, "f"))).unwrap();
    assert_eq!(
        order(&block),
        [("c", None), ("a", Some("f")), ("b", Some("f"))]
    );
}

#[test]
fn a_drop_on_the_other_scope_refuses() {
    let list = connections(&[(G, "a", None), (P, "b", None)]);
    assert_eq!(
        drop_on(&list, &Target::Connection(0), &Target::Connection(1)),
        Err(sqln_core::text::MOVE_OTHER_SCOPE)
    );
}

#[test]
fn a_connection_without_a_file_cannot_move() {
    let mut list = connections(&[(G, "a", None)]);
    list[0].key = None;
    assert_eq!(
        move_by(&list, &Target::Connection(0), true),
        Err(sqln_core::text::MOVE_NOT_SAVED)
    );
}
