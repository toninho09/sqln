//! The parts of the Redis connector that need no driver library.

use super::words::quoted_word;
pub use super::words::split_words;
use crate::catalog::RelationRef;

/// The first word of the text that a key tab sends. It is not a server command.
pub const VALUE_PAGE: &str = "SQLN.VALUE";

/// Returns the text of one page of a key tab. The session reads the type of the key at
/// each page, because a key can change between two pages.
pub fn value_page(relation: &RelationRef, offset: usize, count: usize) -> String {
    format!(
        "{VALUE_PAGE} {} {} {offset} {count}",
        relation.schema,
        quoted_word(&relation.relation)
    )
}
