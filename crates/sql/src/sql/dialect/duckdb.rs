use super::{BASE, SqlDialect, Syntax};
use crate::sql::parameters::Parameter;
use crate::sql::quote_literal;

pub struct DuckDb;

const SYNTAX: Syntax = Syntax {
    dotted_schema: true,
    ..BASE
};

impl SqlDialect for DuckDb {
    fn syntax(&self) -> &'static Syntax {
        &SYNTAX
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        Some(&sqlparser::dialect::DuckDbDialect {})
    }

    fn text_literal(&self, value: &str) -> String {
        quote_literal(value, crate::sql::Dialect::DuckDb)
    }

    fn parameter_marker(&self, _: usize, _: &mut Parameter) -> Result<String, String> {
        Ok("?".to_owned())
    }
}
