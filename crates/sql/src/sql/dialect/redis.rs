//! Redis and Valkey commands. This text is not SQL, so the lexer and the splitter use the
//! command-line paths, and most rules here are not read.

use super::{BASE, SqlDialect, Syntax};
use crate::catalog::RelationRef;
use crate::sql::parameters::Parameter;
use crate::sql::quote_literal;
use crate::structure::{Scope, Section, SessionSignal};

pub struct Redis;

impl SqlDialect for Redis {
    fn syntax(&self) -> &'static Syntax {
        &BASE
    }

    fn parser(&self) -> Option<&'static dyn sqlparser::dialect::Dialect> {
        None
    }

    fn text_literal(&self, value: &str) -> String {
        quote_literal(value, crate::sql::Dialect::Redis)
    }

    fn parameter_marker(&self, _: usize, _: &mut Parameter) -> Result<String, String> {
        Ok("?".to_owned())
    }

    fn structure_sql(&self, _: Scope, _: &RelationRef, _: Section) -> Option<String> {
        None
    }

    fn signal_sql(&self, _: SessionSignal, _: u64) -> Option<String> {
        None
    }
}
