//! The decisions of the auto-refresh that need no `App`: the intervals, and which runs
//! may repeat without a question.

use crate::sql::{Dialect, is_known_redis_read, is_plain_read};
use crate::ui::text;

/// The intervals of the dialog, in seconds. `None` turns the refresh off.
pub(super) const INTERVALS: [Option<u64>; 7] = [
    None,
    Some(1),
    Some(2),
    Some(5),
    Some(10),
    Some(30),
    Some(60),
];

/// Returns true when every statement is a read that an auto-refresh can repeat without a
/// question. A Redis command must be a known read. A continuous command never ends, so
/// it cannot repeat.
pub(super) fn statements_are_reads(statements: &[String], dialect: Dialect) -> bool {
    statements.iter().all(|sql| {
        if crate::sql::is_continuous(sql, dialect) {
            false
        } else if dialect == Dialect::Redis {
            is_known_redis_read(sql)
        } else if dialect == Dialect::Kafka {
            crate::sql::is_known_kafka_read(sql)
        } else {
            is_plain_read(sql, dialect)
        }
    })
}

/// Returns the reason that a console cannot repeat its last run. An auto-refresh never
/// asks the run question, so it repeats reads only.
pub(super) fn console_refusal(
    last_run: Option<&[String]>,
    dialect: Dialect,
) -> Option<&'static str> {
    match last_run {
        None => Some(text::AUTO_REFRESH_NEEDS_RUN),
        Some(statements) if !statements_are_reads(statements, dialect) => {
            Some(text::AUTO_REFRESH_READ_ONLY)
        }
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(statements: &[&str]) -> Vec<String> {
        statements.iter().map(|sql| (*sql).to_owned()).collect()
    }

    #[test]
    fn only_a_run_of_reads_repeats() {
        let postgres = Dialect::Postgres;
        assert_eq!(
            console_refusal(None, postgres),
            Some(text::AUTO_REFRESH_NEEDS_RUN)
        );
        let reads = run(&["SELECT 1", "SELECT 2"]);
        assert_eq!(console_refusal(Some(&reads), postgres), None);
        let change = run(&["SELECT 1", "DELETE FROM t"]);
        let refused = console_refusal(Some(&change), postgres);
        assert_eq!(refused, Some(text::AUTO_REFRESH_READ_ONLY));
    }

    #[test]
    fn redis_repeats_known_reads_only() {
        assert!(statements_are_reads(&run(&["GET k"]), Dialect::Redis));
        assert!(!statements_are_reads(&run(&["SET k v"]), Dialect::Redis));
    }

    #[test]
    fn the_dialog_starts_with_off() {
        assert_eq!(INTERVALS[0], None);
        assert!(INTERVALS[1..].windows(2).all(|pair| pair[0] < pair[1]));
    }
}
