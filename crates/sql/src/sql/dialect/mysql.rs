use super::{BASE, Backslash, SqlDialect, Syntax};
use crate::grid::hex;
use crate::sql::parameters::{Kind, Parameter, mysql_number};
use crate::sql::quote_literal;

pub struct MySql;

const SYNTAX: Syntax = Syntax {
    dash_comment_needs_space: true,
    hash_comment: true,
    string_backslash: Backslash::Always,
    double_quote_string: true,
    delimiter_command: true,
    identifier_quote: '`',
    literal_backslash: true,
    ..BASE
};

impl SqlDialect for MySql {
    fn syntax(&self) -> &'static Syntax {
        &SYNTAX
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        Some(&sqlparser::dialect::MySqlDialect {})
    }

    /// The hexadecimal form does not change with `NO_BACKSLASH_ESCAPES`.
    fn text_literal(&self, value: &str) -> String {
        if value.contains('\\') {
            format!("_utf8mb4 {}", hex(value.as_bytes()))
        } else {
            quote_literal(value, crate::sql::Dialect::MySql)
        }
    }

    /// MySQL binds strings, so a typed cast keeps the precision of a number.
    fn parameter_marker(&self, _: usize, parameter: &mut Parameter) -> Result<String, String> {
        if parameter.kind != Kind::Number {
            return Ok("?".to_owned());
        }
        let (value, precision, scale) = mysql_number(&parameter.value)?;
        parameter.value = value;
        Ok(format!("CAST(? AS DECIMAL({precision},{scale}))"))
    }
}
