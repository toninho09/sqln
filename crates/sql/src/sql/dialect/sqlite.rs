use super::{BASE, SqlDialect, Syntax};
use crate::sql::parameters::{Kind, Parameter};
use crate::sql::quote_literal;

pub struct Sqlite;

const SYNTAX: Syntax = Syntax {
    bracket_identifier: true,
    at_marker: true,
    ..BASE
};

impl SqlDialect for Sqlite {
    fn syntax(&self) -> &'static Syntax {
        &SYNTAX
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        Some(&sqlparser::dialect::SQLiteDialect {})
    }

    fn text_literal(&self, value: &str) -> String {
        quote_literal(value, crate::sql::Dialect::Sqlite)
    }

    fn parameter_marker(&self, _: usize, parameter: &mut Parameter) -> Result<String, String> {
        Ok(match parameter.kind {
            Kind::Number => "CAST(? AS NUMERIC)",
            _ => "?",
        }
        .to_owned())
    }
}
