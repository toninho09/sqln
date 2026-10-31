//! The tree of connections, schemas, relations, and columns.
//!
//! The rows hold positions in the catalog, so a read of the tree copies no name.
//! The cursor holds names, so it stays on its item when the catalog changes.

use std::collections::{HashMap, HashSet};

use crate::catalog::{Columns, RelationKind, RelationRef, Schema};
use crate::connection::{CatalogState, Connection};
use crate::exec::ConnectionId;

pub const DEFAULT_WIDTH: u16 = 24;
pub const MIN_WIDTH: u16 = 12;
const PAGE_ROWS: isize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Connection,
    Schema,
    Table,
    View,
    Column,
}

/// One visible row. The positions index the connection list and its catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub connection: usize,
    pub schema: usize,
    pub relation: usize,
    pub column: usize,
}

impl Row {
    pub fn depth(&self) -> usize {
        match self.kind {
            RowKind::Connection => 0,
            RowKind::Schema => 1,
            RowKind::Table | RowKind::View => 2,
            RowKind::Column => 3,
        }
    }

    fn catalog_schema<'a>(&self, connections: &'a [Connection]) -> Option<&'a Schema> {
        connections[self.connection].catalog.as_ref()?.schemas.get(self.schema)
    }

    /// Returns the name of the row.
    pub fn name<'a>(&self, connections: &'a [Connection]) -> &'a str {
        let schema = self.catalog_schema(connections);
        let relation = schema.and_then(|schema| schema.relations.get(self.relation));
        match self.kind {
            RowKind::Connection => &connections[self.connection].name,
            RowKind::Schema => schema.map_or("", |schema| &schema.name),
            RowKind::Table | RowKind::View => relation.map_or("", |relation| &relation.name),
            RowKind::Column => match relation.map(|relation| &relation.columns) {
                Some(Columns::Loaded(columns)) => columns.get(self.column).map_or("", String::as_str),
                _ => "",
            },
        }
    }

    pub fn relation_ref(&self, connections: &[Connection]) -> Option<RelationRef> {
        let schema = self.catalog_schema(connections)?;
        let relation = schema.relations.get(self.relation)?;
        Some(RelationRef { schema: schema.name.clone(), relation: relation.name.clone() })
    }

    fn key(&self, connections: &[Connection]) -> Key {
        let schema = self.catalog_schema(connections);
        let relation = schema.and_then(|schema| schema.relations.get(self.relation));
        let name = || Some(self.name(connections).to_owned());
        Key {
            connection: connections[self.connection].id,
            schema: (self.kind != RowKind::Connection)
                .then(|| schema.map(|schema| schema.name.clone()))
                .flatten(),
            relation: (self.depth() >= 2)
                .then(|| relation.map(|relation| relation.name.clone()))
                .flatten(),
            column: if self.kind == RowKind::Column { name() } else { None },
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Key {
    connection: ConnectionId,
    schema: Option<String>,
    relation: Option<String>,
    column: Option<String>,
}

impl Key {
    fn depth(&self) -> usize {
        match (&self.schema, &self.relation, &self.column) {
            (None, ..) => 0,
            (_, None, _) => 1,
            (_, _, None) => 2,
            _ => 3,
        }
    }

    /// Returns how many levels of the row match this key. It copies no name.
    fn matching_levels(&self, row: &Row, connections: &[Connection]) -> usize {
        let schema = row.catalog_schema(connections);
        let relation = schema.and_then(|schema| schema.relations.get(row.relation));
        let names = [
            schema.map(|schema| schema.name.as_str()),
            relation.map(|relation| relation.name.as_str()),
            (row.kind == RowKind::Column).then(|| row.name(connections)),
        ];
        let wanted = [self.schema.as_deref(), self.relation.as_deref(), self.column.as_deref()];
        let matching = (0..row.depth()).take_while(|level| names[*level] == wanted[*level]);
        1 + matching.count()
    }
}

/// The open items of one connection. A key of `schemas` is an open schema, and its
/// value holds the open relations of that schema.
#[derive(Debug, Default)]
struct Expansion {
    open: bool,
    schemas: HashMap<String, HashSet<String>>,
}

#[derive(Debug)]
pub struct Navigator {
    pub width: u16,
    pub collapsed: bool,
    /// The first visible row.
    pub top: usize,
    cursor: Key,
    expansion: HashMap<ConnectionId, Expansion>,
}

impl Default for Navigator {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            collapsed: false,
            top: 0,
            cursor: Key::default(),
            expansion: HashMap::new(),
        }
    }
}

impl Navigator {
    pub fn select_connection(&mut self, connection: ConnectionId) {
        self.cursor = Key { connection, ..Key::default() };
    }

    /// Lists the visible rows.
    pub fn rows(&self, connections: &[Connection]) -> Vec<Row> {
        let mut rows = Vec::with_capacity(connections.len());
        for (connection, item) in connections.iter().enumerate() {
            let row = Row { kind: RowKind::Connection, connection, schema: 0, relation: 0, column: 0 };
            rows.push(row);
            let Some(open) = self.expansion.get(&item.id).filter(|open| open.open) else { continue };
            let Some(catalog) = &item.catalog else { continue };
            for (schema, schema_item) in catalog.schemas.iter().enumerate() {
                rows.push(Row { kind: RowKind::Schema, schema, ..row });
                let Some(relations) = open.schemas.get(&schema_item.name) else { continue };
                for (relation, relation_item) in schema_item.relations.iter().enumerate() {
                    let kind = match relation_item.kind {
                        RelationKind::Table => RowKind::Table,
                        RelationKind::View => RowKind::View,
                    };
                    rows.push(Row { kind, schema, relation, ..row });
                    if !relations.contains(&relation_item.name) {
                        continue;
                    }
                    if let Columns::Loaded(columns) = &relation_item.columns {
                        for column in 0..columns.len() {
                            rows.push(Row { kind: RowKind::Column, schema, relation, column, ..row });
                        }
                    }
                }
            }
        }
        rows
    }

    /// Returns the position of the cursor. A removed item gives its closest parent.
    pub fn cursor_index(&self, rows: &[Row], connections: &[Connection]) -> usize {
        let mut best = (0, 0);
        for (index, row) in rows.iter().enumerate() {
            if connections[row.connection].id != self.cursor.connection {
                continue;
            }
            let levels = self.cursor.matching_levels(row, connections);
            if levels == row.depth() + 1 && row.depth() == self.cursor.depth() {
                return index;
            }
            if levels > best.0 {
                best = (levels, index);
            }
        }
        best.1
    }

    pub fn current(&self, connections: &[Connection]) -> Option<Row> {
        let rows = self.rows(connections);
        rows.get(self.cursor_index(&rows, connections)).copied()
    }

    pub fn move_by(&mut self, rows: isize, connections: &[Connection]) {
        let list = self.rows(connections);
        if list.is_empty() {
            return;
        }
        let index = self.cursor_index(&list, connections);
        let target = index.saturating_add_signed(rows).min(list.len() - 1);
        self.cursor = list[target].key(connections);
    }

    pub fn page(&mut self, forward: bool, connections: &[Connection]) {
        self.move_by(if forward { PAGE_ROWS } else { -PAGE_ROWS }, connections);
    }

    pub fn select_index(&mut self, index: usize, connections: &[Connection]) {
        if let Some(row) = self.rows(connections).get(index) {
            self.cursor = row.key(connections);
        }
    }

    pub fn is_open(&self, row: &Row, connections: &[Connection]) -> bool {
        let Some(open) = self.expansion.get(&connections[row.connection].id) else { return false };
        match row.kind {
            RowKind::Connection => open.open,
            RowKind::Schema => open.schemas.contains_key(row.name(connections)),
            RowKind::Table | RowKind::View => {
                let Some(relation) = row.relation_ref(connections) else { return false };
                open.schemas
                    .get(&relation.schema)
                    .is_some_and(|relations| relations.contains(&relation.relation))
            }
            RowKind::Column => false,
        }
    }

    /// Opens a row. The caller reads what the row needs.
    pub fn expand(&mut self, row: &Row, connections: &[Connection]) {
        let open = self.expansion.entry(connections[row.connection].id).or_default();
        match row.kind {
            RowKind::Connection => open.open = true,
            RowKind::Schema => {
                open.schemas.entry(row.name(connections).to_owned()).or_default();
            }
            RowKind::Table | RowKind::View => {
                if let Some(relation) = row.relation_ref(connections) {
                    open.schemas.entry(relation.schema).or_default().insert(relation.relation);
                }
            }
            RowKind::Column => {}
        }
    }

    pub fn collapse(&mut self, row: &Row, connections: &[Connection]) {
        let Some(open) = self.expansion.get_mut(&connections[row.connection].id) else { return };
        match row.kind {
            RowKind::Connection => open.open = false,
            RowKind::Schema => {
                open.schemas.remove(row.name(connections));
            }
            RowKind::Table | RowKind::View => {
                if let Some(relation) = row.relation_ref(connections)
                    && let Some(relations) = open.schemas.get_mut(&relation.schema)
                {
                    relations.remove(&relation.relation);
                }
            }
            RowKind::Column => {}
        }
    }

    /// Moves the cursor to the parent of a row.
    pub fn select_parent(&mut self, row: &Row, connections: &[Connection]) {
        let mut key = row.key(connections);
        match row.kind {
            RowKind::Connection => {}
            RowKind::Schema => key.schema = None,
            RowKind::Table | RowKind::View => key.relation = None,
            RowKind::Column => key.column = None,
        }
        self.cursor = key;
    }

    /// Lists the open relations of a connection. A new catalog needs their columns.
    pub fn open_relations(&self, connection: ConnectionId) -> Vec<RelationRef> {
        let Some(open) = self.expansion.get(&connection) else { return Vec::new() };
        open.schemas
            .iter()
            .flat_map(|(schema, relations)| {
                relations.iter().map(|relation| RelationRef {
                    schema: schema.clone(),
                    relation: relation.clone(),
                })
            })
            .collect()
    }

    pub fn forget(&mut self, connection: ConnectionId) {
        self.expansion.remove(&connection);
    }

    /// Writes the state of a row after its name.
    pub fn suffix(&self, row: &Row, connections: &[Connection]) -> Option<RowNote> {
        match row.kind {
            RowKind::Connection => match connections[row.connection].catalog_state {
                CatalogState::Loading => Some(RowNote::Loading),
                CatalogState::Failed => Some(RowNote::Failed),
                _ => None,
            },
            RowKind::Table | RowKind::View => {
                let relation = row.relation_ref(connections)?;
                connections[row.connection]
                    .requested_columns
                    .contains(&relation)
                    .then_some(RowNote::Loading)
            }
            _ => None,
        }
    }

    /// Returns true when the row can hold children.
    pub fn is_branch(row: &Row) -> bool {
        row.kind != RowKind::Column
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowNote {
    Loading,
    Failed,
}
