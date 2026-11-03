//! The Mermaid `erDiagram` text of a model. The text holds what the boxes show.

use std::fmt::Write;

use super::Model;
use crate::grid::printable;

/// Writes the model as a Mermaid ER diagram. Each entity uses the id `e<n>` and shows its
/// name as an alias, because a database name can hold any character.
pub fn to_mermaid(model: &Model, keys_only: bool) -> String {
    let mut text = String::from("erDiagram\n");
    for (index, entity) in model.entities.iter().enumerate() {
        let label = if entity.stub {
            format!("{}.{}", entity.relation.schema, entity.relation.relation)
        } else {
            entity.relation.relation.clone()
        };
        let _ = write!(text, "    e{}[\"{}\"]", index + 1, quoted(&label));
        let shown = entity.shown(keys_only);
        if entity.stub || shown.is_empty() {
            text.push('\n');
            continue;
        }
        text.push_str(" {\n");
        for column in shown.into_iter().map(|index| &entity.columns[index]) {
            let name = word(&column.name, 'c');
            let _ = write!(text, "        {} {name}", word(&column.ty, 't'));
            let keys: Vec<&str> = [(column.primary.is_some(), "PK"), (column.foreign, "FK")]
                .into_iter()
                .filter_map(|(on, key)| on.then_some(key))
                .collect();
            if !keys.is_empty() {
                let _ = write!(text, " {}", keys.join(", "));
            }
            if name != column.name {
                let _ = write!(text, " \"{}\"", quoted(&column.name));
            }
            text.push('\n');
        }
        text.push_str("    }\n");
    }
    for link in &model.links {
        let parent = if model.is_optional(link) { "|o" } else { "||" };
        let line = if model.is_identifying(link) {
            "--"
        } else {
            ".."
        };
        let child = &model.entities[link.child];
        let label = if link.child_columns.is_empty() {
            link.name.clone()
        } else {
            let names: Vec<&str> = link
                .child_columns
                .iter()
                .map(|&column| child.columns[column].name.as_str())
                .collect();
            names.join(", ")
        };
        let _ = writeln!(
            text,
            "    e{} {parent}{line}o{{ e{} : \"{}\"",
            link.parent + 1,
            link.child + 1,
            quoted(&label)
        );
    }
    text
}

/// Writes text for a place between double quotes, where Mermaid accepts no double quote.
fn quoted(text: &str) -> String {
    printable(text).replace('"', "'")
}

/// Writes a name or a type as one Mermaid word. A word starts with a letter and holds
/// letters, digits, `-`, `_`, parentheses, and square brackets. A word that does not start
/// with a letter gets `first` before it. An empty text gives `any`.
fn word(text: &str, first: char) -> String {
    if text.is_empty() {
        return "any".to_owned();
    }
    let mut word: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '(' | ')' | '[' | ']') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !word.chars().next().is_some_and(char::is_alphabetic) {
        word.insert(0, first);
    }
    word
}
