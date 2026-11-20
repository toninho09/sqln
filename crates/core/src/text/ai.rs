//! AI requests and explanations.

use super::HintPart;

pub const HINT_AI_REQUEST: &[HintPart] = &[("Enter", "sends"), ("Esc", "cancels")];
pub const TITLE_AI_REQUEST: &str = "Complete with AI with a request";
pub const NOTE_AI_REQUEST: &str = "Optional. An empty input sends a plain completion.";
pub const AI_EXPLAIN_ERROR_TITLE: &str = "AI error explanation";
pub const AI_EXPLAIN_PLAN_TITLE: &str = "AI plan explanation";
pub const AI_EXPLAIN_NO_ERROR: &str =
    "No failed statement for the current connection. Run a statement and try again.";
pub const AI_EXPLAIN_NO_PLAN: &str =
    "No query plan on screen. Show the plan of a statement and try again.";
pub const AI_EXPLAIN_DISABLED: &str = "AI is unavailable. Enable it in Settings > Autocomplete.";
pub const AI_EXPLAIN_NO_PROVIDER: &str = "Configure a provider in Settings > Autocomplete.";
pub const AI_EXPLAIN_BUSY: &str =
    "An AI explanation of this console already runs. Wait for its answer or cancel it.";
pub const AI_CURSOR_ONLY: &str = "AI completion works at the cursor, not on a selection.";
