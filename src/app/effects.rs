//! The changes that a feature asks of the application. A feature decides with its own
//! state and the data that it reads, and returns effects. `App::apply` is the one place
//! that writes the status line, the clipboard, and the new consoles for them, so the
//! decision can be tested without an `App`.

use super::App;
use crate::exec::ConnectionId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Effect {
    /// Shows a message on the status line.
    Status(String),
    /// Copies a text and reports the result on the status line.
    Copy(String),
    /// Opens a new console of a connection with a text. Nothing runs.
    OpenTextConsole {
        connection: ConnectionId,
        schema: Option<String>,
        text: String,
    },
}

impl Effect {
    /// The effect of a refusal: its reason on the status line.
    pub(super) fn refused(reason: &str) -> Self {
        Self::Status(reason.to_owned())
    }
}

impl App {
    pub(super) fn apply(&mut self, effects: impl IntoIterator<Item = Effect>) {
        for effect in effects {
            match effect {
                Effect::Status(message) => self.status = Some(message),
                Effect::Copy(text) => self.copy(&text),
                Effect::OpenTextConsole {
                    connection,
                    schema,
                    text,
                } => self.open_text_console(connection, schema, &text),
            }
        }
    }
}
