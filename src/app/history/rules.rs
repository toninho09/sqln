//! The decisions of the query history that need no `App`: which statement it notes.

use crate::exec::StatementStatus;
use crate::sql::{Dialect, is_secret};
use crate::store::history::STATEMENT_LIMIT;

/// Returns whether a statement succeeded, or `None` for a statement that never reached
/// the database or did not end.
pub(super) fn outcome(status: &StatementStatus) -> Option<bool> {
    match status {
        StatementStatus::Success => Some(true),
        StatementStatus::NotExecuted | StatementStatus::Running => None,
        _ => Some(false),
    }
}

/// Returns true when the history keeps a statement: it is not too long, it holds no
/// secret, and it does not repeat the last entry of the same connection.
pub(super) fn worth_noting(
    sql: &str,
    dialect: Dialect,
    connection: &str,
    last: Option<&(String, String)>,
) -> bool {
    if sql.len() > STATEMENT_LIMIT || is_secret(sql, dialect) {
        return false;
    }
    !last.is_some_and(|(id, text)| id == connection && text == sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_statements_that_ended_are_noted() {
        assert_eq!(outcome(&StatementStatus::Success), Some(true));
        assert_eq!(outcome(&StatementStatus::Error("e".into())), Some(false));
        assert_eq!(outcome(&StatementStatus::NotExecuted), None);
        assert_eq!(outcome(&StatementStatus::Running), None);
    }

    #[test]
    fn a_repetition_a_secret_and_a_long_statement_stay_out() {
        let last = ("db".to_owned(), "SELECT 1".to_owned());
        assert!(worth_noting(
            "SELECT 2",
            Dialect::Postgres,
            "db",
            Some(&last)
        ));
        assert!(!worth_noting(
            "SELECT 1",
            Dialect::Postgres,
            "db",
            Some(&last)
        ));
        assert!(worth_noting(
            "SELECT 1",
            Dialect::Postgres,
            "other",
            Some(&last)
        ));
        let secret = "ALTER USER app PASSWORD 'x'";
        assert!(!worth_noting(secret, Dialect::Postgres, "db", None));
        let long = "x".repeat(STATEMENT_LIMIT + 1);
        assert!(!worth_noting(&long, Dialect::Postgres, "db", None));
    }
}
