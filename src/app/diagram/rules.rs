//! The decisions of the diagram tabs that need no `App`: what a row or a tab gives to a
//! new diagram, which commands work, and what the copy command does.

use crate::catalog::RelationKind;
use crate::connection::{CatalogState, Connection};
use crate::diagram::Focus;
use crate::diagram_tab::DiagramTab;
use crate::exec::ConnectionId;
use crate::input::{Action, Region};
use crate::navigator::{Row, RowKind};
use crate::ui::text;

use super::super::effects::Effect;

/// What a row or a tab gives to a new diagram.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Target {
    Open(ConnectionId, Focus),
    /// The current schema of the connection needs its catalog first.
    Wait(ConnectionId),
}

fn find(connections: &[Connection], id: ConnectionId) -> Option<&Connection> {
    connections.iter().find(|item| item.id == id)
}

/// Returns the diagram of a navigator row, or the reason that it has none. A table shows
/// its neighbors, a column shows the diagram of its table, and a connection shows its
/// current schema.
pub(super) fn row_target(connections: &[Connection], row: &Row) -> Result<Target, &'static str> {
    let item = &connections[row.connection];
    if item.dialect.is_command_line() {
        return Err(text::DIAGRAM_REFUSED);
    }
    let focus = match row.kind {
        RowKind::Connection => return schema_target(connections, item.id, None),
        RowKind::Schema => Focus::Schema(row.name(connections).to_owned()),
        RowKind::Table | RowKind::Column => {
            let relation = row.relation_ref(connections);
            Focus::Table(relation.ok_or(text::DIAGRAM_EMPTY)?)
        }
        RowKind::View => return Err(text::DIAGRAM_VIEW),
        RowKind::Consoles | RowKind::Console => return Err(text::DIAGRAM_CONSOLE),
        RowKind::Group(_) | RowKind::Object(_) => return Err(text::NOT_FOR_OBJECT),
        RowKind::Folder => return Err(text::NOT_FOR_FOLDER),
    };
    focus_target(connections, item.id, focus)
}

/// Refuses the diagram of a view, which has no keys.
pub(super) fn focus_target(
    connections: &[Connection],
    connection: ConnectionId,
    focus: Focus,
) -> Result<Target, &'static str> {
    if let Focus::Table(table) = &focus {
        let catalog = find(connections, connection).and_then(|item| item.catalog.as_ref());
        let found =
            catalog.and_then(|catalog| catalog.relation(Some(&table.schema), &table.relation));
        if found.is_some_and(|(_, relation)| relation.kind == RelationKind::View) {
            return Err(text::DIAGRAM_VIEW);
        }
    }
    Ok(Target::Open(connection, focus))
}

/// Returns the diagram of a chosen schema, or of the current schema of a connection.
/// Before its catalog, a connection does not know its current schema.
pub(super) fn schema_target(
    connections: &[Connection],
    connection: ConnectionId,
    chosen: Option<String>,
) -> Result<Target, &'static str> {
    let item = find(connections, connection).ok_or(text::NO_SELECTED_CONNECTION)?;
    match chosen.or_else(|| item.default_schema.clone()) {
        Some(schema) => Ok(Target::Open(connection, Focus::Schema(schema))),
        None if matches!(
            item.catalog_state,
            CatalogState::NotLoaded | CatalogState::Loading
        ) =>
        {
            Ok(Target::Wait(connection))
        }
        None => Err(text::DIAGRAM_NO_SCHEMA),
    }
}

/// Returns true when an action of the diagram region has an effect on a diagram.
pub(super) fn command_available(tab: &DiagramTab, action: Action) -> bool {
    match action {
        Action::DiagramReload => !tab.is_busy(),
        Action::DiagramOpen | Action::DiagramStructure => tab.selected.is_some(),
        _ => !tab.model.entities.is_empty(),
    }
}

/// Copies the diagram as Mermaid text. An empty diagram copies nothing.
pub(super) fn copy(tab: &DiagramTab) -> Option<Effect> {
    (!tab.model.entities.is_empty()).then(|| Effect::Copy(tab.mermaid()))
}

/// Returns true for an action that a diagram takes and ignores: it has no editor, no
/// results, no transaction, and no schema choice.
pub(super) fn ignores(action: Action) -> bool {
    matches!(
        action,
        Action::Run
            | Action::RunScript
            | Action::RunSubquery
            | Action::Explain
            | Action::ExplainAnalyze
            | Action::FormatSql
            | Action::ToggleComment
            | Action::OpenHistory
            | Action::ToggleMode
            | Action::Commit
            | Action::Rollback
            | Action::ChooseSchema
            | Action::RenameConsole
            | Action::ChangeConnection
            | Action::FocusUp
            | Action::FocusDown
    ) || action.region() == Region::Grid
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::catalog::{Catalog, Relation, RelationRef, Schema};
    use crate::diagram::Entity;
    use crate::sql::Dialect;
    use sqln_testkit::mock::{MockConnector, RunHandler};

    fn connection(id: ConnectionId, dialect: Dialect) -> Connection {
        let run: RunHandler = Arc::new(|_, _| Box::pin(async { Ok(Default::default()) }));
        let connector = MockConnector::new(dialect, run);
        Connection::new(id, format!("c{id}"), None, &connector)
    }

    fn row(kind: RowKind) -> Row {
        Row {
            kind,
            connection: 0,
            schema: 0,
            relation: 0,
            column: 0,
            in_folder: false,
        }
    }

    fn table(name: &str) -> RelationRef {
        RelationRef {
            schema: "main".to_owned(),
            relation: name.to_owned(),
        }
    }

    #[test]
    fn a_connection_waits_for_its_catalog_before_it_knows_its_schema() {
        let mut item = connection(1, Dialect::Sqlite);
        let target = row_target(std::slice::from_ref(&item), &row(RowKind::Connection));
        assert_eq!(target, Ok(Target::Wait(1)));
        item.catalog_state = CatalogState::Loaded;
        let target = row_target(std::slice::from_ref(&item), &row(RowKind::Connection));
        assert_eq!(target, Err(text::DIAGRAM_NO_SCHEMA));
        item.default_schema = Some("main".to_owned());
        let target = row_target(std::slice::from_ref(&item), &row(RowKind::Connection));
        assert_eq!(
            target,
            Ok(Target::Open(1, Focus::Schema("main".to_owned())))
        );
        let chosen = schema_target(&[item], 1, Some("other".to_owned()));
        assert_eq!(
            chosen,
            Ok(Target::Open(1, Focus::Schema("other".to_owned())))
        );
    }

    #[test]
    fn rows_without_a_diagram_give_their_reason() {
        let connections = [connection(1, Dialect::Sqlite)];
        let reason = |kind| row_target(&connections, &row(kind));
        assert_eq!(reason(RowKind::View), Err(text::DIAGRAM_VIEW));
        assert_eq!(reason(RowKind::Console), Err(text::DIAGRAM_CONSOLE));
        assert_eq!(reason(RowKind::Folder), Err(text::NOT_FOR_FOLDER));
        let redis = [connection(1, Dialect::Redis)];
        let refused = row_target(&redis, &row(RowKind::Connection));
        assert_eq!(refused, Err(text::DIAGRAM_REFUSED));
    }

    #[test]
    fn a_view_of_the_catalog_has_no_diagram() {
        let mut item = connection(1, Dialect::Sqlite);
        item.catalog = Some(Catalog {
            schemas: vec![Schema {
                name: "main".to_owned(),
                relations: vec![Relation {
                    name: "totals".to_owned(),
                    kind: RelationKind::View,
                    ..Relation::default()
                }],
                ..Schema::default()
            }],
            current_schema: None,
        });
        let connections = [item];
        let view = focus_target(&connections, 1, Focus::Table(table("totals")));
        assert_eq!(view, Err(text::DIAGRAM_VIEW));
        let other = focus_target(&connections, 1, Focus::Table(table("orders")));
        assert_eq!(other, Ok(Target::Open(1, Focus::Table(table("orders")))));
    }

    #[test]
    fn an_empty_diagram_offers_reload_only_and_copies_nothing() {
        let mut tab = DiagramTab::new(1, 1, Focus::Schema("main".to_owned()), 1);
        assert!(command_available(&tab, Action::DiagramReload));
        assert!(!command_available(&tab, Action::DiagramCopy));
        assert!(!command_available(&tab, Action::DiagramOpen));
        assert_eq!(copy(&tab), None);
        tab.model.entities.push(Entity {
            relation: table("orders"),
            stub: false,
            columns: Vec::new(),
        });
        assert!(command_available(&tab, Action::DiagramCopy));
        assert!(matches!(copy(&tab), Some(Effect::Copy(_))));
    }

    #[test]
    fn a_diagram_ignores_the_commands_of_an_editor() {
        assert!(ignores(Action::Run));
        assert!(ignores(Action::Rollback));
        assert!(!ignores(Action::DiagramReload));
    }
}
