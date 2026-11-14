//! The saved settings of the AI completion. `config.json` keeps them in
//! `preferences.aiCompletion`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// The most AI answers that wait at once in one console: ghosts that show and
    /// requests that run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_simultaneous: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Provider>,
}

impl Preferences {
    pub fn resolve(global: Option<&Self>, project: Option<&Self>) -> Self {
        Self {
            enabled: Some(
                project
                    .and_then(|p| p.enabled)
                    .or_else(|| global.and_then(|p| p.enabled))
                    .unwrap_or(false),
            ),
            max_simultaneous: project
                .and_then(|p| p.max_simultaneous)
                .or_else(|| global.and_then(|p| p.max_simultaneous)),
            provider: project
                .and_then(|p| p.provider.clone())
                .or_else(|| global.and_then(|p| p.provider.clone())),
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled == Some(true)
    }
    /// Returns the most answers that wait at once in one console. The default is 3.
    pub fn max_simultaneous(&self) -> usize {
        usize::from(self.max_simultaneous.unwrap_or(3).clamp(1, 9))
    }
}

/// The provider kinds of saved settings. All kinds use the same
/// OpenAI-compatible chat standard; Deepseek and Openrouter are legacy values
/// that SQLN accepts and treats like Compatible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Deepseek,
    Openrouter,
    #[default]
    Compatible,
}

/// The protocol values of saved settings. The completion always uses the
/// OpenAI chat standard. SQLN accepts the old values and ignores them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    #[default]
    Chat,
    /// Accepts any unknown saved value, including the old `fim`.
    #[serde(other)]
    Fim,
}

/// The reasoning effort of the provider. `Off` sends no field, so the
/// endpoint uses its default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reasoning {
    Low,
    Medium,
    High,
    /// The explicit request that the model does not think.
    None,
    #[default]
    /// Accepts any unknown saved value. The request omits the field.
    #[serde(other)]
    Off,
}

impl Reasoning {
    /// The order of the choices of the settings.
    pub const ALL: [Reasoning; 5] = [
        Reasoning::Off,
        Reasoning::None,
        Reasoning::Low,
        Reasoning::Medium,
        Reasoning::High,
    ];

    /// Returns true when the request carries no field.
    pub fn is_off(&self) -> bool {
        *self == Reasoning::Off
    }

    /// Returns the value of the request body, or none for `Off`.
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Reasoning::Off => None,
            Reasoning::None => Some("none"),
            Reasoning::Low => Some("low"),
            Reasoning::Medium => Some("medium"),
            Reasoning::High => Some("high"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "camelCase", deny_unknown_fields)]
pub enum Credential {
    #[default]
    None,
    Vault {
        id: String,
        endpoint: String,
    },
    File {
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provider {
    pub kind: Kind,
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    /// Legacy field of saved settings. New saves omit it.
    #[serde(default, skip_serializing)]
    pub protocol: Protocol,
    #[serde(default)]
    pub credential: Credential,
    #[serde(default = "default_context")]
    pub context_window_tokens: usize,
    #[serde(default = "default_output")]
    pub max_output_tokens: usize,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    /// The reasoning effort of the request. New saves omit `Off`.
    #[serde(default, skip_serializing_if = "Reasoning::is_off")]
    pub reasoning_effort: Reasoning,
}
const fn default_context() -> usize {
    32768
}
const fn default_output() -> usize {
    1024
}
const fn default_timeout() -> u64 {
    60
}
impl Default for Provider {
    fn default() -> Self {
        Self {
            kind: Kind::Compatible,
            model: String::new(),
            base_url: String::new(),
            protocol: Protocol::Chat,
            credential: Credential::None,
            context_window_tokens: default_context(),
            max_output_tokens: default_output(),
            timeout_seconds: default_timeout(),
            reasoning_effort: Reasoning::Off,
        }
    }
}
