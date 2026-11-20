//! The decisions of the compare tabs that need no `App`: which row gives a source, which
//! connections the dialog lists, what the dialog shows for each side, which table it
//! accepts, which commands work, and what a command of the script does. The glue in
//! `mod.rs` reads the state of `App`, calls these functions, and applies their effects.

use crate::catalog::{Catalog, RelationKind};
use crate::compare::Side;
use crate::compare::form::{Choice, SchemaChoice};
use crate::compare_tab::CompareTab;
use crate::connection::{CatalogState, Connection};
use crate::exec::ConnectionId;
use crate::input::{Action, Grid};
use crate::navigator::{Row, RowKind};
use crate::ui::text;

use super::super::effects::Effect;

/// The source that a row gives to the dialog: a connection, a schema, and a table.
pub(super) type Source = (ConnectionId, Option<String>, Option<String>);

/// Returns the source of a compare for a row of the explorer, or the reason that the row
/// has none. A column gives its table.
pub(super) fn row_source(connections: &[Connection], row: &Row) -> Result<Source, &'static str> {
    let item = &connections[row.connection];
    if item.dialect.is_command_line() {
        return Err(text::COMPARE_REFUSED);
    }
    match row.kind {
        RowKind::Connection => Ok((item.id, item.default_schema.clone(), None)),
        RowKind::Schema => Ok((item.id, Some(row.name(connections).to_owned()), None)),
        RowKind::Table | RowKind::Column => {
            let relation = row.relation_ref(connections).ok_or(text::COMPARE_REFUSED)?;
            Ok((item.id, Some(relation.schema), Some(relation.relation)))
        }
        RowKind::View => Err(text::COMPARE_VIEW),
        RowKind::Consoles | RowKind::Console => Err(text::COMPARE_CONSOLE),
        RowKind::Group(_) | RowKind::Object(_) => Err(text::NOT_FOR_OBJECT),
        RowKind::Folder => Err(text::NOT_FOR_FOLDER),
    }
}

/// Lists the connections that a compare can use. Redis and Kafka have no tables.
pub(super) fn choices(connections: &[Connection]) -> Vec<Choice> {
    connections
        .iter()
        .filter(|item| !item.dialect.is_command_line())
        .map(|item| Choice {
            id: item.id,
            name: item.name.clone(),
            dialect: item.dialect,
        })
        .collect()
}

/// What the dialog shows for the schemas of one side.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SchemaSync {
    /// The schemas and their tables, with the default schema of the connection.
    Show(Vec<SchemaChoice>, Option<String>),
    /// The catalog read failed: the side has no schema to choose.
    Nothing,
    /// The catalog is not loaded yet.
    Read,
}

/// Returns what the dialog shows for a connection. Views stay out, because a compare
/// refuses them.
pub(super) fn schema_sync(item: &Connection) -> SchemaSync {
    match (&item.catalog, item.catalog_state) {
        (Some(catalog), CatalogState::Loaded) => {
            let schemas = catalog.schemas.iter().map(|schema| SchemaChoice {
                name: schema.name.clone(),
                tables: schema
                    .relations
                    .iter()
                    .filter(|relation| relation.kind == RelationKind::Table)
                    .map(|relation| relation.name.clone())
                    .collect(),
            });
            SchemaSync::Show(schemas.collect(), item.default_schema.clone())
        }
        (_, CatalogState::Failed) => SchemaSync::Nothing,
        _ => SchemaSync::Read,
    }
}

/// Refuses a source table that the catalog does not hold, and a view.
pub(super) fn check_table(catalog: Option<&Catalog>, side: &Side) -> Result<(), String> {
    let Some(table) = &side.table else {
        return Ok(());
    };
    match catalog.and_then(|catalog| catalog.relation(Some(&side.schema), table)) {
        Some((_, relation)) if relation.kind == RelationKind::View => {
            Err(text::COMPARE_VIEW.to_owned())
        }
        Some(_) => Ok(()),
        None => Err(text::compare_no_table(table)),
    }
}

/// Returns the script of a compare, or the reason that it has none.
pub(super) fn script(tab: Option<&CompareTab>) -> Result<String, &'static str> {
    let tab = tab.ok_or(text::COMPARE_NOT_READ)?;
    if !tab.done {
        return Err(text::COMPARE_NOT_READ);
    }
    if tab.count == 0 {
        return Err(text::COMPARE_NO_SCRIPT);
    }
    if tab.script.is_empty() {
        return Err(text::COMPARE_NO_SCRIPT_DIALECTS);
    }
    Ok(tab.script.clone())
}

/// Copies the script of the compare.
pub(super) fn copy_script(tab: Option<&CompareTab>) -> Effect {
    match script(tab) {
        Ok(script) => Effect::Copy(script),
        Err(reason) => Effect::refused(reason),
    }
}

/// Opens the script in a new console of the target. It does not run the script.
pub(super) fn open_script(tab: Option<&CompareTab>) -> Option<Effect> {
    let script = match script(tab) {
        Ok(script) => script,
        Err(reason) => return Some(Effect::refused(reason)),
    };
    let target = &tab?.sides[1].side;
    Some(Effect::OpenTextConsole {
        connection: target.connection,
        schema: Some(target.schema.clone()),
        text: script,
    })
}

/// Returns true when an action of the grid has an effect on a compare.
pub(super) fn command_available(tab: &CompareTab, action: Action, can_export: bool) -> bool {
    match action {
        _ if !action.on_grid(Grid::Compare) => false,
        Action::CompareScript | Action::CompareCopy => script(Some(tab)).is_ok(),
        Action::GridReload => !tab.is_busy(),
        Action::GridOpenRow => !tab.result.table.rows.is_empty(),
        Action::GridExport => can_export,
        _ => tab.done,
    }
}

/// Returns true for an action that a compare takes and ignores: it has no editor, no
/// transaction, no filter, and one result.
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
    ) || !action.on_grid(Grid::Compare)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::catalog::{Relation, Schema};
    use crate::compare::Kind;
    use crate::compare_tab::SideRead;
    use crate::sql::Dialect;
    use sqln_testkit::mock::{MockConnector, RunHandler};

    fn connection(id: ConnectionId, dialect: Dialect) -> Connection {
        let run: RunHandler = Arc::new(|_, _| Box::pin(async { Ok(Default::default()) }));
        let connector = MockConnector::new(dialect, run);
        let mut item = Connection::new(id, format!("c{id}"), None, &connector);
        item.default_schema = Some("main".to_owned());
        item
    }

    fn row(kind: RowKind, connection: usize) -> Row {
        Row {
            kind,
            connection,
            schema: 0,
            relation: 0,
            column: 0,
            in_folder: false,
        }
    }

    fn side(connection: ConnectionId, table: Option<&str>) -> Side {
        Side {
            connection,
            schema: "main".to_owned(),
            table: table.map(str::to_owned),
        }
    }

    fn catalog() -> Catalog {
        let relation = |name: &str, kind| Relation {
            name: name.to_owned(),
            kind,
            ..Relation::default()
        };
        Catalog {
            schemas: vec![Schema {
                name: "main".to_owned(),
                relations: vec![
                    relation("orders", RelationKind::Table),
                    relation("totals", RelationKind::View),
                ],
                ..Schema::default()
            }],
            current_schema: Some("main".to_owned()),
        }
    }

    fn tab() -> CompareTab {
        let read = |id: u64, connection: ConnectionId| {
            SideRead::new(id, side(connection, None), Dialect::Sqlite, "c".into())
        };
        CompareTab::new(1, Kind::Structure, [read(1, 1), read(2, 2)], false)
    }

    #[test]
    fn a_connection_row_gives_its_default_schema_and_other_rows_give_a_reason() {
        let connections = vec![
            connection(1, Dialect::Sqlite),
            connection(2, Dialect::Redis),
        ];
        let source = row_source(&connections, &row(RowKind::Connection, 0));
        assert_eq!(source, Ok((1, Some("main".to_owned()), None)));
        let view = row_source(&connections, &row(RowKind::View, 0));
        assert_eq!(view, Err(text::COMPARE_VIEW));
        let console = row_source(&connections, &row(RowKind::Console, 0));
        assert_eq!(console, Err(text::COMPARE_CONSOLE));
        let folder = row_source(&connections, &row(RowKind::Folder, 0));
        assert_eq!(folder, Err(text::NOT_FOR_FOLDER));
        let redis = row_source(&connections, &row(RowKind::Connection, 1));
        assert_eq!(redis, Err(text::COMPARE_REFUSED));
    }

    #[test]
    fn the_dialog_lists_only_connections_with_tables() {
        let connections = vec![
            connection(1, Dialect::Sqlite),
            connection(2, Dialect::Kafka),
            connection(3, Dialect::Postgres),
        ];
        let ids: Vec<_> = choices(&connections)
            .iter()
            .map(|choice| choice.id)
            .collect();
        assert_eq!(ids, [1, 3]);
    }

    #[test]
    fn the_dialog_shows_tables_without_views_once_the_catalog_is_loaded() {
        let mut item = connection(1, Dialect::Sqlite);
        assert_eq!(schema_sync(&item), SchemaSync::Read);
        item.catalog_state = CatalogState::Failed;
        assert_eq!(schema_sync(&item), SchemaSync::Nothing);
        item.catalog = Some(catalog());
        item.catalog_state = CatalogState::Loaded;
        let SchemaSync::Show(schemas, default) = schema_sync(&item) else {
            panic!("the schemas show");
        };
        assert_eq!(default.as_deref(), Some("main"));
        assert_eq!(schemas[0].tables, ["orders"]);
    }

    #[test]
    fn a_source_table_must_exist_and_must_not_be_a_view() {
        let catalog = catalog();
        assert_eq!(check_table(Some(&catalog), &side(1, None)), Ok(()));
        assert_eq!(
            check_table(Some(&catalog), &side(1, Some("orders"))),
            Ok(())
        );
        let view = check_table(Some(&catalog), &side(1, Some("totals")));
        assert_eq!(view, Err(text::COMPARE_VIEW.to_owned()));
        let missing = check_table(Some(&catalog), &side(1, Some("nowhere")));
        assert_eq!(missing, Err(text::compare_no_table("nowhere")));
        let unread = check_table(None, &side(1, Some("orders")));
        assert_eq!(unread, Err(text::compare_no_table("orders")));
    }

    #[test]
    fn the_script_commands_refuse_until_the_compare_finds_a_difference() {
        let mut tab = tab();
        assert_eq!(copy_script(None), Effect::refused(text::COMPARE_NOT_READ));
        assert_eq!(
            copy_script(Some(&tab)),
            Effect::refused(text::COMPARE_NOT_READ)
        );
        tab.done = true;
        assert_eq!(
            copy_script(Some(&tab)),
            Effect::refused(text::COMPARE_NO_SCRIPT)
        );
        tab.count = 1;
        let refused = Effect::refused(text::COMPARE_NO_SCRIPT_DIALECTS);
        assert_eq!(copy_script(Some(&tab)), refused);
        tab.script = "ALTER TABLE t ADD c int;".to_owned();
        assert_eq!(copy_script(Some(&tab)), Effect::Copy(tab.script.clone()));
        assert_eq!(
            open_script(Some(&tab)),
            Some(Effect::OpenTextConsole {
                connection: 2,
                schema: Some("main".to_owned()),
                text: tab.script.clone(),
            }),
            "the script opens on the target",
        );
        assert!(command_available(&tab, Action::CompareCopy, false));
    }

    #[test]
    fn a_compare_ignores_the_commands_of_an_editor() {
        assert!(ignores(Action::Run));
        assert!(ignores(Action::Commit));
        assert!(!ignores(Action::GridReload));
        assert!(!ignores(Action::CompareScript));
    }
}
