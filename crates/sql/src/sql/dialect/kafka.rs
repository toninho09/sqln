//! Kafka line commands and the SELECT subset of SQLN. The lexer and the splitter use the
//! command-line paths, and most rules here are not read.

use super::{BASE, SqlDialect, Syntax};
use crate::sql::parameters::Parameter;

pub struct Kafka;

impl SqlDialect for Kafka {
    fn syntax(&self) -> &'static Syntax {
        &BASE
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        None
    }

    /// Backslash escapes, which SELECT and the line commands both read. A doubled quote
    /// works only in SELECT.
    fn text_literal(&self, value: &str) -> String {
        let mut literal = String::with_capacity(value.len() + 2);
        literal.push('\'');
        for c in value.chars() {
            match c {
                '\'' | '\\' => {
                    literal.push('\\');
                    literal.push(c);
                }
                '\n' => literal.push_str("\\n"),
                '\r' => literal.push_str("\\r"),
                '\t' => literal.push_str("\\t"),
                c if c.is_ascii_control() => literal.push_str(&format!("\\x{:02x}", u32::from(c))),
                c => literal.push(c),
            }
        }
        literal.push('\'');
        literal
    }

    fn parameter_marker(&self, _: usize, _: &mut Parameter) -> Result<String, String> {
        Ok("?".to_owned())
    }
}
