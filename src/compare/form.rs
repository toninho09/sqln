//! The dialog of a compare. It holds the two sides and the choice of removals, and it
//! reads nothing. The application gives it the schemas and the tables of each connection.
//! A connection, a schema, and a table field show a list of names that a text filters.

use crossterm::event::{KeyCode, KeyEvent};

use super::{Kind, Side};
use crate::exec::ConnectionId;
use crate::filter_list::{holds_words, words_of};
use crate::line_input::{Edit, LineInput};
use crate::sql::Dialect;
use crate::ui::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Kind,
    SourceConnection,
    SourceSchema,
    SourceTable,
    TargetConnection,
    TargetSchema,
    TargetTable,
    Removals,
    Compare,
    Cancel,
}

impl Field {
    pub const ALL: [Self; 10] = [
        Self::Kind,
        Self::SourceConnection,
        Self::SourceSchema,
        Self::SourceTable,
        Self::TargetConnection,
        Self::TargetSchema,
        Self::TargetTable,
        Self::Removals,
        Self::Compare,
        Self::Cancel,
    ];

    /// Returns the side of a field of a side: 0 for the source, 1 for the target.
    pub fn side(self) -> Option<usize> {
        match self {
            Self::SourceConnection | Self::SourceSchema | Self::SourceTable => Some(0),
            Self::TargetConnection | Self::TargetSchema | Self::TargetTable => Some(1),
            _ => None,
        }
    }

    /// True for a field with a list of names: a connection, a schema, or a table.
    pub fn has_list(self) -> bool {
        self.side().is_some()
    }

    fn is_table(self) -> bool {
        matches!(self, Self::SourceTable | Self::TargetTable)
    }

    fn is_connection(self) -> bool {
        matches!(self, Self::SourceConnection | Self::TargetConnection)
    }
}

/// A connection that the dialog offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub id: ConnectionId,
    pub name: String,
    pub dialect: Dialect,
}

/// A schema that the dialog offers, with the names of its tables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaChoice {
    pub name: String,
    pub tables: Vec<String>,
}

/// The choices of one side.
pub struct FormSide {
    /// The place of the connection in the list of the dialog.
    pub connection: usize,
    /// The schemas of the catalog. `None` while the catalog is not loaded.
    pub schemas: Option<Vec<SchemaChoice>>,
    pub schema: usize,
    pub table: LineInput,
    /// The schema to choose when the catalog arrives.
    wanted: Option<String>,
}

impl FormSide {
    fn new(connection: usize, schema: Option<String>, table: &str) -> Self {
        let mut input = LineInput::default();
        input.set(table);
        Self {
            connection,
            schemas: None,
            schema: 0,
            table: input,
            wanted: schema,
        }
    }

    /// Returns the chosen schema. Without its catalog, a side has no schema.
    pub fn schema_name(&self) -> Option<&str> {
        self.chosen_schema().map(|schema| schema.name.as_str())
    }

    fn chosen_schema(&self) -> Option<&SchemaChoice> {
        self.schemas
            .as_ref()
            .and_then(|schemas| schemas.get(self.schema))
    }

    /// Returns the tables of the chosen schema.
    fn tables(&self) -> &[String] {
        self.chosen_schema()
            .map(|schema| schema.tables.as_slice())
            .unwrap_or_default()
    }
}

/// The list of the focused field: the names that its text keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldList {
    /// Each name with its place in the full list.
    pub names: Vec<(usize, String)>,
    /// The marked name, as a position in `names`.
    pub selected: Option<usize>,
}

impl FieldList {
    /// Keeps the names that hold each word of the text. The mark goes to the name equal to
    /// the text, or, without a text, to the current choice. A text without an equal name
    /// marks the first name. An empty text without a choice marks nothing.
    fn new<'a>(names: impl Iterator<Item = &'a str>, text: &str, current: Option<&str>) -> Self {
        let words = words_of(text);
        let names: Vec<(usize, String)> = names
            .enumerate()
            .filter(|(_, name)| holds_words(&[&name.to_lowercase()], &words))
            .map(|(place, name)| (place, name.to_owned()))
            .collect();
        let wanted = if text.is_empty() { current } else { Some(text) };
        let selected = wanted
            .and_then(|wanted| names.iter().position(|(_, name)| name == wanted))
            .or_else(|| (!text.is_empty() && !names.is_empty()).then_some(0));
        Self { names, selected }
    }

    /// True when the list has a name to show.
    pub fn is_shown(&self) -> bool {
        !self.names.is_empty()
    }

    fn chosen(&self) -> Option<&(usize, String)> {
        self.selected.and_then(|place| self.names.get(place))
    }

    /// Moves the mark one row. It goes around at the ends.
    fn move_mark(&mut self, forward: bool) {
        let count = self.names.len();
        if count == 0 {
            return;
        }
        self.selected = Some(match self.selected {
            None if forward => 0,
            None => count - 1,
            Some(place) if forward => (place + 1) % count,
            Some(place) => (place + count - 1) % count,
        });
    }
}

pub enum CompareRequest {
    None,
    Cancel,
    Compare,
}

pub struct CompareForm {
    pub focus: Field,
    pub kind: Kind,
    pub connections: Vec<Choice>,
    pub sides: [FormSide; 2],
    pub removals: bool,
    pub note: Option<String>,
    /// The search text of a connection or a schema field. A table field searches with
    /// the name in its text.
    pub search: LineInput,
    /// The list of the focused field. `None` while the list is closed.
    pub list: Option<FieldList>,
}

impl CompareForm {
    /// Opens the dialog with the source of a row. The target is the next connection, or
    /// the same connection when it is alone, and it starts with the same table. A table
    /// starts a data compare, and a schema starts a structure compare.
    pub fn new(
        connections: Vec<Choice>,
        source: ConnectionId,
        schema: Option<String>,
        table: Option<String>,
    ) -> Self {
        let first = connections
            .iter()
            .position(|choice| choice.id == source)
            .unwrap_or(0);
        let second = if connections.len() > 1 {
            (first + 1) % connections.len()
        } else {
            first
        };
        let kind = if table.is_some() {
            Kind::Data
        } else {
            Kind::Structure
        };
        let table = table.unwrap_or_default();
        let mut form = Self {
            focus: Field::TargetConnection,
            kind,
            sides: [
                FormSide::new(first, schema.clone(), &table),
                FormSide::new(second, schema, &table),
            ],
            connections,
            removals: false,
            note: None,
            search: LineInput::default(),
            list: None,
        };
        form.open_list();
        form
    }

    /// Opens the dialog with the sides of an open compare.
    pub fn again(connections: Vec<Choice>, kind: Kind, sides: &[Side; 2], removals: bool) -> Self {
        let [source, target] = sides;
        let mut form = Self::new(
            connections,
            source.connection,
            Some(source.schema.clone()),
            source.table.clone(),
        );
        if let Some(place) = form
            .connections
            .iter()
            .position(|choice| choice.id == target.connection)
        {
            form.sides[1] = FormSide::new(
                place,
                Some(target.schema.clone()),
                target.table.as_deref().unwrap_or_default(),
            );
        }
        form.kind = kind;
        form.removals = removals;
        form.focus_on(Field::Compare);
        form
    }

    pub fn choice(&self, side: usize) -> Option<&Choice> {
        self.connections.get(self.sides[side].connection)
    }

    /// Gives the schemas of the catalog of a side. The chosen name stays when it is in
    /// the list. A new list chooses the wanted schema, then the default, then the first.
    pub fn set_schemas(&mut self, side: usize, schemas: Vec<SchemaChoice>, default: Option<&str>) {
        let form = &mut self.sides[side];
        let chosen = form
            .schema_name()
            .map(str::to_owned)
            .or_else(|| form.wanted.take());
        let place = |name: Option<&str>| {
            name.and_then(|name| schemas.iter().position(|schema| schema.name == name))
        };
        form.schema = place(chosen.as_deref()).or(place(default)).unwrap_or(0);
        form.schemas = Some(schemas);
        if self.focus.side() == Some(side) && !self.focus.is_connection() {
            self.refresh_list();
        }
    }

    /// True when the focused field shows its list.
    pub fn list_shown(&self) -> bool {
        self.list.as_ref().is_some_and(FieldList::is_shown)
    }

    pub fn on_key(&mut self, key: KeyEvent, vim: bool) -> CompareRequest {
        self.note = None;
        if self.on_list_key(key) {
            return CompareRequest::None;
        }
        let list = self.focus.has_list();
        let text = self.focus.is_table();
        match key.code {
            KeyCode::Esc => return CompareRequest::Cancel,
            KeyCode::Enter if self.focus == Field::Cancel => return CompareRequest::Cancel,
            KeyCode::Enter if self.focus == Field::Compare => return CompareRequest::Compare,
            KeyCode::Enter | KeyCode::Tab | KeyCode::Down => self.move_focus(true),
            KeyCode::BackTab | KeyCode::Up => self.move_focus(false),
            KeyCode::Char('j') if vim && !list => self.move_focus(true),
            KeyCode::Char('k') if vim && !list => self.move_focus(false),
            KeyCode::Left if !text => self.change(false),
            KeyCode::Right if !text => self.change(true),
            KeyCode::Char(' ') if !list => self.change(true),
            KeyCode::Char('h') if vim && !list => self.change(false),
            KeyCode::Char('l') if vim && !list => self.change(true),
            _ => self.edit(key),
        }
        CompareRequest::None
    }

    /// Handles a key of the list. `Esc` closes it, the arrows move the mark, and `Enter`
    /// or `Tab` choose the marked name. It returns false when the form must act.
    fn on_list_key(&mut self, key: KeyEvent) -> bool {
        let shown = self.list_shown();
        let marked = self.list.as_ref().and_then(FieldList::chosen).is_some();
        match key.code {
            KeyCode::Esc if shown || !self.search.text().is_empty() => {
                self.list = None;
                self.search = LineInput::default();
            }
            KeyCode::Up | KeyCode::Down if shown => {
                if let Some(list) = &mut self.list {
                    list.move_mark(key.code == KeyCode::Down);
                }
            }
            KeyCode::Enter | KeyCode::Tab if shown && marked => {
                self.accept();
                self.move_focus(true);
            }
            _ => return false,
        }
        true
    }

    /// Puts the marked name of the list in the focused field.
    fn accept(&mut self) {
        let Some((place, name)) = self.list.as_ref().and_then(FieldList::chosen).cloned() else {
            return;
        };
        let Some(side) = self.focus.side() else {
            return;
        };
        match self.focus {
            Field::SourceConnection | Field::TargetConnection => {
                self.choose_connection(side, place);
            }
            Field::SourceSchema | Field::TargetSchema => self.sides[side].schema = place,
            _ => self.sides[side].table.set(&name),
        }
    }

    /// Writes a key in the text of the focused field. A changed text filters the list.
    fn edit(&mut self, key: KeyEvent) {
        let Some(input) = self.input_mut() else {
            return;
        };
        if !matches!(input.on_key(key), Edit::None | Edit::Moved) {
            self.open_list();
        }
    }

    /// Opens the list of the focused field. A field without names has no list.
    fn open_list(&mut self) {
        let Some(side) = self.focus.side() else {
            self.list = None;
            return;
        };
        let form = &self.sides[side];
        let search = self.search.text();
        self.list = Some(match self.focus {
            Field::SourceConnection | Field::TargetConnection => FieldList::new(
                self.connections.iter().map(|choice| choice.name.as_str()),
                search,
                self.choice(side).map(|choice| choice.name.as_str()),
            ),
            Field::SourceSchema | Field::TargetSchema => FieldList::new(
                form.schemas
                    .iter()
                    .flatten()
                    .map(|schema| schema.name.as_str()),
                search,
                form.schema_name(),
            ),
            _ => FieldList::new(
                form.tables().iter().map(String::as_str),
                form.table.text().trim(),
                None,
            ),
        });
    }

    /// Builds the open list again, after its names or its choice changed.
    fn refresh_list(&mut self) {
        if self.list.is_some() {
            self.open_list();
        }
    }

    /// Starts the focused field: the search is empty, and a field with names opens its list.
    fn enter_field(&mut self) {
        self.search = LineInput::default();
        self.open_list();
    }

    fn move_focus(&mut self, forward: bool) {
        let place = Field::ALL
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        let count = Field::ALL.len();
        let next = if forward {
            (place + 1) % count
        } else {
            (place + count - 1) % count
        };
        self.focus = Field::ALL[next];
        self.enter_field();
    }

    /// Chooses the connection of a side. A new connection waits for its catalog.
    fn choose_connection(&mut self, side: usize, place: usize) {
        let form = &mut self.sides[side];
        if place != form.connection {
            form.wanted = form.schema_name().map(str::to_owned);
            form.connection = place;
            form.schemas = None;
            form.schema = 0;
        }
    }

    /// Changes the choice of the focused field. The list shows the new choice.
    fn change(&mut self, forward: bool) {
        let step = |place: usize, count: usize| {
            if count == 0 {
                0
            } else if forward {
                (place + 1) % count
            } else {
                (place + count - 1) % count
            }
        };
        match self.focus {
            Field::SourceConnection | Field::TargetConnection => {
                let side = self.focus.side().unwrap_or(0);
                let next = step(self.sides[side].connection, self.connections.len());
                self.choose_connection(side, next);
            }
            Field::SourceSchema | Field::TargetSchema => {
                let side = &mut self.sides[self.focus.side().unwrap_or(0)];
                let count = side.schemas.as_ref().map_or(0, Vec::len);
                side.schema = step(side.schema, count);
            }
            Field::Removals => self.removals = !self.removals,
            Field::Kind => {
                self.kind = match self.kind {
                    Kind::Structure => Kind::Data,
                    Kind::Data => Kind::Structure,
                };
            }
            _ => {}
        }
        if self.focus.has_list() {
            self.search = LineInput::default();
            self.refresh_list();
        }
    }

    /// Returns the text of the focused field: the table, or the search of a connection or
    /// a schema.
    fn input_mut(&mut self) -> Option<&mut LineInput> {
        match self.focus {
            Field::SourceTable => Some(&mut self.sides[0].table),
            Field::TargetTable => Some(&mut self.sides[1].table),
            field if field.has_list() => Some(&mut self.search),
            _ => None,
        }
    }

    pub fn paste(&mut self, value: &str) {
        let line = value.lines().next().unwrap_or("");
        if let Some(input) = self.input_mut() {
            input.insert(line);
            self.open_list();
        }
    }

    /// Puts the focus on one field and opens its list. The mouse uses this.
    pub fn focus_on(&mut self, field: Field) {
        self.focus = field;
        self.enter_field();
    }

    /// Chooses a row of the shown list, as `Enter` does. The mouse uses this.
    pub fn pick(&mut self, row: usize) {
        let Some(list) = self.list.as_mut().filter(|list| row < list.names.len()) else {
            return;
        };
        list.selected = Some(row);
        self.accept();
        self.move_focus(true);
    }

    /// Acts on a click: a choice changes, a button runs, and a field with names keeps
    /// its list open.
    pub fn press(&mut self) -> CompareRequest {
        match self.focus {
            Field::Compare => CompareRequest::Compare,
            Field::Cancel => CompareRequest::Cancel,
            field if !field.has_list() => {
                self.change(true);
                CompareRequest::None
            }
            _ => CompareRequest::None,
        }
    }

    /// Returns the two sides, or the text of the first wrong field. A target without a
    /// table takes the table of the source.
    pub fn sides(&self) -> Result<[Side; 2], String> {
        let side = |index: usize| -> Result<Side, String> {
            let choice = self
                .choice(index)
                .ok_or_else(|| text::COMPARE_NO_CONNECTION.to_owned())?;
            let form = &self.sides[index];
            let schema = form
                .schema_name()
                .ok_or_else(|| text::compare_wait_catalog(&choice.name))?;
            let table = form.table.text().trim();
            Ok(Side {
                connection: choice.id,
                schema: schema.to_owned(),
                table: (!table.is_empty()).then(|| table.to_owned()),
            })
        };
        let source = side(0)?;
        let mut target = side(1)?;
        if self.kind == Kind::Data && source.table.is_none() {
            return Err(text::COMPARE_DATA_TABLE.to_owned());
        }
        if source.table.is_none() && target.table.is_some() {
            return Err(text::COMPARE_TABLE_ON_BOTH.to_owned());
        }
        if target.table.is_none() {
            target.table.clone_from(&source.table);
        }
        if source == target {
            return Err(text::COMPARE_SAME_SIDES.to_owned());
        }
        Ok([source, target])
    }
}
