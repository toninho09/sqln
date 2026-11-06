//! Scoped autocomplete settings. Editing does not contact a provider.
use super::{Credential, Kind, Preferences, Protocol, Provider};
use crate::line_input::LineInput;
use crate::store::Scope;
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Pages,
    Enabled,
    Profile,
    Kind,
    Model,
    Endpoint,
    Protocol,
    Source,
    Path,
    Key,
    Context,
    Output,
    Timeout,
    Test,
    Apply,
    Cancel,
}
pub enum Request {
    None,
    Cancel,
    Apply,
    Test,
}
pub struct Form {
    pub scope: Scope,
    pub page: usize,
    pub focus: Field,
    pub enabled: Option<bool>,
    pub inherit: bool,
    pub provider: Provider,
    pub model: LineInput,
    pub endpoint: LineInput,
    pub path: LineInput,
    pub key: LineInput,
    pub context: LineInput,
    pub output: LineInput,
    pub timeout: LineInput,
    pub source: usize,
    pub note: Option<String>,
    pub busy: bool,
    pub inherited_enabled: bool,
    inherited: Option<Provider>,
}
fn input(text: &str) -> LineInput {
    let mut i = LineInput::default();
    i.set(text);
    i
}
impl Form {
    pub fn new(scope: Scope, local: Option<&Preferences>, global: Option<&Preferences>) -> Self {
        let effective =
            Preferences::resolve(global, if scope == Scope::Project { local } else { None });
        let provider = local
            .and_then(|p| p.provider.clone())
            .or(effective.provider.clone())
            .unwrap_or_default();
        let source = match &provider.credential {
            Credential::None => 0,
            Credential::Vault { .. } => 1,
            Credential::File { .. } => 2,
        };
        let path = match &provider.credential {
            Credential::File { path } => path.as_str(),
            _ => "",
        };
        Self {
            scope,
            page: 0,
            focus: Field::Pages,
            enabled: local.and_then(|p| p.enabled),
            inherit: local.is_none_or(|p| p.provider.is_none()),
            model: input(&provider.model),
            endpoint: input(&provider.base_url),
            path: input(path),
            key: LineInput::default(),
            context: input(&provider.context_window_tokens.to_string()),
            output: input(&provider.max_output_tokens.to_string()),
            timeout: input(&provider.timeout_seconds.to_string()),
            provider,
            source,
            note: None,
            busy: false,
            inherited_enabled: global.is_some_and(Preferences::enabled),
            inherited: effective.provider,
        }
    }
    pub fn fields(&self) -> Vec<Field> {
        use Field::*;
        let mut fields = vec![Pages];
        fields.extend(match self.page {
            0 => vec![Enabled, Profile, Kind, Model, Endpoint, Protocol],
            1 => vec![Source, Path, Key],
            _ => vec![Context, Output, Timeout],
        });
        fields.extend([Test, Apply, Cancel]);
        fields
    }
    pub fn input(&self, field: Field) -> Option<&LineInput> {
        match field {
            Field::Model => Some(&self.model),
            Field::Endpoint => Some(&self.endpoint),
            Field::Path => Some(&self.path),
            Field::Key => Some(&self.key),
            Field::Context => Some(&self.context),
            Field::Output => Some(&self.output),
            Field::Timeout => Some(&self.timeout),
            _ => None,
        }
    }
    fn input_mut(&mut self) -> Option<&mut LineInput> {
        match self.focus {
            Field::Model => Some(&mut self.model),
            Field::Endpoint => Some(&mut self.endpoint),
            Field::Path => Some(&mut self.path),
            Field::Key => Some(&mut self.key),
            Field::Context => Some(&mut self.context),
            Field::Output => Some(&mut self.output),
            Field::Timeout => Some(&mut self.timeout),
            _ => None,
        }
    }
    pub fn paste(&mut self, text: &str) {
        if !self.busy
            && let Some(input) = self.input_mut()
        {
            input.insert(text);
            self.inherit = false;
        }
    }
    pub fn value(&self, field: Field) -> String {
        use Field::*;
        match field {
            Enabled => match self.enabled {
                Some(true) => "On".into(),
                Some(false) => "Off".into(),
                None => format!(
                    "Inherited ({})",
                    if self.scope == Scope::Project && self.inherited_enabled {
                        "On"
                    } else {
                        "Off"
                    }
                ),
            },
            Profile => if self.inherit { "Inherited" } else { "Custom" }.into(),
            Kind => format!("{:?}", self.provider.kind),
            Protocol => format!("{:?}", self.provider.protocol()),
            Source => ["None", "System vault", "Key file"][self.source].into(),
            Pages => ["Provider", "Credentials", "Limits"][self.page].into(),
            Key => {
                if self.key.text().is_empty() {
                    if matches!(self.provider.credential, Credential::Vault { .. }) {
                        "(saved key)"
                    } else {
                        ""
                    }
                    .into()
                } else {
                    "*".repeat(self.key.text().chars().count().min(24))
                }
            }
            _ => self
                .input(field)
                .map(|i| i.text().to_owned())
                .unwrap_or_default(),
        }
    }
    fn change(&mut self, forward: bool) {
        let step = |v: usize, n: usize| {
            if forward {
                (v + 1) % n
            } else {
                (v + n - 1) % n
            }
        };
        match self.focus {
            Field::Pages => self.page = step(self.page, 3),
            Field::Enabled => {
                self.enabled = [None, Some(true), Some(false)][step(
                    match self.enabled {
                        None => 0,
                        Some(true) => 1,
                        Some(false) => 2,
                    },
                    3,
                )]
            }
            Field::Profile => self.inherit = !self.inherit,
            Field::Kind => {
                self.provider.kind = [Kind::Deepseek, Kind::Openrouter, Kind::Compatible][step(
                    match self.provider.kind {
                        Kind::Deepseek => 0,
                        Kind::Openrouter => 1,
                        Kind::Compatible => 2,
                    },
                    3,
                )];
                self.model.set("");
                self.endpoint.set("");
                self.source = 0;
                self.provider.credential = Credential::None;
                self.key.set("");
                self.inherit = false;
            }
            Field::Protocol if self.provider.kind == Kind::Compatible => {
                self.provider.protocol = if self.provider.protocol == Protocol::Fim {
                    Protocol::Chat
                } else {
                    Protocol::Fim
                };
                self.inherit = false;
            }
            Field::Source => {
                self.source = step(self.source, 3);
                self.inherit = false;
            }
            _ => {}
        }
    }
    pub fn press(&mut self) -> Request {
        if self.busy {
            return Request::None;
        }
        match self.focus {
            Field::Cancel => Request::Cancel,
            Field::Apply => Request::Apply,
            Field::Test => Request::Test,
            _ => {
                self.change(true);
                Request::None
            }
        }
    }
    pub fn on_key(&mut self, key: KeyEvent) -> Request {
        if key.code == KeyCode::Esc {
            return Request::Cancel;
        }
        if self.busy {
            return Request::None;
        }
        let fields = self.fields();
        let index = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        match key.code {
            KeyCode::Tab | KeyCode::Down => self.focus = fields[(index + 1) % fields.len()],
            KeyCode::BackTab | KeyCode::Up => {
                self.focus = fields[(index + fields.len() - 1) % fields.len()]
            }
            KeyCode::Enter => return self.press(),
            KeyCode::Left | KeyCode::Right if self.input(self.focus).is_none() => {
                self.change(key.code == KeyCode::Right)
            }
            KeyCode::F(n) if (1..=3).contains(&n) => {
                self.page = usize::from(n - 1);
                self.focus = Field::Pages;
            }
            _ => {
                if let Some(input) = self.input_mut() {
                    input.on_key(key);
                    self.inherit = false;
                }
            }
        }
        Request::None
    }
    pub fn preferences(&self) -> Result<Preferences, String> {
        if self.key.text().len() > 65536 {
            return Err("The API key exceeds 64 KiB. Supply only the key.".into());
        }
        if self.inherit {
            return Ok(Preferences {
                enabled: self.enabled,
                provider: None,
            });
        }
        let mut provider = self.provider.clone();
        provider.model = self.model.text().to_owned();
        provider.base_url = self.endpoint.text().to_owned();
        provider.context_window_tokens = self
            .context
            .text()
            .parse()
            .map_err(|_| "Set a valid context size.")?;
        provider.max_output_tokens = self
            .output
            .text()
            .parse()
            .map_err(|_| "Set a valid output size.")?;
        provider.timeout_seconds = self
            .timeout
            .text()
            .parse()
            .map_err(|_| "Set a valid timeout.")?;
        provider.credential = match self.source {
            0 => Credential::None,
            2 => Credential::File {
                path: self.path.text().to_owned(),
            },
            _ => {
                if !self.key.text().is_empty() {
                    Credential::Vault {
                        id: uuid::Uuid::new_v4().to_string(),
                        endpoint: provider.endpoint()?,
                    }
                } else {
                    match &provider.credential {
                        Credential::Vault { .. } => provider.credential.clone(),
                        _ => return Err("Insert a key for the system vault.".into()),
                    }
                }
            }
        };
        provider.validate()?;
        Ok(Preferences {
            enabled: self.enabled,
            provider: Some(provider),
        })
    }
    pub fn effective_provider(&self) -> Result<Provider, String> {
        self.preferences()?
            .provider
            .or(self.inherited.clone())
            .ok_or_else(|| "Configure a provider before testing.".into())
    }
}
