//! A message of the Messages panel: a level and a text.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageLevel {
    Info,
    /// A notice or a warning of the database.
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub level: MessageLevel,
    pub text: String,
}

impl Message {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            level: MessageLevel::Info,
            text: text.into(),
        }
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self {
            level: MessageLevel::Warning,
            text: text.into(),
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            level: MessageLevel::Error,
            text: text.into(),
        }
    }
}
