use super::{BASE, Backslash, BinaryLiteral, SqlDialect, Syntax};
use crate::sql::escape_literal;
use crate::sql::parameters::Parameter;

pub struct Postgres;

const SYNTAX: Syntax = Syntax {
    nested_block_comment: true,
    string_backslash: Backslash::AfterEscapePrefix,
    backtick_identifier: false,
    dollar_quote: true,
    begin_needs_atomic: true,
    question_marker: false,
    escaped_name_literal: true,
    binary_literal: BinaryLiteral::HexText,
    booleans: ("true", "false"),
    folds_to_lower: true,
    ..BASE
};

impl SqlDialect for Postgres {
    fn syntax(&self) -> &'static Syntax {
        &SYNTAX
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        Some(&sqlparser::dialect::PostgreSqlDialect {})
    }

    /// The `E` form does not change with `standard_conforming_strings`.
    fn text_literal(&self, value: &str) -> String {
        escape_literal(value)
    }

    fn parameter_marker(&self, number: usize, _: &mut Parameter) -> Result<String, String> {
        Ok(format!("${number}"))
    }
}
