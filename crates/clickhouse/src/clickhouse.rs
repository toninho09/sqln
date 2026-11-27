//! ClickHouse connector over the HTTP interface. Each session has its own `session_id`,
//! so `SET` and temporary tables stay between two statements of the session. ClickHouse
//! has no transactions, and the connector reads and runs only.

mod api;
mod tsv;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use self::api::{Api, Lines};
use crate::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use crate::connector::target::NetworkTarget;
use crate::connector::{
    Canceller, Capabilities, ConnectError, Connector, ErrorKind, ExecError, Namespace,
    ResetOutcome, Session, SessionContext, StatementEnd, TransactionCommand, TransactionState,
};
use crate::grid::{RowWriter, Value};
use crate::sink::{Flow, ResultSink};
use crate::sql::{Dialect, quote_literal};
use crate::text;
use crate::trace::{self, TraceContext};

/// The column of the answer of a statement with its own `FORMAT` clause.
const RAW_COLUMN: &str = "result";

const SCHEMAS_SQL: &str = concat!(
    "SELECT name FROM system.databases WHERE ",
    sqln_sql::clickhouse_user_databases!("name"),
    " ORDER BY name"
);

const TABLES_SQL: &str = concat!(
    "SELECT database, name, engine FROM system.tables WHERE ",
    sqln_sql::clickhouse_user_databases!("database"),
    " AND NOT is_temporary ORDER BY database, name"
);

pub struct ClickHouseConnector {
    target: NetworkTarget,
}

impl ClickHouseConnector {
    pub fn new(target: NetworkTarget) -> Self {
        Self { target }
    }
}

#[async_trait]
impl Connector for ClickHouseConnector {
    fn dialect(&self) -> Dialect {
        Dialect::ClickHouse
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            namespace: Namespace::Database,
            transactions: false,
            data_edit: false,
            ..Capabilities::default()
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let api = Api::new(&self.target)?;
        if let Err(error) = api.send("SELECT 1", &[]).await {
            return Err(if api::is_refusal(&error) {
                ConnectError::refused_password(text::CLICKHOUSE_REFUSED)
            } else {
                ConnectError::new(error.message).kind(error.kind)
            });
        }
        Ok(Box::new(ClickHouseSession {
            canceller: Arc::new(ClickHouseCanceller {
                api: api.clone(),
                running: Mutex::default(),
            }),
            api,
            session_id: new_id(),
            start_database: self.target.database.clone(),
            database: self.target.database.clone(),
            read_only: false,
            trace: None,
        }))
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Stops the statement in progress with its query id.
struct ClickHouseCanceller {
    api: Api,
    running: Mutex<Option<String>>,
}

impl ClickHouseCanceller {
    fn running(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[async_trait]
impl Canceller for ClickHouseCanceller {
    async fn cancel(&self) -> Result<(), String> {
        let id = self.running().clone();
        match id {
            Some(id) => self.api.kill(&id).await,
            None => Ok(()),
        }
    }
}

struct ClickHouseSession {
    api: Api,
    canceller: Arc<ClickHouseCanceller>,
    /// A new id after each cleanup gives a new server session without state.
    session_id: String,
    /// The database of the connection. A session without a schema returns to it.
    start_database: Option<String>,
    database: Option<String>,
    read_only: bool,
    trace: Option<TraceContext>,
}

impl ClickHouseSession {
    /// Sends a statement. The canceller keeps its query id until `end`.
    async fn execute(&self, sql: &str) -> Result<reqwest::Response, ExecError> {
        let query_id = new_id();
        let mut parameters = vec![
            ("session_id", self.session_id.clone()),
            ("query_id", query_id.clone()),
            ("default_format", api::FORMAT.to_owned()),
        ];
        if let Some(database) = &self.database {
            parameters.push(("database", database.clone()));
        }
        if self.read_only {
            // `2` refuses changes of data and keeps `SET` of settings.
            parameters.push(("readonly", "2".to_owned()));
        }
        *self.canceller.running() = Some(query_id);
        let parameters: Vec<(&str, &str)> = parameters
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        self.api.send(sql, &parameters).await
    }

    fn end(&self) {
        *self.canceller.running() = None;
    }

    async fn read(&self, sql: &str, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let response = self.execute(sql).await?;
        let raw = !api::uses_format(&response);
        let mut lines = Lines::new(response, sink.memory_limit());
        let Some(first) = lines.next().await? else {
            return Ok(false);
        };
        let mut row = RowWriter::default();
        if raw {
            sink.set_columns(vec![RAW_COLUMN.to_owned()]).await;
            let mut line = Some(first);
            while let Some(bytes) = line {
                row.push(match std::str::from_utf8(&bytes) {
                    Ok(text) => Value::Text(text),
                    Err(_) => Value::binary(&bytes),
                });
                if sink.push(row.finish()).await != Flow::Continue {
                    break;
                }
                line = lines.next().await?;
            }
            return Ok(true);
        }
        let names = tsv::text_fields(&first)
            .into_iter()
            .map(Option::unwrap_or_default)
            .collect();
        let types = lines
            .next()
            .await?
            .map(|line| tsv::text_fields(&line))
            .unwrap_or_default();
        let numbers: Vec<bool> = types
            .iter()
            .map(|kind| kind.as_deref().is_some_and(tsv::is_number))
            .collect();
        sink.set_typed_columns(names, types).await;
        while let Some(line) = lines.next().await? {
            tsv::write_row(&mut row, &line, &numbers);
            // The rest of the answer stays unread, and ClickHouse stops at the closed
            // connection.
            if sink.push(row.finish()).await != Flow::Continue {
                break;
            }
        }
        Ok(true)
    }

    /// Runs a catalog read and returns its cells as text.
    async fn query(&self, sql: &str) -> Result<Vec<Vec<Option<String>>>, ExecError> {
        let result = trace::observe(self.trace.clone(), sql, async {
            let response = self.execute(sql).await?;
            if !api::uses_format(&response) {
                return Err(ExecError::new(text::CLICKHOUSE_BAD_ANSWER));
            }
            let mut lines = Lines::new(response, usize::MAX);
            // The names and the types come before the rows.
            lines.next().await?;
            lines.next().await?;
            let mut rows = Vec::new();
            while let Some(line) = lines.next().await? {
                rows.push(tsv::text_fields(&line));
            }
            Ok(rows)
        })
        .await;
        self.end();
        result
    }
}

/// Returns the first cell of each row that has a value.
fn first_cells(rows: Vec<Vec<Option<String>>>) -> Vec<String> {
    rows.into_iter()
        .filter_map(|row| row.into_iter().next().flatten())
        .collect()
}

fn relation(name: String, engine: Option<&str>) -> Relation {
    Relation {
        name,
        // `View`, `MaterializedView`, `LiveView`, and `WindowView`.
        kind: if engine.is_some_and(|engine| engine.ends_with("View")) {
            RelationKind::View
        } else {
            RelationKind::Table
        },
        columns: Columns::NotLoaded,
        foreign_keys: Vec::new(),
        referencing: Vec::new(),
        notes: Vec::new(),
    }
}

#[async_trait]
impl Session for ClickHouseSession {
    fn set_trace_context(&mut self, trace: Option<TraceContext>) {
        self.trace = trace;
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        trace::canceller(self.canceller.clone(), &self.trace)
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        self.read_only = context.read_only;
        self.database = context
            .schema
            .clone()
            .or_else(|| self.start_database.clone());
        Ok(())
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        let read = trace::observe(self.trace.clone(), sql, self.read(sql, &mut sink)).await;
        self.end();
        let (rows, limit) = sink.finish().await;
        read.map(|has_columns| StatementEnd {
            rows,
            has_columns,
            changed: None,
            limit,
        })
    }

    async fn transaction_state(&mut self) -> TransactionState {
        TransactionState::None
    }

    async fn control(&mut self, _: TransactionCommand) -> Result<(), ExecError> {
        Err(ExecError::with_kind(
            ErrorKind::Unsupported,
            text::CLICKHOUSE_NO_TRANSACTIONS,
        ))
    }

    async fn reset(&mut self) -> ResetOutcome {
        self.session_id = new_id();
        self.database.clone_from(&self.start_database);
        self.read_only = false;
        ResetOutcome::Clean
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        let current = first_cells(self.query("SELECT currentDatabase()").await?)
            .into_iter()
            .next();
        let mut schemas: Vec<Schema> = first_cells(self.query(SCHEMAS_SQL).await?)
            .into_iter()
            .map(|name| Schema {
                name,
                ..Schema::default()
            })
            .collect();
        let positions: HashMap<String, usize> = schemas
            .iter()
            .enumerate()
            .map(|(position, schema)| (schema.name.clone(), position))
            .collect();
        for row in self.query(TABLES_SQL).await? {
            let [Some(database), Some(name), engine] = <[Option<String>; 3]>::try_from(row)
                .map_err(|_| ExecError::new(text::CLICKHOUSE_BAD_ANSWER))?
            else {
                continue;
            };
            if let Some(&position) = positions.get(&database) {
                schemas[position]
                    .relations
                    .push(relation(name, engine.as_deref()));
            }
        }
        Ok(Catalog {
            schemas,
            current_schema: current,
        })
    }

    async fn read_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<String>, ExecError> {
        let literal = |name: &str| quote_literal(name, Dialect::ClickHouse);
        let rows = self
            .query(&format!(
                "SELECT name FROM system.columns WHERE database = {} AND table = {} \
                 ORDER BY position",
                literal(schema),
                literal(relation)
            ))
            .await?;
        Ok(first_cells(rows))
    }
}
