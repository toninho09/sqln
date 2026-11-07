//! The connection form. It holds text only, and it builds a definition on request.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use zeroize::Zeroizing;

use crate::connection::{
    DEFAULT_BUSY_SECONDS, DEFAULT_CONNECT_SECONDS, DEFAULT_IDLE_SECONDS, DEFAULT_POOL_LIMIT,
    DEFAULT_SQL_SECONDS, Definition, Source,
};
use crate::connector::sqlite::{SqliteTarget, validate_path};
use crate::connector::target::{
    Address, ClientCertificate, Engine, NetworkTarget, Secret, Tls, TlsFiles, parse_uri,
};
use crate::exec::ConnectionId;
use crate::mark_list::{MarkList, MarkRequest};
use crate::settings::{next_choice, next_optional};
use crate::store::Scope;
use crate::store::config::{ConnectionIcon, CredentialName, ThemeName};
use crate::ui::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    /// The sidebar. It is one place of the focus.
    Sections,
    Kind,
    Name,
    Scope,
    Entry,
    Uri,
    Host,
    Port,
    Database,
    User,
    Password,
    Credential,
    Tls,
    CaFile,
    ClientCertificate,
    ClientKey,
    File,
    ForeignKeys,
    BusyTimeout,
    /// The schemas, or the Redis databases, that the explorer shows.
    Schemas,
    Pool,
    ReadOnly,
    Production,
    ConnectTimeout,
    SqlTimeout,
    IdleTimeout,
    Theme,
    Icon,
    Test,
    Save,
    Disconnect,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldStyle {
    Text,
    Secret,
    Choice,
    Flag,
    Button,
}

impl Field {
    pub fn style(self) -> FieldStyle {
        match self {
            Field::Sections
            | Field::Kind
            | Field::Scope
            | Field::Entry
            | Field::Credential
            | Field::Tls
            | Field::ForeignKeys
            | Field::Theme
            | Field::Icon => FieldStyle::Choice,
            Field::ReadOnly | Field::Production => FieldStyle::Flag,
            Field::Password => FieldStyle::Secret,
            Field::Test | Field::Save | Field::Disconnect | Field::Cancel => FieldStyle::Button,
            _ => FieldStyle::Text,
        }
    }

    /// Returns the section of the sidebar that holds the field.
    pub fn group(self) -> Group {
        match self {
            Field::Kind | Field::Name | Field::Scope | Field::Entry => Group::General,
            Field::Uri | Field::Host | Field::Port | Field::Database | Field::File => {
                Group::Address
            }
            Field::User | Field::Password | Field::Credential => Group::Credentials,
            Field::Tls | Field::CaFile | Field::ClientCertificate | Field::ClientKey => {
                Group::Security
            }
            _ => Group::Options,
        }
    }

    /// Returns true for a field that holds the path of a file.
    pub fn is_path(self) -> bool {
        matches!(
            self,
            Field::File | Field::CaFile | Field::ClientCertificate | Field::ClientKey
        )
    }

    /// Returns true for a field with a button that opens a window: the file browser or
    /// the list of the schemas.
    pub fn opens_window(self) -> bool {
        self.is_path() || self == Field::Schemas
    }
}

/// The sections of the sidebar, in the order of the fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    General,
    Address,
    Credentials,
    Security,
    Options,
}

/// A value that stops the save. The form opens the section of the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid {
    pub field: Field,
    pub message: String,
}

fn invalid(field: Field, message: impl Into<String>) -> Invalid {
    Invalid {
        field,
        message: message.into(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Postgres,
    MySql,
    Sqlite,
    Redis,
}

impl Kind {
    /// Returns the engine of a network connection. SQLite has no engine.
    fn engine(self) -> Option<Engine> {
        match self {
            Kind::Postgres => Some(Engine::Postgres),
            Kind::MySql => Some(Engine::MySql),
            Kind::Redis => Some(Engine::Redis),
            Kind::Sqlite => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormRequest {
    None,
    Cancel,
    Test,
    Save,
    Disconnect,
    /// Opens the file browser for the focused field.
    Browse,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormNote {
    Info(String),
    /// The result of an action that worked.
    Success(String),
    Error(String),
}

/// The window that chooses the visible schemas. It covers the form.
pub struct SchemaList {
    /// True until the names arrive. The number of the read is `ConnectionForm::test`.
    pub loading: bool,
    pub list: MarkList,
}

pub struct ConnectionForm {
    /// The connection that this form edits.
    pub editing: Option<ConnectionId>,
    /// True while the connection is connected. Only its name, its theme, its icon, and
    /// its production mark can change, because these values need no new session.
    pub display_only: bool,
    pub kind: Kind,
    pub credential: CredentialName,
    /// The stable identity of a new definition, including tests before its first save.
    pub connection_id: String,
    pub use_uri: bool,
    pub verify_tls: bool,
    /// `None` keeps the default of SQLite.
    pub foreign_keys: Option<bool>,
    pub read_only: bool,
    pub production: bool,
    /// `None` uses the default theme.
    pub theme: Option<ThemeName>,
    /// `None` uses the icon of the database.
    pub icon: Option<ConnectionIcon>,
    /// The scope of a new connection. The scope of a saved connection does not change.
    pub scope: Scope,
    /// True when a project is open, so the form can offer its scope.
    pub project_available: bool,
    pub focus: Field,
    /// The section that the sidebar shows.
    pub section: Group,
    pub note: Option<FormNote>,
    /// The file of the saved definition, as its configuration writes it.
    stored_file: Option<String>,
    /// The identity of the test in progress.
    pub test: Option<u64>,
    /// The position of the cursor in the focused text, in characters.
    pub cursor: usize,
    texts: HashMap<Field, Zeroizing<String>>,
    /// True after the user chose the TLS field. The form then stops its suggestion.
    tls_chosen: bool,
    pub schema_list: Option<SchemaList>,
}

/// Splits the text of the schemas field at each comma. Spaces at the ends, empty parts,
/// and repeats go away.
pub fn schema_names(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for name in text
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        if !names.iter().any(|item| item == name) {
            names.push(name.to_owned());
        }
    }
    names
}

fn is_local_host(host: &str) -> bool {
    if host.starts_with('/') {
        return true;
    }
    Address::Tcp {
        host: host.to_owned(),
        port: 0,
    }
    .is_local()
}

fn path_text(path: &std::path::Path) -> String {
    path.display().to_string()
}

impl Default for ConnectionForm {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionForm {
    pub fn new() -> Self {
        let mut form = Self {
            editing: None,
            display_only: false,
            stored_file: None,
            kind: Kind::Postgres,
            credential: CredentialName::Session,
            connection_id: uuid::Uuid::new_v4().to_string(),
            use_uri: false,
            verify_tls: true,
            foreign_keys: None,
            read_only: false,
            production: false,
            theme: None,
            icon: None,
            scope: Scope::Global,
            project_available: false,
            focus: Field::Kind,
            section: Group::General,
            note: None,
            test: None,
            cursor: 0,
            texts: HashMap::new(),
            tls_chosen: false,
            schema_list: None,
        };
        form.set(Field::Pool, DEFAULT_POOL_LIMIT.to_string());
        form.set(Field::ConnectTimeout, DEFAULT_CONNECT_SECONDS.to_string());
        form.set(Field::BusyTimeout, DEFAULT_BUSY_SECONDS.to_string());
        form.set(Field::SqlTimeout, DEFAULT_SQL_SECONDS.to_string());
        form.set(Field::IdleTimeout, DEFAULT_IDLE_SECONDS.to_string());
        form
    }

    /// A form for a new connection. With a project, the project scope is the default.
    pub fn new_in(project_available: bool) -> Self {
        let mut form = Self::new();
        form.project_available = project_available;
        if project_available {
            form.scope = Scope::Project;
        }
        form
    }

    /// Fills the form from a definition. The password stays in the form, in memory.
    pub fn editing(id: ConnectionId, definition: &Definition, display_only: bool) -> Self {
        let mut form = Self::new();
        form.editing = Some(id);
        form.display_only = display_only;
        form.focus = Field::Name;
        form.tls_chosen = true;
        form.read_only = definition.read_only;
        form.production = definition.production;
        form.theme = definition.theme;
        form.icon = definition.icon;
        form.set(Field::Name, definition.name.clone());
        form.set(Field::Pool, definition.max_connections.to_string());
        form.set(
            Field::ConnectTimeout,
            definition.connect_timeout.as_secs().to_string(),
        );
        let sql_seconds = definition.sql_timeout.map_or(0, |time| time.as_secs());
        form.set(Field::SqlTimeout, sql_seconds.to_string());
        let idle_seconds = definition.idle_timeout.map_or(0, |time| time.as_secs());
        form.set(Field::IdleTimeout, idle_seconds.to_string());
        form.set(Field::Schemas, definition.schemas.join(", "));
        match &definition.source {
            Source::Sqlite(target) => {
                form.kind = Kind::Sqlite;
                form.foreign_keys = target.foreign_keys;
                let file = target.file_display();
                form.stored_file = Some(file.clone());
                form.set(Field::File, file);
                form.set(
                    Field::BusyTimeout,
                    target.busy_timeout.as_secs().to_string(),
                );
            }
            Source::Network(target) => form.fill_network(target),
        }
        form.cursor = form.focused_len();
        form
    }

    fn fill_network(&mut self, target: &NetworkTarget) {
        self.kind = match target.engine {
            Engine::Postgres => Kind::Postgres,
            Engine::MySql => Kind::MySql,
            Engine::Redis => Kind::Redis,
        };
        match &target.address {
            Address::Tcp { host, port } => {
                self.set(Field::Host, host.clone());
                self.set(Field::Port, port.to_string());
            }
            Address::Socket(path) => self.set(Field::Host, path_text(path)),
        }
        self.set(Field::Database, target.database.clone().unwrap_or_default());
        self.set(Field::User, target.user.clone());
        if let Some(password) = &target.password {
            self.set(Field::Password, password.expose().to_owned());
        }
        self.verify_tls = matches!(target.tls, Tls::Verify(_));
        if let Tls::Verify(files) = &target.tls {
            if let Some(ca) = &files.ca {
                self.set(Field::CaFile, path_text(ca));
            }
            if let Some(client) = &files.client {
                self.set(Field::ClientCertificate, path_text(&client.certificate));
                self.set(Field::ClientKey, path_text(&client.key));
            }
        }
    }

    fn set(&mut self, field: Field, value: String) {
        self.texts.insert(field, Zeroizing::new(value));
    }

    pub fn text(&self, field: Field) -> &str {
        self.texts.get(&field).map_or("", |value| value.as_str())
    }

    fn trimmed(&self, field: Field) -> &str {
        self.text(field).trim()
    }

    fn focused_len(&self) -> usize {
        self.text(self.focus).chars().count()
    }

    /// Returns the value of a field with the style `Flag`.
    pub fn flag(&self, field: Field) -> bool {
        match field {
            Field::Production => self.production,
            _ => self.read_only,
        }
    }

    /// Lists the visible fields, in order.
    pub fn fields(&self) -> Vec<Field> {
        if self.display_only {
            let mut fields = vec![Field::Name];
            if self.kind != Kind::Sqlite {
                fields.push(Field::Schemas);
            }
            fields.extend([
                Field::Theme,
                Field::Icon,
                Field::Production,
                Field::Disconnect,
                Field::Save,
                Field::Cancel,
            ]);
            return fields;
        }
        let mut fields = vec![Field::Kind, Field::Name];
        if self.project_available && self.editing.is_none() {
            fields.push(Field::Scope);
        }
        match self.kind {
            Kind::Sqlite => fields.extend([Field::File, Field::ForeignKeys, Field::BusyTimeout]),
            _ if self.use_uri => fields.extend([Field::Entry, Field::Uri, Field::Credential]),
            _ => fields.extend([
                Field::Entry,
                Field::Host,
                Field::Port,
                Field::Database,
                Field::User,
                Field::Password,
                Field::Credential,
                Field::Tls,
                Field::CaFile,
                Field::ClientCertificate,
                Field::ClientKey,
            ]),
        }
        if self.kind != Kind::Sqlite {
            fields.push(Field::Schemas);
        }
        fields.extend([Field::Pool, Field::ReadOnly, Field::Production]);
        if self.kind != Kind::Sqlite {
            fields.push(Field::ConnectTimeout);
        }
        fields.extend([
            Field::SqlTimeout,
            Field::IdleTimeout,
            Field::Theme,
            Field::Icon,
            Field::Test,
            Field::Save,
            Field::Cancel,
        ]);
        fields
    }

    /// Lists the sections that hold a field, in order.
    pub fn sections(&self) -> Vec<Group> {
        let mut sections: Vec<Group> = Vec::new();
        for field in self.fields() {
            let group = field.group();
            if field.style() != FieldStyle::Button && sections.last() != Some(&group) {
                sections.push(group);
            }
        }
        sections
    }

    /// Returns the open section. A section without fields gives its place to the first one.
    pub fn shown_section(&self) -> Group {
        let sections = self.sections();
        if sections.contains(&self.section) {
            self.section
        } else {
            sections.first().copied().unwrap_or(Group::General)
        }
    }

    /// Lists the fields of one section, without the buttons.
    pub fn fields_of(&self, section: Group) -> Vec<Field> {
        self.fields()
            .into_iter()
            .filter(|field| field.style() != FieldStyle::Button && field.group() == section)
            .collect()
    }

    /// Lists the fields of the open section, without the buttons.
    pub fn section_fields(&self) -> Vec<Field> {
        self.fields_of(self.shown_section())
    }

    /// Lists the buttons. Each section shows them.
    pub fn buttons(&self) -> Vec<Field> {
        self.fields()
            .into_iter()
            .filter(|field| field.style() == FieldStyle::Button)
            .collect()
    }

    /// Returns the places of the focus, in the order of `Tab`.
    fn stops(&self) -> Vec<Field> {
        let mut stops = vec![Field::Sections];
        stops.extend(self.section_fields());
        stops.extend(self.buttons());
        stops
    }

    /// Puts the focus on a field and opens its section.
    pub fn focus_on(&mut self, field: Field) {
        self.focus = field;
        if field != Field::Sections && field.style() != FieldStyle::Button {
            self.section = field.group();
        }
        self.cursor = self.focused_len();
    }

    /// Opens one section and puts the focus on the sidebar. The mouse uses this.
    pub fn select_section(&mut self, section: Group) {
        self.section = section;
        self.focus = Field::Sections;
    }

    fn move_focus(&mut self, forward: bool) {
        let next = next_choice(&self.stops(), self.focus, forward);
        self.focus_on(next);
    }

    /// `Up` and `Down` open another section from the sidebar, and move the focus elsewhere.
    fn step(&mut self, forward: bool) {
        if self.focus == Field::Sections {
            self.section = next_choice(&self.sections(), self.shown_section(), forward);
        } else {
            self.move_focus(forward);
        }
    }

    /// Changes the value of a choice or a flag.
    fn change(&mut self, forward: bool) {
        match self.focus {
            Field::Kind => {
                let order = [Kind::Postgres, Kind::MySql, Kind::Sqlite, Kind::Redis];
                let index = order
                    .iter()
                    .position(|kind| *kind == self.kind)
                    .unwrap_or(0);
                let step = if forward { 1 } else { order.len() - 1 };
                self.kind = order[(index + step) % order.len()];
            }
            Field::Scope => {
                self.scope = if self.scope == Scope::Project {
                    Scope::Global
                } else {
                    Scope::Project
                };
            }
            Field::Entry => self.use_uri = !self.use_uri,
            Field::Credential => {
                self.credential = if self.credential == CredentialName::Vault {
                    CredentialName::Session
                } else {
                    CredentialName::Vault
                };
            }
            Field::Tls => {
                self.verify_tls = !self.verify_tls;
                self.tls_chosen = true;
            }
            Field::ForeignKeys => {
                let order = [None, Some(true), Some(false)];
                let index = order
                    .iter()
                    .position(|value| *value == self.foreign_keys)
                    .unwrap_or(0);
                let step = if forward { 1 } else { order.len() - 1 };
                self.foreign_keys = order[(index + step) % order.len()];
            }
            Field::ReadOnly => self.read_only = !self.read_only,
            Field::Production => self.production = !self.production,
            Field::Theme => self.theme = next_optional(&ThemeName::ALL, self.theme, forward),
            Field::Icon => self.icon = next_optional(&ConnectionIcon::ALL, self.icon, forward),
            _ => {}
        }
    }

    fn insert(&mut self, text: &str) {
        let value = self.texts.entry(self.focus).or_default();
        let byte = value
            .char_indices()
            .nth(self.cursor)
            .map_or(value.len(), |(byte, _)| byte);
        value.insert_str(byte, text);
        self.cursor += text.chars().count();
        self.after_edit();
    }

    fn delete_before_cursor(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        let cursor = self.cursor;
        if let Some(value) = self.texts.get_mut(&self.focus)
            && let Some((byte, _)) = value.char_indices().nth(cursor)
        {
            value.remove(byte);
        }
        self.after_edit();
    }

    /// The form suggests no TLS for a local address and verification for any other,
    /// until the user chooses a value.
    fn after_edit(&mut self) {
        if self.focus == Field::Host && !self.tls_chosen {
            self.verify_tls = !is_local_host(self.trimmed(Field::Host));
        }
    }

    /// Writes the choice of the file browser in the focused field.
    pub fn set_focused_path(&mut self, path: &str) {
        self.set(self.focus, path.to_owned());
        self.cursor = self.focused_len();
    }

    /// Opens the list of the schemas. The names of the field are marked. The names of
    /// the server arrive later.
    pub fn open_schema_list(&mut self) {
        let marked = schema_names(self.text(Field::Schemas));
        self.schema_list = Some(SchemaList {
            loading: true,
            list: MarkList::new(Vec::new(), marked),
        });
    }

    /// Shows the names of the server in the open list.
    pub fn fill_schema_list(&mut self, names: Vec<String>) {
        if let Some(open) = &mut self.schema_list {
            open.list = MarkList::new(names, open.list.marked().to_vec());
            open.loading = false;
        }
    }

    pub fn close_schema_list(&mut self) {
        self.schema_list = None;
    }

    /// Writes the marked names in the field, and it closes the list.
    fn end_schema_list(&mut self, names: &[String]) {
        self.set(Field::Schemas, names.join(", "));
        self.focus_on(Field::Schemas);
        self.cursor = self.focused_len();
        self.close_schema_list();
    }

    fn on_schema_list_key(&mut self, key: KeyEvent) {
        let Some(open) = &mut self.schema_list else {
            return;
        };
        if open.loading {
            if key.code == KeyCode::Esc {
                self.test = None;
                self.close_schema_list();
            }
            return;
        }
        match open.list.on_key(key) {
            MarkRequest::None => {}
            MarkRequest::Cancel => self.close_schema_list(),
            MarkRequest::Done(names) => self.end_schema_list(&names),
        }
    }

    /// Writes pasted text in a text field. A form field holds one line.
    pub fn on_paste(&mut self, text: &str) {
        if let Some(open) = &mut self.schema_list {
            if !open.loading {
                open.list.paste(text);
            }
            return;
        }
        if matches!(self.focus.style(), FieldStyle::Text | FieldStyle::Secret) {
            let line = Zeroizing::new(
                text.lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect::<String>(),
            );
            self.insert(&line);
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, vim: bool) -> FormRequest {
        if self.schema_list.is_some() {
            self.on_schema_list_key(key);
            return FormRequest::None;
        }
        let style = self.focus.style();
        let in_text = matches!(style, FieldStyle::Text | FieldStyle::Secret);
        match key.code {
            KeyCode::Esc => return FormRequest::Cancel,
            KeyCode::Tab => self.move_focus(true),
            KeyCode::BackTab => self.move_focus(false),
            KeyCode::Down => self.step(true),
            KeyCode::Up => self.step(false),
            KeyCode::Enter => return self.on_enter(),
            KeyCode::Char('o')
                if key.modifiers == KeyModifiers::CONTROL && self.focus.opens_window() =>
            {
                return FormRequest::Browse;
            }
            KeyCode::Left if in_text => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right if in_text => self.cursor = (self.cursor + 1).min(self.focused_len()),
            KeyCode::Left if style == FieldStyle::Button => self.move_focus(false),
            KeyCode::Right if style == FieldStyle::Button => self.move_focus(true),
            KeyCode::Left => self.change(false),
            KeyCode::Right => self.change(true),
            KeyCode::Home if in_text => self.cursor = 0,
            KeyCode::End if in_text => self.cursor = self.focused_len(),
            KeyCode::Backspace if in_text => self.delete_before_cursor(),
            KeyCode::Char(c) if in_text && (key.modifiers - KeyModifiers::SHIFT).is_empty() => {
                self.insert(c.encode_utf8(&mut [0; 4]));
            }
            KeyCode::Char(' ') if style == FieldStyle::Flag => self.change(true),
            KeyCode::Char('j') if vim => self.step(true),
            KeyCode::Char('k') if vim => self.step(false),
            KeyCode::Char('h') if vim && style == FieldStyle::Button => self.move_focus(false),
            KeyCode::Char('l') if vim && style == FieldStyle::Button => self.move_focus(true),
            KeyCode::Char('h') if vim => self.change(false),
            KeyCode::Char('l') if vim => self.change(true),
            _ => {}
        }
        FormRequest::None
    }

    /// Activates a button, and it changes a choice or a flag. The mouse uses this.
    pub fn press(&mut self) -> FormRequest {
        match self.focus.style() {
            FieldStyle::Button => self.on_enter(),
            FieldStyle::Choice | FieldStyle::Flag => {
                self.change(true);
                FormRequest::None
            }
            _ => FormRequest::None,
        }
    }

    fn on_enter(&mut self) -> FormRequest {
        match self.focus {
            Field::Test => FormRequest::Test,
            Field::Save => FormRequest::Save,
            Field::Disconnect => FormRequest::Disconnect,
            Field::Cancel => FormRequest::Cancel,
            _ => {
                self.move_focus(true);
                FormRequest::None
            }
        }
    }

    /// Returns the visible schemas of the field. A Redis database must be a number.
    pub fn schemas(&self) -> Result<Vec<String>, Invalid> {
        let names = schema_names(self.text(Field::Schemas));
        let engine = self.kind.engine();
        if engine.is_some_and(|engine| !names.iter().all(|name| engine.accepts_database(name))) {
            return Err(invalid(Field::Schemas, text::FORM_REDIS_SCHEMAS));
        }
        Ok(names)
    }

    fn number(&self, field: Field, label: &str) -> Result<u64, Invalid> {
        self.trimmed(field)
            .parse()
            .map_err(|_| invalid(field, text::form_needs_number(label)))
    }

    fn optional_path(&self, field: Field) -> Option<PathBuf> {
        Some(self.trimmed(field))
            .filter(|text| !text.is_empty())
            .map(PathBuf::from)
    }

    fn password(&self) -> Option<Secret> {
        Some(self.text(Field::Password))
            .filter(|text| !text.is_empty())
            .map(Secret::new)
    }

    /// A remote address without TLS is the explicit choice of the TLS field.
    fn tls(&self) -> Result<Tls, Invalid> {
        let ca = self.optional_path(Field::CaFile);
        let client = match (
            self.optional_path(Field::ClientCertificate),
            self.optional_path(Field::ClientKey),
        ) {
            (Some(certificate), Some(key)) => Some(ClientCertificate { certificate, key }),
            (None, None) => None,
            (None, Some(_)) => {
                return Err(invalid(Field::ClientCertificate, text::FORM_CLIENT_PAIR));
            }
            (Some(_), None) => return Err(invalid(Field::ClientKey, text::FORM_CLIENT_PAIR)),
        };
        if self.verify_tls {
            return Ok(Tls::Verify(TlsFiles { ca, client }));
        }
        if ca.is_some() || client.is_some() {
            return Err(invalid(Field::Tls, text::FORM_TLS_FILES));
        }
        Ok(Tls::Disabled)
    }

    fn network_target(&self, engine: Engine) -> Result<NetworkTarget, Invalid> {
        let host = self.trimmed(Field::Host);
        if host.is_empty() {
            return Err(invalid(Field::Host, text::FORM_NEEDS_HOST));
        }
        let user = self.trimmed(Field::User);
        if user.is_empty() && engine.needs_user() {
            return Err(invalid(Field::User, text::FORM_NEEDS_USER));
        }
        let address = if host.starts_with('/') {
            Address::Socket(PathBuf::from(host))
        } else {
            let port = match self.trimmed(Field::Port) {
                "" => engine.default_port(),
                port => port
                    .parse()
                    .map_err(|_| invalid(Field::Port, text::form_needs_number(text::FIELD_PORT)))?,
            };
            Address::Tcp {
                host: host.to_owned(),
                port,
            }
        };
        let tls = self.tls()?;
        let database = Some(self.trimmed(Field::Database)).filter(|text| !text.is_empty());
        if database.is_some_and(|name| !engine.accepts_database(name)) {
            return Err(invalid(Field::Database, text::FORM_REDIS_DATABASE));
        }
        Ok(NetworkTarget {
            engine,
            address,
            user: user.to_owned(),
            password: self.password(),
            database: database.map(str::to_owned),
            tls,
        })
    }

    fn uri_target(&self, engine: Engine) -> Result<NetworkTarget, Invalid> {
        let mut target =
            parse_uri(self.trimmed(Field::Uri)).map_err(|message| invalid(Field::Uri, message))?;
        if target.engine != engine {
            return Err(invalid(Field::Uri, text::FORM_URI_KIND));
        }
        if target.password.is_none() {
            target.password = self.password();
        }
        Ok(target)
    }

    fn sqlite_target(&self) -> Result<SqliteTarget, Invalid> {
        let file = self.trimmed(Field::File);
        if file.is_empty() {
            return Err(invalid(Field::File, text::FORM_NEEDS_FILE));
        }
        if file.eq_ignore_ascii_case(":memory:") {
            // A database in memory needs no file, and it lives in one session.
            let mut target = SqliteTarget::in_memory();
            target.foreign_keys = self.foreign_keys;
            let seconds = self.number(Field::BusyTimeout, text::FIELD_BUSY)?;
            target.busy_timeout = Duration::from_secs(seconds);
            return Ok(target);
        }
        validate_path(Path::new(file)).map_err(|error| invalid(Field::File, error.message))?;
        let mut target = SqliteTarget::new(self.file_path(file)?);
        target.foreign_keys = self.foreign_keys;
        let seconds = self.number(Field::BusyTimeout, text::FIELD_BUSY)?;
        target.busy_timeout = Duration::from_secs(seconds);
        Ok(target)
    }

    /// Returns the path of the SQLite file. A path that the user writes starts at the
    /// current folder, and it becomes a full path, so the test and the saved connection
    /// open the same file. A saved path keeps its meaning in the configuration.
    fn file_path(&self, file: &str) -> Result<PathBuf, Invalid> {
        if self.stored_file.as_deref() == Some(file) {
            return Ok(PathBuf::from(file));
        }
        std::path::absolute(file).map_err(|_| invalid(Field::File, text::FORM_NO_CURRENT_FOLDER))
    }

    /// Builds the definition. The error names the field and holds no input, because a
    /// connection text can hold a password.
    pub fn definition(&self) -> Result<Definition, Invalid> {
        let name = self.trimmed(Field::Name);
        if name.is_empty() {
            return Err(invalid(Field::Name, text::FORM_NEEDS_NAME));
        }
        let source = match self.kind.engine() {
            None => Source::Sqlite(self.sqlite_target()?),
            Some(engine) if self.use_uri => Source::Network(self.uri_target(engine)?),
            Some(engine) => Source::Network(self.network_target(engine)?),
        };
        let limit = self.number(Field::Pool, text::FIELD_POOL)?;
        if limit == 0 {
            return Err(invalid(Field::Pool, text::FORM_POOL_ZERO));
        }
        let mut definition = Definition::new(name, source);
        definition.read_only = self.read_only;
        definition.production = self.production;
        definition.theme = self.theme;
        definition.icon = self.icon;
        definition.max_connections = usize::try_from(limit).unwrap_or(usize::MAX);
        if self.kind != Kind::Sqlite {
            definition.schemas = self.schemas()?;
            let seconds = self.number(Field::ConnectTimeout, text::FIELD_CONNECT)?;
            if seconds == 0 {
                return Err(invalid(
                    Field::ConnectTimeout,
                    text::form_needs_number(text::FIELD_CONNECT),
                ));
            }
            definition.connect_timeout = Duration::from_secs(seconds);
        }
        // A limit of 0 seconds means that the statements have no time limit.
        let seconds = self.number(Field::SqlTimeout, text::FIELD_SQL_TIMEOUT)?;
        definition.sql_timeout = (seconds > 0).then(|| Duration::from_secs(seconds));
        // A limit of 0 seconds keeps the idle sessions open.
        let seconds = self.number(Field::IdleTimeout, text::FIELD_IDLE_TIMEOUT)?;
        definition.idle_timeout = (seconds > 0).then(|| Duration::from_secs(seconds));
        Ok(definition)
    }
}
