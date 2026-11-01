//! The connection form. It holds text only, and it builds a definition on request.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use zeroize::Zeroizing;

use crate::connection::{
    DEFAULT_BUSY_SECONDS, DEFAULT_CONNECT_SECONDS, DEFAULT_POOL_LIMIT, DEFAULT_SQL_SECONDS,
    Definition, Source,
};
use crate::connector::sqlite::{SqliteTarget, validate_path};
use crate::connector::target::{
    Address, ClientCertificate, Engine, NetworkTarget, Secret, Tls, TlsFiles, parse_uri,
};
use crate::exec::ConnectionId;
use crate::settings::next_optional;
use crate::store::Scope;
use crate::store::config::{ConnectionIcon, CredentialName, ThemeName};
use crate::ui::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
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
    Pool,
    ReadOnly,
    Production,
    ConnectTimeout,
    SqlTimeout,
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
            Field::Kind
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

    /// Returns true for a field that holds the path of a file.
    pub fn is_path(self) -> bool {
        matches!(
            self,
            Field::File | Field::CaFile | Field::ClientCertificate | Field::ClientKey
        )
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
            note: None,
            test: None,
            cursor: 0,
            texts: HashMap::new(),
            tls_chosen: false,
        };
        form.set(Field::Pool, DEFAULT_POOL_LIMIT.to_string());
        form.set(Field::ConnectTimeout, DEFAULT_CONNECT_SECONDS.to_string());
        form.set(Field::BusyTimeout, DEFAULT_BUSY_SECONDS.to_string());
        form.set(Field::SqlTimeout, DEFAULT_SQL_SECONDS.to_string());
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
        match &definition.source {
            Source::Sqlite(target) => {
                form.kind = Kind::Sqlite;
                form.foreign_keys = target.foreign_keys;
                form.stored_file = Some(path_text(&target.path));
                form.set(Field::File, path_text(&target.path));
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
            return vec![
                Field::Name,
                Field::Theme,
                Field::Icon,
                Field::Production,
                Field::Disconnect,
                Field::Save,
                Field::Cancel,
            ];
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
        fields.extend([Field::Pool, Field::ReadOnly, Field::Production]);
        if self.kind != Kind::Sqlite {
            fields.push(Field::ConnectTimeout);
        }
        fields.extend([
            Field::SqlTimeout,
            Field::Theme,
            Field::Icon,
            Field::Test,
            Field::Save,
            Field::Cancel,
        ]);
        fields
    }

    pub fn focus_on(&mut self, field: Field) {
        self.focus = field;
        self.cursor = self.focused_len();
    }

    fn move_focus(&mut self, forward: bool) {
        let fields = self.fields();
        let count = fields.len();
        let index = fields
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0);
        let next = (index + if forward { 1 } else { count - 1 }) % count;
        self.focus_on(fields[next]);
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

    /// Writes pasted text in a text field. A form field holds one line.
    pub fn on_paste(&mut self, text: &str) {
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
        let style = self.focus.style();
        let in_text = matches!(style, FieldStyle::Text | FieldStyle::Secret);
        match key.code {
            KeyCode::Esc => return FormRequest::Cancel,
            KeyCode::Tab | KeyCode::Down => self.move_focus(true),
            KeyCode::BackTab | KeyCode::Up => self.move_focus(false),
            KeyCode::Enter => return self.on_enter(),
            KeyCode::Char('o')
                if key.modifiers == KeyModifiers::CONTROL && self.focus.is_path() =>
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
        self.trimmed(field)
            .parse()
            .map_err(|_| text::form_needs_number(label))
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
        if user.is_empty() && engine.needs_user() {
            return Err(text::FORM_NEEDS_USER.to_owned());
        }
        let address = if host.starts_with('/') {
            Address::Socket(PathBuf::from(host))
        } else {
            let port = match self.trimmed(Field::Port) {
                "" => engine.default_port(),
                port => port
                    .parse()
                    .map_err(|_| text::form_needs_number(text::FIELD_PORT))?,
            };
            Address::Tcp {
                host: host.to_owned(),
                port,
            }
        };
        let tls = self.tls()?;
        let database = Some(self.trimmed(Field::Database)).filter(|text| !text.is_empty());
        if database.is_some_and(|name| !engine.accepts_database(name)) {
            return Err(text::FORM_REDIS_DATABASE.to_owned());
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
        validate_path(Path::new(file)).map_err(|error| error.message)?;
        let mut target = SqliteTarget::new(self.file_path(file)?);
        target.foreign_keys = self.foreign_keys;
        let seconds = self.number(Field::BusyTimeout, text::FIELD_BUSY)?;
        target.busy_timeout = Duration::from_secs(seconds);
        Ok(target)
    }

    /// Returns the path of the SQLite file. A path that the user writes starts at the
    /// current folder, and it becomes a full path, so the test and the saved connection
    /// open the same file. A saved path keeps its meaning in the configuration.
    fn file_path(&self, file: &str) -> Result<PathBuf, String> {
        if self.stored_file.as_deref() == Some(file) {
            return Ok(PathBuf::from(file));
        }
        std::path::absolute(file).map_err(|_| text::FORM_NO_CURRENT_FOLDER.to_owned())
    }

    /// Builds the definition. The error text names the field and holds no input,
    /// because a connection text can hold a password.
    pub fn definition(&self) -> Result<Definition, String> {
        let name = self.trimmed(Field::Name);
        if name.is_empty() {
            return Err(text::FORM_NEEDS_NAME.to_owned());
        }
        let source = match self.kind.engine() {
            None => Source::Sqlite(self.sqlite_target()?),
            Some(engine) if self.use_uri => Source::Network(self.uri_target(engine)?),
            Some(engine) => Source::Network(self.network_target(engine)?),
        };
        let limit = self.number(Field::Pool, text::FIELD_POOL)?;
        if limit == 0 {
            return Err(text::FORM_POOL_ZERO.to_owned());
        }
        let mut definition = Definition::new(name, source);
        definition.read_only = self.read_only;
        definition.production = self.production;
        definition.theme = self.theme;
        definition.icon = self.icon;
        definition.max_connections = usize::try_from(limit).unwrap_or(usize::MAX);
        if self.kind != Kind::Sqlite {
            let seconds = self.number(Field::ConnectTimeout, text::FIELD_CONNECT)?;
            if seconds == 0 {
                return Err(text::form_needs_number(text::FIELD_CONNECT));
            }
            definition.connect_timeout = Duration::from_secs(seconds);
        }
        // A limit of 0 seconds means that the statements have no time limit.
        let seconds = self.number(Field::SqlTimeout, text::FIELD_SQL_TIMEOUT)?;
        definition.sql_timeout = (seconds > 0).then(|| Duration::from_secs(seconds));
        Ok(definition)
    }
}
