use super::{BASE, Backslash, BinaryLiteral, SqlDialect, Syntax};
use crate::catalog::RelationRef;
use crate::sql::Dialect;
use crate::sql::escape_literal;
use crate::sql::parameters::Parameter;
use crate::structure::{Scope, Section, SessionSignal, name_literal};

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

    fn structure_sql(
        &self,
        scope: Scope,
        relation: &RelationRef,
        section: Section,
    ) -> Option<String> {
        let schema = name_literal(&relation.schema, Dialect::Postgres);
        let table = name_literal(&relation.relation, Dialect::Postgres);
        Some(match (scope, section) {
            (Scope::Relation, Section::Columns) => columns(&schema, &table),
            (Scope::Relation, Section::Indexes) => indexes(&schema, &table),
            (Scope::Relation, Section::ForeignKeys) => foreign_keys(&schema, &table),
            (Scope::Relation, Section::Statistics) => statistics(&schema, &table),
            (Scope::Schema, _) => all_columns(Some(&schema)),
            (Scope::Connection, _) => all_columns(None),
            (Scope::Sessions, _) => SESSIONS.to_owned(),
            (Scope::Locks, _) => LOCKS.to_owned(),
        })
    }

    fn signal_sql(&self, signal: SessionSignal, session: u64) -> Option<String> {
        Some(match signal {
            SessionSignal::CancelQuery => format!("SELECT pg_catalog.pg_cancel_backend({session})"),
            SessionSignal::EndSession => {
                format!("SELECT pg_catalog.pg_terminate_backend({session})")
            }
        })
    }
}

/// The sessions of each client, without the session that reads them. The active ones come
/// first, with the oldest at the top. The first column is the number that a signal needs,
/// the second is the user, and the last is the text of the query.
const SESSIONS: &str = "\
    SELECT a.pid, a.usename::text AS \"user\", a.datname::text AS database, \
    a.application_name AS application, a.client_addr::text AS client, a.state, \
    a.wait_event_type || ': ' || a.wait_event AS wait, \
    pg_catalog.date_trunc('second', pg_catalog.now() - a.xact_start)::text AS transaction_age, \
    pg_catalog.date_trunc('second', pg_catalog.now() - a.query_start)::text AS query_age, \
    a.query \
    FROM pg_catalog.pg_stat_activity a \
    WHERE a.backend_type = 'client backend' AND a.pid <> pg_catalog.pg_backend_pid() \
    ORDER BY a.state = 'active' DESC NULLS LAST, a.query_start, a.pid";

/// The sessions that wait for a lock, one row for each session that blocks them. The
/// first column is the session that blocks, so a signal frees the wait; the second is
/// its user, and the last is its query. The session that reads the list is not offered
/// as a blocker. A relation of another database has no name here, so it shows its number.
const LOCKS: &str = "\
    SELECT b.pid AS blocking_pid, b.usename::text AS blocking_user, \
    w.pid AS blocked_pid, w.usename::text AS blocked_user, \
    l.locktype || ': ' || l.mode AS \"lock\", \
    CASE WHEN l.relation IS NULL THEN NULL \
      WHEN l.database = (SELECT d.oid FROM pg_catalog.pg_database d \
        WHERE d.datname = pg_catalog.current_database()) \
      THEN l.relation::pg_catalog.regclass::text \
      ELSE 'oid ' || l.relation::text END AS object, \
    pg_catalog.date_trunc('second', pg_catalog.now() - w.query_start)::text \
      AS blocked_query_age, \
    w.query AS blocked_query, b.state AS blocking_state, \
    pg_catalog.date_trunc('second', pg_catalog.now() - b.xact_start)::text \
      AS blocking_transaction_age, \
    b.query AS blocking_query \
    FROM pg_catalog.pg_stat_activity w \
    CROSS JOIN LATERAL pg_catalog.unnest(pg_catalog.pg_blocking_pids(w.pid)) AS k(pid) \
    JOIN pg_catalog.pg_stat_activity b ON b.pid = k.pid \
    LEFT JOIN LATERAL (SELECT x.locktype, x.mode, x.relation, x.database \
      FROM pg_catalog.pg_locks x WHERE x.pid = w.pid AND NOT x.granted \
      FETCH FIRST 1 ROW ONLY) l ON true \
    WHERE b.pid <> pg_catalog.pg_backend_pid() \
    ORDER BY w.query_start NULLS LAST, w.pid, b.pid";

/// The columns that a table and a schema share, after the name.
const COLUMN_DATA: &str = "\
    pg_catalog.format_type(a.atttypid, a.atttypmod) AS type, \
    CASE WHEN a.attnotnull THEN 'no' ELSE 'yes' END AS nullable, \
    pg_catalog.pg_get_expr(d.adbin, d.adrelid) AS \"default\"";
const COLUMN_SOURCE: &str = "\
    FROM pg_catalog.pg_attribute a \
    JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
    LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum \
    WHERE a.attnum > 0 AND NOT a.attisdropped AND c.relkind IN ('r', 'p', 'f', 'v', 'm')";

fn columns(schema: &str, table: &str) -> String {
    format!(
        "SELECT a.attname::text AS name, {COLUMN_DATA}, \
         CASE WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_index i \
           WHERE i.indrelid = c.oid AND i.indisprimary AND a.attnum = ANY (i.indkey)) \
         THEN 'PK' END AS key \
         {COLUMN_SOURCE} AND n.nspname = {schema} AND c.relname = {table} \
         ORDER BY a.attnum"
    )
}

fn all_columns(schema: Option<&str>) -> String {
    let filter = match schema {
        Some(schema) => format!("n.nspname = {schema}"),
        None => "n.nspname !~ '^pg_' AND n.nspname <> 'information_schema'".to_owned(),
    };
    format!(
        "SELECT n.nspname::text AS schema, c.relname::text AS relation, \
         a.attname::text AS name, {COLUMN_DATA} \
         {COLUMN_SOURCE} AND {filter} \
         ORDER BY n.nspname, c.relname, a.attnum"
    )
}

fn indexes(schema: &str, table: &str) -> String {
    format!(
        "SELECT i.relname::text AS name, \
         (SELECT pg_catalog.string_agg(pg_catalog.pg_get_indexdef(x.indexrelid, k, true), ', ' \
            ORDER BY k) \
          FROM pg_catalog.generate_series(1, x.indnatts) AS k) AS columns, \
         CASE WHEN x.indisunique THEN 'yes' ELSE 'no' END AS \"unique\" \
         FROM pg_catalog.pg_index x \
         JOIN pg_catalog.pg_class i ON i.oid = x.indexrelid \
         JOIN pg_catalog.pg_class c ON c.oid = x.indrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE n.nspname = {schema} AND c.relname = {table} \
         ORDER BY i.relname"
    )
}

/// Lists the names of the attributes of a key, in the order of the key.
fn key_names(key: &str, owner: &str) -> String {
    format!(
        "(SELECT pg_catalog.string_agg(a.attname::text, ', ' ORDER BY u.place) \
         FROM pg_catalog.unnest({key}) WITH ORDINALITY AS u(attnum, place) \
         JOIN pg_catalog.pg_attribute a ON a.attrelid = {owner} AND a.attnum = u.attnum)"
    )
}

fn action(code: &str) -> String {
    format!(
        "CASE {code} WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE' \
         WHEN 'n' THEN 'SET NULL' WHEN 'd' THEN 'SET DEFAULT' END"
    )
}

fn foreign_keys(schema: &str, table: &str) -> String {
    let columns = key_names("k.conkey", "k.conrelid");
    let targets = key_names("k.confkey", "k.confrelid");
    let on_update = action("k.confupdtype");
    let on_delete = action("k.confdeltype");
    format!(
        "SELECT k.conname::text AS name, {columns} AS columns, \
         rn.nspname::text || '.' || r.relname::text || ' (' || {targets} || ')' AS \"references\", \
         {on_update} AS on_update, {on_delete} AS on_delete \
         FROM pg_catalog.pg_constraint k \
         JOIN pg_catalog.pg_class c ON c.oid = k.conrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_catalog.pg_class r ON r.oid = k.confrelid \
         JOIN pg_catalog.pg_namespace rn ON rn.oid = r.relnamespace \
         WHERE k.contype = 'f' AND n.nspname = {schema} AND c.relname = {table} \
         ORDER BY k.conname"
    )
}

/// The statistics of a table: sizes from the catalog, and counts and dates from the
/// statistics view. A view has no statistics row, so its counts read as `NULL`.
fn statistics(schema: &str, table: &str) -> String {
    format!(
        "SELECT v.name, v.value \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_catalog.pg_stat_user_tables s ON s.relid = c.oid \
         CROSS JOIN LATERAL (VALUES \
           (1, 'estimated rows', NULLIF(c.reltuples, -1)::bigint::text), \
           (2, 'estimated live rows', s.n_live_tup::text), \
           (3, 'estimated dead rows', s.n_dead_tup::text), \
           (4, 'total size', pg_catalog.pg_size_pretty( \
             pg_catalog.pg_total_relation_size(c.oid))), \
           (5, 'table size', pg_catalog.pg_size_pretty( \
             pg_catalog.pg_relation_size(c.oid))), \
           (6, 'indexes size', pg_catalog.pg_size_pretty( \
             pg_catalog.pg_indexes_size(c.oid))), \
           (7, 'sequential scans', s.seq_scan::text), \
           (8, 'index scans', s.idx_scan::text), \
           (9, 'last vacuum', s.last_vacuum::text), \
           (10, 'last analyze', s.last_analyze::text), \
           (11, 'last autovacuum', s.last_autovacuum::text), \
           (12, 'last autoanalyze', s.last_autoanalyze::text) \
         ) AS v(place, name, value) \
         WHERE n.nspname = {schema} AND c.relname = {table} \
         ORDER BY v.place"
    )
}
