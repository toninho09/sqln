//! The connection form. It holds text only, and it builds a definition on request.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::connection::{
    DEFAULT_BUSY_SECONDS, DEFAULT_CONNECT_SECONDS, DEFAULT_POOL_LIMIT, Definition, Source,
};
use crate::connector::sqlite::{SqliteTarget, validate_path};
use crate::connector::target::{
    Address, ClientCertificate, Engine, NetworkTarget, Secret, Tls, TlsFiles, parse_uri,
};
use crate::exec::ConnectionId;
use crate::ui::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Kind,
    Name,
    Entry,
    Uri,
    Host,
    Port,
    Database,
    User,
    Password,
    Tls,
    CaFile,
    ClientCertificate,
    ClientKey,
    File,
    ForeignKeys,
    BusyTimeout,
    Pool,
    ReadOnly,
    ConnectTimeout,
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
            Field::Kind | Field::Entry | Field::Tls | Field::ForeignKeys => FieldStyle::Choice,
            Field::ReadOnly => FieldStyle::Flag,
            Field::Password => FieldStyle::Secret,
            Field::Test | Field::Save | Field::Disconnect | Field::Cancel => FieldStyle::Button,
            _ => FieldStyle::Text,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Postgres,
    MySql,
    Sqlite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormRequest {
    None,
    Cancel,
    Test,
    Save,
    Disconnect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormNote {
    Info(String),
    Error(String),
}

pub struct ConnectionForm {
    /// The connection that this form edits.
    pub editing: Option<ConnectionId>,
    /// True while the connection is connected, so only its name can change.
    pub name_only: bool,
    pub kind: Kind,
    pub use_uri: bool,
    pub verify_tls: bool,
    /// `None` keeps the default of SQLite.
    pub foreign_keys: Option<bool>,
    pub read_only: bool,
    pub focus: Field,
    pub note: Option<FormNote>,
    /// The identity of the test in progress.
    pub test: Option<u64>,
    /// The position of the cursor in the focused text, in characters.
    pub cursor: usize,
    texts: HashMap<Field, String>,
    /// True after the user chose the TLS field. The form then stops its suggestion.
    tls_chosen: bool,
}

fn is_local_host(host: &str) -> bool {
    if host.starts_with('/') {
        return true;
    }
    Address::Tcp { host: host.to_owned(), port: 0 }.is_local()
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
            name_only: false,
            kind: Kind::Postgres,
            use_uri: false,
            verify_tls: true,
            foreign_keys: None,
            read_only: false,
            focus: Field::Kind,
            note: None,
            test: None,
            cursor: 0,
            texts: HashMap::new(),
            tls_chosen: false,
        };
        form.set(Field::Pool, DEFAULT_POOL_LIMIT.to_string());
        form.set(Field::ConnectTimeout, DEFAULT_CONNECT_SECONDS.to_string());
        form.set(Field::BusyTimeout, DEFAULT_BUSY_SECONDS.to_string());
        form
    }

    /// Fills the form from a definition. The password stays in the form, in memory.
    pub fn editing(id: ConnectionId, definition: &Definition, name_only: bool) -> Self {
        let mut form = Self::new();
        form.editing = Some(id);
        form.name_only = name_only;
        form.focus = Field::Name;
        form.tls_chosen = true;
        form.read_only = definition.read_only;
        form.set(Field::Name, definition.name.clone());
        form.set(Field::Pool, definition.max_connections.to_string());
        form.set(Field::ConnectTimeout, definition.connect_timeout.as_secs().to_string());
        match &definition.source {
            Source::Sqlite(target) => {
                form.kind = Kind::Sqlite;
                form.foreign_keys = target.foreign_keys;
                form.set(Field::File, path_text(&target.path));
                form.set(Field::BusyTimeout, target.busy_timeout.as_secs().to_string());
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
        self.texts.insert(field, value);
    }

    pub fn text(&self, field: Field) -> &str {
        self.texts.get(&field).map_or("", String::as_str)
    }

    fn trimmed(&self, field: Field) -> &str {
        self.text(field).trim()
    }

    fn focused_len(&self) -> usize {
        self.text(self.focus).chars().count()
    }

    /// Lists the visible fields, in order.
    pub fn fields(&self) -> Vec<Field> {
        if self.name_only {
            return vec![Field::Name, Field::Disconnect, Field::Save, Field::Cancel];
        }
        let mut fields = vec![Field::Kind, Field::Name];
        match self.kind {
            Kind::Sqlite => fields.extend([Field::File, Field::ForeignKeys, Field::BusyTimeout]),
            _ if self.use_uri => fields.extend([Field::Entry, Field::Uri]),
            _ => fields.extend([
                Field::Entry,
                Field::Host,
                Field::Port,
                Field::Database,
                Field::User,
                Field::Password,
                Field::Tls,
                Field::CaFile,
                Field::ClientCertificate,
                Field::ClientKey,
            ]),
        }
        fields.extend([Field::Pool, Field::ReadOnly]);
        if self.kind != Kind::Sqlite {
            fields.push(Field::ConnectTimeout);
        }
        fields.extend([Field::Test, Field::Save, Field::Cancel]);
        fields
    }

    pub fn focus_on(&mut self, field: Field) {
        self.focus = field;
        self.cursor = self.focused_len();
    }

    fn move_focus(&mut self, forward: bool) {
        let fields = self.fields();
        let count = fields.len();
        let index = fields.iter().position(|field| *field == self.focus).unwrap_or(0);
        let next = (index + if forward { 1 } else { count - 1 }) % count;
        self.focus_on(fields[next]);
    }

    /// Changes the value of a choice or a flag.
    fn change(&mut self, forward: bool) {
        match self.focus {
            Field::Kind => {
                let order = [Kind::Postgres, Kind::MySql, Kind::Sqlite];
                let index = order.iter().position(|kind| *kind == self.kind).unwrap_or(0);
                let step = if forward { 1 } else { order.len() - 1 };
                self.kind = order[(index + step) % order.len()];
            }
            Field::Entry => self.use_uri = !self.use_uri,
            Field::Tls => {
                self.verify_tls = !self.verify_tls;
                self.tls_chosen = true;
            }
            Field::ForeignKeys => {
                let order = [None, Some(true), Some(false)];
                let index = order.iter().position(|value| *value == self.foreign_keys).unwrap_or(0);
                let step = if forward { 1 } else { order.len() - 1 };
                self.foreign_keys = order[(index + step) % order.len()];
            }
            Field::ReadOnly => self.read_only = !self.read_only,
            _ => {}
        }
    }

    fn insert(&mut self, text: &str) {
        let value = self.texts.entry(self.focus).or_default();
        let byte = value.char_indices().nth(self.cursor).map_or(value.len(), |(byte, _)| byte);
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

    /// Writes pasted text in a text field. A form field holds one line.
    pub fn on_paste(&mut self, text: &str) {
        if matches!(self.focus.style(), FieldStyle::Text | FieldStyle::Secret) {
            let line: String = text.lines().next().unwrap_or("").chars().filter(|c| !c.is_control()).collect();
            self.insert(&line);
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, vim: bool) -> FormRequest {
        let style = self.focus.style();
        let in_text = matches!(style, FieldStyle::Text | FieldStyle::Secret);
        match key.code {
            KeyCode::Esc => return FormRequest::Cancel,
            KeyCode::Tab | KeyCode::Down => self.move_focus(true),
            KeyCode::BackTab | KeyCode::Up => self.move_focus(false),
            KeyCode::Enter => return self.on_enter(),
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
            KeyCode::Char('j') if vim => self.move_focus(true),
            KeyCode::Char('k') if vim => self.move_focus(false),
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

    fn number(&self, field: Field, label: &str) -> Result<u64, String> {
        self.trimmed(field).parse().map_err(|_| text::form_needs_number(label))
    }

    fn optional_path(&self, field: Field) -> Option<PathBuf> {
        Some(self.trimmed(field)).filter(|text| !text.is_empty()).map(PathBuf::from)
    }

    fn password(&self) -> Option<Secret> {
        Some(self.text(Field::Password)).filter(|text| !text.is_empty()).map(Secret::new)
    }

    /// A remote address without TLS is the explicit choice of the TLS field.
    fn tls(&self) -> Result<Tls, String> {
        let ca = self.optional_path(Field::CaFile);
        let client = match (
            self.optional_path(Field::ClientCertificate),
            self.optional_path(Field::ClientKey),
        ) {
            (Some(certificate), Some(key)) => Some(ClientCertificate { certificate, key }),
            (None, None) => None,
            _ => return Err(text::FORM_CLIENT_PAIR.to_owned()),
        };
        if self.verify_tls {
            return Ok(Tls::Verify(TlsFiles { ca, client }));
        }
        if ca.is_some() || client.is_some() {
            return Err(text::FORM_TLS_FILES.to_owned());
        }
        Ok(Tls::Disabled)
    }

    fn network_target(&self, engine: Engine) -> Result<NetworkTarget, String> {
        let host = self.trimmed(Field::Host);
        if host.is_empty() {
            return Err(text::FORM_NEEDS_HOST.to_owned());
        }
        let user = self.trimmed(Field::User);
        if user.is_empty() {
            return Err(text::FORM_NEEDS_USER.to_owned());
        }
        let address = if host.starts_with('/') {
            Address::Socket(PathBuf::from(host))
        } else {
            let port = match self.trimmed(Field::Port) {
                "" => engine.default_port(),
                port => port.parse().map_err(|_| text::form_needs_number(text::FIELD_PORT))?,
            };
            Address::Tcp { host: host.to_owned(), port }
        };
        let tls = self.tls()?;
        let database = Some(self.trimmed(Field::Database)).filter(|text| !text.is_empty());
        Ok(NetworkTarget {
            engine,
            address,
            user: user.to_owned(),
            password: self.password(),
            database: database.map(str::to_owned),
            tls,
        })
    }

    fn uri_target(&self, engine: Engine) -> Result<NetworkTarget, String> {
        let mut target = parse_uri(self.trimmed(Field::Uri))?;
        if target.engine != engine {
            return Err(text::FORM_URI_KIND.to_owned());
        }
        if target.password.is_none() {
            target.password = self.password();
        }
        Ok(target)
    }

    fn sqlite_target(&self) -> Result<SqliteTarget, String> {
        let file = self.trimmed(Field::File);
        if file.is_empty() {
            return Err(text::FORM_NEEDS_FILE.to_owned());
        }
        let mut target = SqliteTarget::new(file);
        validate_path(&target.path).map_err(|error| error.0)?;
        target.foreign_keys = self.foreign_keys;
        let seconds = self.number(Field::BusyTimeout, text::FIELD_BUSY)?;
        target.busy_timeout = Duration::from_secs(seconds);
        Ok(target)
    }

    /// Builds the definition. The error text names the field and holds no input,
    /// because a connection text can hold a password.
    pub fn definition(&self) -> Result<Definition, String> {
        let name = self.trimmed(Field::Name);
        if name.is_empty() {
            return Err(text::FORM_NEEDS_NAME.to_owned());
        }
        let source = match (self.kind, self.use_uri) {
            (Kind::Sqlite, _) => Source::Sqlite(self.sqlite_target()?),
            (Kind::Postgres, true) => Source::Network(self.uri_target(Engine::Postgres)?),
            (Kind::MySql, true) => Source::Network(self.uri_target(Engine::MySql)?),
            (Kind::Postgres, false) => Source::Network(self.network_target(Engine::Postgres)?),
            (Kind::MySql, false) => Source::Network(self.network_target(Engine::MySql)?),
        };
        let limit = self.number(Field::Pool, text::FIELD_POOL)?;
        if limit == 0 {
            return Err(text::FORM_POOL_ZERO.to_owned());
        }
        let mut definition = Definition::new(name, source);
        definition.read_only = self.read_only;
        definition.max_connections = usize::try_from(limit).unwrap_or(usize::MAX);
        if self.kind != Kind::Sqlite {
            let seconds = self.number(Field::ConnectTimeout, text::FIELD_CONNECT)?;
            if seconds == 0 {
                return Err(text::form_needs_number(text::FIELD_CONNECT));
            }
            definition.connect_timeout = Duration::from_secs(seconds);
        }
        Ok(definition)
    }
}
