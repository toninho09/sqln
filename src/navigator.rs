//! The tree of connections, consoles, schemas, relations, and columns.
//!
//! The rows hold positions in the catalog, so a read of the tree copies no name.
//! The cursor holds names, so it stays on its item when the catalog changes.
//! A connection keeps its console rows, so a read of the tree does not read the disk.

use std::collections::{HashMap, HashSet};

use crossterm::event::KeyEvent;

use crate::catalog::{Columns, RelationKind, RelationRef, Schema};
use crate::connection::{CatalogState, Connection};
use crate::exec::ConnectionId;
use crate::filter_list::{holds_words, words_of};
use crate::line_input::{Edit, LineInput};
use crate::ui::text;

pub const DEFAULT_WIDTH: u16 = 24;
pub const MIN_WIDTH: u16 = 12;
const PAGE_ROWS: isize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Connection,
    /// The node above the consoles of a connection.
    Consoles,
    /// `Row::relation` is the position in the consoles of the connection.
    Console,
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
            RowKind::Consoles | RowKind::Schema => 1,
            RowKind::Console | RowKind::Table | RowKind::View => 2,
            RowKind::Column => 3,
        }
    }

    /// Returns true for a row without a place in the catalog.
    fn in_consoles(&self) -> bool {
        matches!(self.kind, RowKind::Consoles | RowKind::Console)
    }

    fn catalog_schema<'a>(&self, connections: &'a [Connection]) -> Option<&'a Schema> {
        if self.in_consoles() {
            return None;
        }
        connections[self.connection]
            .catalog
            .as_ref()?
            .schemas
            .get(self.schema)
    }

    /// Returns the name of the row.
    pub fn name<'a>(&self, connections: &'a [Connection]) -> &'a str {
        let schema = self.catalog_schema(connections);
        let relation = schema.and_then(|schema| schema.relations.get(self.relation));
        match self.kind {
            RowKind::Connection => &connections[self.connection].name,
            RowKind::Consoles => text::NODE_CONSOLES,
            RowKind::Console => connections[self.connection]
                .consoles
                .get(self.relation)
                .map_or("", |console| &console.name),
            RowKind::Schema => schema.map_or("", |schema| &schema.name),
            RowKind::Table | RowKind::View => relation.map_or("", |relation| &relation.name),
            RowKind::Column => match relation.map(|relation| &relation.columns) {
                Some(Columns::Loaded(columns)) => {
                    columns.get(self.column).map_or("", String::as_str)
                }
                _ => "",
            },
        }
    }

    pub fn relation_ref(&self, connections: &[Connection]) -> Option<RelationRef> {
        let schema = self.catalog_schema(connections)?;
        let relation = schema.relations.get(self.relation)?;
        Some(RelationRef {
            schema: schema.name.clone(),
            relation: relation.name.clone(),
        })
    }

    fn key(&self, connections: &[Connection]) -> Key {
        if self.in_consoles() {
            return Key {
                connection: connections[self.connection].id,
                consoles: true,
                relation: (self.kind == RowKind::Console)
                    .then(|| self.name(connections).to_owned()),
                ..Key::default()
            };
        }
        let schema = self.catalog_schema(connections);
        let relation = schema.and_then(|schema| schema.relations.get(self.relation));
        let name = || Some(self.name(connections).to_owned());
        Key {
            connection: connections[self.connection].id,
            consoles: false,
            schema: (self.kind != RowKind::Connection)
                .then(|| schema.map(|schema| schema.name.clone()))
                .flatten(),
            relation: (self.depth() >= 2)
                .then(|| relation.map(|relation| relation.name.clone()))
                .flatten(),
            column: if self.kind == RowKind::Column {
                name()
            } else {
                None
            },
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Key {
    connection: ConnectionId,
    /// True on the `Consoles` node and below it. There, `relation` is the console name.
    consoles: bool,
    schema: Option<String>,
    relation: Option<String>,
    column: Option<String>,
}

impl Key {
    fn depth(&self) -> usize {
        if self.consoles {
            return 1 + usize::from(self.relation.is_some());
        }
        match (&self.schema, &self.relation, &self.column) {
            (None, ..) => 0,
            (_, None, _) => 1,
            (_, _, None) => 2,
            _ => 3,
        }
    }

    /// Returns how many levels of the row match this key. It copies no name.
    fn matching_levels(&self, row: &Row, connections: &[Connection]) -> usize {
        if self.consoles != row.in_consoles() {
            return 1;
        }
        if self.consoles {
            let named = row.kind == RowKind::Console
                && self.relation.as_deref() == Some(row.name(connections));
            return 2 + usize::from(named);
        }
        let schema = row.catalog_schema(connections);
        let relation = schema.and_then(|schema| schema.relations.get(row.relation));
        let names = [
            schema.map(|schema| schema.name.as_str()),
            relation.map(|relation| relation.name.as_str()),
            (row.kind == RowKind::Column).then(|| row.name(connections)),
        ];
        let wanted = [
            self.schema.as_deref(),
            self.relation.as_deref(),
            self.column.as_deref(),
        ];
        let matching = (0..row.depth()).take_while(|level| names[*level] == wanted[*level]);
        1 + matching.count()
    }
}

/// The open items of one connection. A key of `schemas` is an open schema, and its
/// value holds the open relations of that schema.
#[derive(Debug, Default)]
struct Expansion {
    open: bool,
    consoles: bool,
    schemas: HashMap<String, HashSet<String>>,
}

/// The filter of the tree. With text, the tree lists only the relations that match, from
/// the catalogs in memory. The filter never opens a connection.
#[derive(Debug)]
struct TreeFilter {
    input: LineInput,
    /// True while the keys write in the field. False while they act on the filtered tree.
    editing: bool,
    /// The cursor before the filter. `Esc` in the field returns to it.
    before: Key,
}

#[derive(Debug)]
pub struct Navigator {
    pub width: u16,
    pub collapsed: bool,
    /// The first visible row.
    pub top: usize,
    cursor: Key,
    expansion: HashMap<ConnectionId, Expansion>,
    filter: Option<TreeFilter>,
}

impl Default for Navigator {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            collapsed: false,
            top: 0,
            cursor: Key::default(),
            expansion: HashMap::new(),
            filter: None,
        }
    }
}

pub fn is_relation(row: &Row) -> bool {
    matches!(row.kind, RowKind::Table | RowKind::View)
}

/// Lists the relations that match, under their connections and schemas. It ignores the
/// open items, because a match can stand under a closed schema.
fn filtered_rows(connections: &[Connection], words: &[String]) -> Vec<Row> {
    let mut rows = Vec::new();
    for (connection, item) in connections.iter().enumerate() {
        let Some(catalog) = &item.catalog else {
            continue;
        };
        let parent = Row {
            kind: RowKind::Connection,
            connection,
            schema: 0,
            relation: 0,
            column: 0,
        };
        let first = rows.len();
        for (schema, schema_item) in catalog.schemas.iter().enumerate() {
            let schema_name = schema_item.name.to_lowercase();
            let matches = schema_item
                .relations
                .iter()
                .enumerate()
                .filter(|(_, relation)| {
                    holds_words(&[&relation.name.to_lowercase(), &schema_name], words)
                });
            let start = rows.len();
            for (relation, relation_item) in matches {
                if rows.len() == start {
                    rows.push(Row {
                        kind: RowKind::Schema,
                        schema,
                        ..parent
                    });
                }
                let kind = match relation_item.kind {
                    RelationKind::Table => RowKind::Table,
                    RelationKind::View => RowKind::View,
                };
                rows.push(Row {
                    kind,
                    schema,
                    relation,
                    ..parent
                });
            }
        }
        if rows.len() > first {
            rows.insert(first, parent);
        }
    }
    rows
}

impl Navigator {
    pub fn select_connection(&mut self, connection: ConnectionId) {
        self.cursor = Key {
            connection,
            ..Key::default()
        };
    }

    /// Lists the visible rows.
    pub fn rows(&self, connections: &[Connection]) -> Vec<Row> {
        if let Some(words) = self.filter_words() {
            return filtered_rows(connections, &words);
        }
        let mut rows = Vec::with_capacity(connections.len());
        for (connection, item) in connections.iter().enumerate() {
            let row = Row {
                kind: RowKind::Connection,
                connection,
                schema: 0,
                relation: 0,
                column: 0,
            };
            rows.push(row);
            let Some(open) = self.expansion.get(&item.id).filter(|open| open.open) else {
                continue;
            };
            if !item.consoles.is_empty() {
                rows.push(Row {
                    kind: RowKind::Consoles,
                    ..row
                });
            }
            if open.consoles {
                rows.extend((0..item.consoles.len()).map(|relation| Row {
                    kind: RowKind::Console,
                    relation,
                    ..row
                }));
            }
            let Some(catalog) = &item.catalog else {
                continue;
            };
            for (schema, schema_item) in catalog.schemas.iter().enumerate() {
                rows.push(Row {
                    kind: RowKind::Schema,
                    schema,
                    ..row
                });
                let Some(relations) = open.schemas.get(&schema_item.name) else {
                    continue;
                };
                for (relation, relation_item) in schema_item.relations.iter().enumerate() {
                    let kind = match relation_item.kind {
                        RelationKind::Table => RowKind::Table,
                        RelationKind::View => RowKind::View,
                    };
                    rows.push(Row {
                        kind,
                        schema,
                        relation,
                        ..row
                    });
                    if !relations.contains(&relation_item.name) {
                        continue;
                    }
                    if let Columns::Loaded(columns) = &relation_item.columns {
                        for column in 0..columns.len() {
                            rows.push(Row {
                                kind: RowKind::Column,
                                schema,
                                relation,
                                column,
                                ..row
                            });
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

    /// Returns the words of the filter. An empty field filters nothing.
    fn filter_words(&self) -> Option<Vec<String>> {
        let words = words_of(self.filter.as_ref()?.input.text());
        (!words.is_empty()).then_some(words)
    }

    /// Returns true when the tree lists only the relations that match.
    pub fn is_filtered(&self) -> bool {
        self.filter
            .as_ref()
            .is_some_and(|filter| !filter.input.text().trim().is_empty())
    }

    /// Returns true while the keys write in the field of the filter.
    pub fn filter_is_editing(&self) -> bool {
        self.filter.as_ref().is_some_and(|filter| filter.editing)
    }

    pub fn filter_input(&self) -> Option<&LineInput> {
        self.filter.as_ref().map(|filter| &filter.input)
    }

    /// Puts the keys in the field of the filter.
    pub fn open_filter(&mut self) {
        match &mut self.filter {
            Some(filter) => filter.editing = true,
            None => {
                self.filter = Some(TreeFilter {
                    input: LineInput::default(),
                    editing: true,
                    before: self.cursor.clone(),
                });
            }
        }
    }

    /// Moves the cursor to the first match. An empty field returns to the first cursor.
    fn after_filter_change(&mut self, connections: &[Connection]) {
        self.top = 0;
        if self.is_filtered() {
            let rows = self.rows(connections);
            if let Some(row) = rows.iter().find(|row| is_relation(row)) {
                self.cursor = row.key(connections);
            }
        } else if let Some(filter) = &self.filter {
            self.cursor = filter.before.clone();
        }
    }

    /// Applies an editing key to the field of the filter.
    pub fn on_filter_key(&mut self, key: KeyEvent, connections: &[Connection]) {
        let Some(filter) = &mut self.filter else {
            return;
        };
        if !matches!(filter.input.on_key(key), Edit::None | Edit::Moved) {
            self.after_filter_change(connections);
        }
    }

    pub fn paste_in_filter(&mut self, text: &str, connections: &[Connection]) {
        if let Some(filter) = &mut self.filter {
            filter.input.insert(text.lines().next().unwrap_or(""));
            self.after_filter_change(connections);
        }
    }

    /// Moves the cursor between the relations of the filtered tree.
    pub fn step_relation(&mut self, forward: bool, connections: &[Connection]) {
        let rows = self.rows(connections);
        if rows.is_empty() {
            return;
        }
        let index = self.cursor_index(&rows, connections);
        let next = if forward {
            rows[index + 1..].iter().find(|row| is_relation(row))
        } else {
            rows[..index].iter().rev().find(|row| is_relation(row))
        };
        if let Some(row) = next {
            self.cursor = row.key(connections);
        }
    }

    /// Sends the keys to the tree. A field without text closes.
    pub fn leave_filter_field(&mut self) {
        if !self.is_filtered() {
            self.cancel_filter();
        } else if let Some(filter) = &mut self.filter {
            filter.editing = false;
        }
    }

    /// Closes the filter, and the cursor returns to its first row.
    pub fn cancel_filter(&mut self) {
        if let Some(filter) = self.filter.take() {
            self.cursor = filter.before;
            self.top = 0;
        }
    }

    /// Closes the filter. The cursor stays, so the parents of its row open.
    pub fn clear_filter(&mut self, connections: &[Connection]) {
        let row = self.current(connections);
        if self.filter.take().is_none() {
            return;
        }
        self.top = 0;
        if let Some(row) = row
            && let Some(schema) = row.catalog_schema(connections)
        {
            let open = self
                .expansion
                .entry(connections[row.connection].id)
                .or_default();
            open.open = true;
            open.schemas.entry(schema.name.clone()).or_default();
        }
    }

    pub fn is_open(&self, row: &Row, connections: &[Connection]) -> bool {
        if self.is_filtered() {
            return matches!(row.kind, RowKind::Connection | RowKind::Schema);
        }
        let Some(open) = self.expansion.get(&connections[row.connection].id) else {
            return false;
        };
        match row.kind {
            RowKind::Connection => open.open,
            RowKind::Consoles => open.consoles,
            RowKind::Schema => open.schemas.contains_key(row.name(connections)),
            RowKind::Table | RowKind::View => {
                let Some(relation) = row.relation_ref(connections) else {
                    return false;
                };
                open.schemas
                    .get(&relation.schema)
                    .is_some_and(|relations| relations.contains(&relation.relation))
            }
            RowKind::Console | RowKind::Column => false,
        }
    }

    /// Opens a row. The caller reads what the row needs.
    pub fn expand(&mut self, row: &Row, connections: &[Connection]) {
        let open = self
            .expansion
            .entry(connections[row.connection].id)
            .or_default();
        match row.kind {
            RowKind::Connection => open.open = true,
            RowKind::Consoles => open.consoles = true,
            RowKind::Schema => {
                open.schemas
                    .entry(row.name(connections).to_owned())
                    .or_default();
            }
            RowKind::Table | RowKind::View => {
                if let Some(relation) = row.relation_ref(connections) {
                    open.schemas
                        .entry(relation.schema)
                        .or_default()
                        .insert(relation.relation);
                }
            }
            RowKind::Console | RowKind::Column => {}
        }
    }

    pub fn collapse(&mut self, row: &Row, connections: &[Connection]) {
        let Some(open) = self.expansion.get_mut(&connections[row.connection].id) else {
            return;
        };
        match row.kind {
            RowKind::Connection => open.open = false,
            RowKind::Consoles => open.consoles = false,
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
            RowKind::Console | RowKind::Column => {}
        }
    }

    /// Moves the cursor to the parent of a row.
    pub fn select_parent(&mut self, row: &Row, connections: &[Connection]) {
        let mut key = row.key(connections);
        match row.kind {
            RowKind::Connection => {}
            RowKind::Consoles => key.consoles = false,
            RowKind::Schema => key.schema = None,
            RowKind::Console | RowKind::Table | RowKind::View => key.relation = None,
            RowKind::Column => key.column = None,
        }
        self.cursor = key;
    }

    /// Returns true when the tree shows the rows below a connection.
    pub fn is_connection_open(&self, connection: ConnectionId) -> bool {
        self.expansion
            .get(&connection)
            .is_some_and(|open| open.open)
    }

    /// Keeps the cursor on a console after a new name.
    pub fn rename_console(&mut self, old: &str, new: &str) {
        if self.cursor.consoles && self.cursor.relation.as_deref() == Some(old) {
            self.cursor.relation = Some(new.to_owned());
        }
    }

    /// Lists the open relations of a connection. A new catalog needs their columns.
    pub fn open_relations(&self, connection: ConnectionId) -> Vec<RelationRef> {
        let Some(open) = self.expansion.get(&connection) else {
            return Vec::new();
        };
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
            RowKind::Schema => row
                .catalog_schema(connections)?
                .partial
                .then_some(RowNote::Partial),
            RowKind::Table | RowKind::View => {
                let relation = row.relation_ref(connections)?;
                connections[row.connection]
                    .requested_columns
                    .contains(&relation)
                    .then_some(RowNote::Loading)
            }
            RowKind::Console => connections[row.connection]
                .consoles
                .get(row.relation)?
                .tab
                .map(|_| RowNote::Open),
            _ => None,
        }
    }

    /// Returns true when the row can hold children.
    pub fn is_branch(row: &Row) -> bool {
        !matches!(row.kind, RowKind::Console | RowKind::Column)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowNote {
    Loading,
    Failed,
    /// The list of a schema holds only a part of its relations.
    Partial,
    /// A tab shows the console of the row.
    Open,
}
