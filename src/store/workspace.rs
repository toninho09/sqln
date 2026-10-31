//! `workspace.json`: the state of the interface that stays on one machine. The file
//! is not versioned. A damaged file gives an empty state, because the data is not
//! important.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::file::{self, Expect};
use super::{Root, Scope};

pub const VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorState {
    pub cursor: Position,
    pub scroll: Position,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub navigator_collapsed: bool,
    pub navigator_width: u16,
    /// The share of the editor in the console, from 0 to 1.
    pub editor_ratio: f64,
}

/// One tab of the tab row. The data lets a later start open the tab again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OpenTab {
    Console { scope: Scope, id: String },
    Table { scope: Scope, connection: String, schema: String, relation: String, view: bool },
}

/// The tabs at the end of the last session, in the order of the tab row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenTabs {
    pub tabs: Vec<OpenTab>,
    /// The index in `tabs` of the tab on screen.
    pub active: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub version: u32,
    #[serde(default)]
    pub consoles: BTreeMap<String, EditorState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_tabs: Option<OpenTabs>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self { version: VERSION, consoles: BTreeMap::new(), layout: None, open_tabs: None }
    }
}

pub fn load(root: &Root) -> Workspace {
    read(root).unwrap_or_default()
}

fn read(root: &Root) -> Option<Workspace> {
    std::fs::read(root.workspace_file())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Workspace>(&bytes).ok())
        .filter(|workspace| workspace.version == VERSION)
}

/// Returns true for a file that exists and that `load` must ignore.
pub fn is_unreadable(root: &Root) -> bool {
    root.workspace_file().exists() && read(root).is_none()
}

/// Reads the file again, applies the change, and writes the file.
pub fn update(root: &Root, change: impl FnOnce(&mut Workspace)) -> Result<(), String> {
    let _lock = file::lock(&root.locks_dir().join("config.lock")).map_err(|error| error.to_string())?;
    let mut workspace = load(root);
    change(&mut workspace);
    let mut text = serde_json::to_string_pretty(&workspace).map_err(|error| error.to_string())?;
    text.push('\n');
    file::write_atomic(&root.workspace_file(), text.as_bytes(), Expect::Any)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
