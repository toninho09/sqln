//! The last choices of the export and import dialogs. `config.json` keeps them in
//! `preferences.export` and `preferences.import`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The rows of one batch of an export or an import, without a saved choice.
pub const DEFAULT_BATCH: usize = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    #[default]
    Csv,
    Tsv,
    Json,
    JsonLines,
    SqlInsert,
}

impl ExportFormat {
    pub const ALL: [Self; 5] = [
        Self::Csv,
        Self::Tsv,
        Self::Json,
        Self::JsonLines,
        Self::SqlInsert,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Json => "json",
            Self::JsonLines => "jsonl",
            Self::SqlInsert => "sql",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Delimiter {
    #[default]
    Comma,
    Semicolon,
    Pipe,
}

impl Delimiter {
    pub const ALL: [Self; 3] = [Self::Comma, Self::Semicolon, Self::Pipe];

    pub fn symbol(self) -> u8 {
        match self {
            Self::Comma => b',',
            Self::Semicolon => b';',
            Self::Pipe => b'|',
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NullText {
    #[default]
    Empty,
    Word,
}

impl NullText {
    pub fn text(self) -> &'static str {
        match self {
            Self::Empty => "",
            Self::Word => "NULL",
        }
    }
}

/// What the modal remembers. `config.json` keeps these values in `preferences.export`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default, rename_all = "camelCase")]
pub struct ExportChoices {
    pub format: ExportFormat,
    pub header: bool,
    pub delimiter: Delimiter,
    pub null_text: NullText,
    pub bom: bool,
    pub grouped: bool,
    pub batch: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

impl Default for ExportChoices {
    fn default() -> Self {
        Self {
            format: ExportFormat::default(),
            header: true,
            delimiter: Delimiter::default(),
            null_text: NullText::default(),
            bom: false,
            grouped: false,
            batch: DEFAULT_BATCH,
            folder: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Encoding {
    #[default]
    Utf8,
    Windows1252,
}

impl Encoding {
    pub const ALL: [Self; 2] = [Self::Utf8, Self::Windows1252];
}

/// What the dialog remembers. `config.json` keeps these values in `preferences.import`.
/// The failure mode stays out, because two modes leave partial data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default, rename_all = "camelCase")]
pub struct ImportChoices {
    pub encoding: Encoding,
    pub header: bool,
    pub delimiter: Delimiter,
    pub null_text: NullText,
    pub batch: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

impl Default for ImportChoices {
    fn default() -> Self {
        Self {
            encoding: Encoding::default(),
            header: true,
            delimiter: Delimiter::default(),
            null_text: NullText::default(),
            batch: DEFAULT_BATCH,
            folder: None,
        }
    }
}
