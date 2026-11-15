//! Built-in types and function signatures. These lists do not use the connection.

use crate::dialect::features;
use crate::sql::Dialect;

pub fn available_types(dialect: Dialect, cast: bool) -> impl Iterator<Item = &'static str> {
    let vocabulary = features(dialect).vocabulary();
    let types = match vocabulary.cast_types {
        Some(cast_types) if cast => cast_types,
        _ => vocabulary.types,
    };
    types
        .iter()
        .copied()
        .filter(move |word| !cast || !word.ends_with("SERIAL"))
}

pub(crate) const COMMON_FUNCTIONS: &[(&str, &str)] = &[
    ("COUNT", "COUNT(expression | *)"),
    ("SUM", "SUM(expression)"),
    ("AVG", "AVG(expression)"),
    ("MIN", "MIN(expression)"),
    ("MAX", "MAX(expression)"),
    ("LOWER", "LOWER(text)"),
    ("UPPER", "UPPER(text)"),
    ("TRIM", "TRIM(text)"),
    ("REPLACE", "REPLACE(text, from_text, to_text)"),
    ("COALESCE", "COALESCE(value, ...)"),
    ("NULLIF", "NULLIF(value, other)"),
    ("ABS", "ABS(number)"),
    ("ROUND", "ROUND(number, digits)"),
    ("LENGTH", "LENGTH(value)"),
];

pub fn functions(dialect: Dialect) -> impl Iterator<Item = (&'static str, &'static str)> {
    features(dialect)
        .vocabulary()
        .functions
        .iter()
        .flat_map(|list| list.iter().copied())
}

pub const EXPRESSION_WORDS: &[&str] = &[
    "CASE",
    "CAST",
    "DISTINCT",
    "EXISTS",
    "FALSE",
    "NOT",
    "NULL",
    "TRUE",
    "CURRENT_DATE",
    "CURRENT_TIMESTAMP",
];
