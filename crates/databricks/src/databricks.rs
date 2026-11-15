//! Databricks SQL connector. The API keeps no session state, so each request carries the
//! catalog and the schema of the session. A schema name of the catalog is always
//! `catalog.schema`, so the explorer shows the schemas of all catalogs of Unity Catalog.

mod api;
mod arrow;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use self::api::{Answer, Api, ExternalLink, State};
use crate::catalog::{Catalog, Columns, Relation, RelationKind, Schema};
use crate::connector::target::NetworkTarget;
use crate::connector::{
    Canceller, Capabilities, ConnectError, Connector, ExecError, Namespace, ResetOutcome, Session,
    SessionContext, StatementEnd, TransactionCommand, TransactionState,
};
use crate::sink::{Flow, ResultSink};
use crate::sql::{Dialect, changes_state, quote_literal};
use crate::text;
use crate::trace::{self, TraceContext};

/// The first pause between two status requests. Each pause doubles up to `POLL_MAX`.
const POLL_START: Duration = Duration::from_millis(100);
const POLL_MAX: Duration = Duration::from_secs(1);
/// The catalog reads leave out the views of the catalog itself.
const INFORMATION_SCHEMA: &str = "information_schema";
/// The `catalog.schema` name of a row of `schemata`.
const SCHEMA_NAME: &str = "catalog_name || '.' || schema_name";
/// The `catalog.schema` name of a row of `tables`.
const TABLE_SCHEMA: &str = "table_catalog || '.' || table_schema";

pub struct DatabricksConnector {
    target: NetworkTarget,
}

impl DatabricksConnector {
    pub fn new(target: NetworkTarget) -> Self {
        Self { target }
    }
}

#[async_trait]
impl Connector for DatabricksConnector {
    fn dialect(&self) -> Dialect {
        Dialect::Databricks
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            namespace: Namespace::Schema,
            transactions: false,
            data_edit: false,
            ..Capabilities::default()
        }
    }

    async fn connect(&self) -> Result<Box<dyn Session>, ConnectError> {
        let api = Api::new(&self.target)?;
        api.check().await?;
        Ok(Box::new(DatabricksSession {
            canceller: Arc::new(DatabricksCanceller {
                api: api.clone(),
                running: Mutex::default(),
            }),
            api,
            start_catalog: self.target.database.clone(),
            catalog: self.target.database.clone(),
            schema: None,
            read_only: false,
            trace: None,
        }))
    }
}

/// The statement in progress, or a cancel that arrived before its id.
#[derive(Default)]
struct Running {
    id: Option<String>,
    cancel: bool,
}

struct DatabricksCanceller {
    api: Api,
    running: Mutex<Running>,
}

impl DatabricksCanceller {
    fn running(&self) -> std::sync::MutexGuard<'_, Running> {
        self.running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Starts a statement: a cancel of an earlier statement must not reach it.
    fn start(&self) {
        *self.running() = Running::default();
    }

    /// Keeps the id of the statement. Returns true when a cancel arrived before it.
    fn started(&self, id: &str) -> bool {
        let mut running = self.running();
        running.id = Some(id.to_owned());
        running.cancel
    }

    fn end(&self) {
        *self.running() = Running::default();
    }
}

#[async_trait]
impl Canceller for DatabricksCanceller {
    async fn cancel(&self) -> Result<(), String> {
        let id = {
            let mut running = self.running();
            running.cancel = true;
            running.id.clone()
        };
        match id {
            Some(id) => self.api.cancel(&id).await,
            // The statement gets its id later, and `run` sends the cancel then.
            None => Ok(()),
        }
    }
}

struct DatabricksSession {
    api: Api,
    canceller: Arc<DatabricksCanceller>,
    /// The catalog of the connection. A session without a schema returns to it.
    start_catalog: Option<String>,
    catalog: Option<String>,
    schema: Option<String>,
    read_only: bool,
    trace: Option<TraceContext>,
}

/// A statement that ended with success.
struct Done {
    id: String,
    columns: Vec<String>,
    chunks: usize,
    links: Vec<ExternalLink>,
}

impl DatabricksSession {
    /// Sends a statement and waits for its end. A cancel stops it in the warehouse.
    async fn execute(&self, sql: &str) -> Result<Done, ExecError> {
        self.canceller.start();
        let result = self.execute_untracked(sql).await;
        self.canceller.end();
        result
    }

    async fn execute_untracked(&self, sql: &str) -> Result<Done, ExecError> {
        let mut answer = self
            .api
            .submit(sql, self.catalog.as_deref(), self.schema.as_deref())
            .await?;
        if self.canceller.started(&answer.statement_id) && !answer.status.state.is_final() {
            self.api
                .cancel(&answer.statement_id)
                .await
                .map_err(ExecError::new)?;
        }
        let mut pause = POLL_START;
        while !answer.status.state.is_final() {
            tokio::time::sleep(pause).await;
            pause = (pause * 2).min(POLL_MAX);
            answer = self.api.status(&answer.statement_id).await?;
        }
        done(answer)
    }

    /// Downloads one result part. An expired link gets a new link one time.
    async fn part(&self, done: &Done, index: usize, limit: usize) -> Result<Vec<u8>, ExecError> {
        let known = done
            .links
            .iter()
            .find(|link| link.chunk_index == index)
            .cloned();
        if let Some(link) = known
            && let Some(bytes) = self.api.download(&link, limit).await?
        {
            return Ok(bytes);
        }
        let fresh = self.api.chunk(&done.id, index).await?;
        let link = fresh
            .external_links
            .into_iter()
            .find(|link| link.chunk_index == index)
            .ok_or_else(|| ExecError::new(text::DATABRICKS_BAD_ANSWER))?;
        self.api
            .download(&link, limit)
            .await?
            .ok_or_else(|| ExecError::new(text::DATABRICKS_CLOSED))
    }

    /// Runs a catalog read and returns its cells as text.
    async fn query(&self, sql: &str) -> Result<Vec<Vec<Option<String>>>, ExecError> {
        trace::observe(self.trace.clone(), sql, async {
            let done = self.execute(sql).await?;
            let mut rows = Vec::new();
            for index in 0..done.chunks {
                for batch in arrow::batches(self.part(&done, index, usize::MAX).await?)? {
                    rows.extend(arrow::text_rows(&batch)?);
                }
            }
            Ok(rows)
        })
        .await
    }

    async fn read(&self, sql: &str, sink: &mut ResultSink) -> Result<bool, ExecError> {
        let done = self.execute(sql).await?;
        if done.columns.is_empty() {
            return Ok(false);
        }
        sink.set_columns(done.columns.clone()).await;
        let limit = sink.memory_limit();
        // One part at a time: the rows of a batch go to the grid before the next batch.
        for index in 0..done.chunks {
            for batch in arrow::batches(self.part(&done, index, limit).await?)? {
                for row in arrow::rows(&batch)? {
                    if sink.push(row).await != Flow::Continue {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(true)
    }

    /// The filter of a list of `catalog.schema` names on `column`, with a leading space.
    fn schema_filter(names: &[String], column: &str) -> String {
        if names.is_empty() {
            return String::new();
        }
        let names = names
            .iter()
            .map(|name| quote_literal(name, Dialect::Databricks))
            .collect::<Vec<_>>()
            .join(", ");
        format!(" AND {column} IN ({names})")
    }

    /// Reads the `catalog.schema` names that pass `filter`.
    async fn schema_names(&self, filter: &str) -> Result<Vec<String>, ExecError> {
        let rows = self
            .query(&format!(
                "SELECT {SCHEMA_NAME} FROM system.information_schema.schemata \
                 WHERE schema_name <> '{INFORMATION_SCHEMA}'{filter} ORDER BY 1"
            ))
            .await?;
        Ok(first_cells(rows))
    }

    async fn current_schema(&self) -> Result<Option<String>, ExecError> {
        let rows = self
            .query("SELECT current_catalog() || '.' || current_schema()")
            .await?;
        Ok(first_cells(rows).into_iter().next())
    }
}

fn done(answer: Answer) -> Result<Done, ExecError> {
    match answer.status.state {
        State::Succeeded => {}
        State::Failed => {
            return Err(answer
                .status
                .error
                .map(api::ApiError::into_exec_error)
                .unwrap_or_else(|| ExecError::new(text::DATABRICKS_BAD_ANSWER)));
        }
        State::Canceled => return Err(ExecError::new(text::DATABRICKS_CANCELED)),
        State::Closed => return Err(ExecError::new(text::DATABRICKS_CLOSED)),
        State::Pending | State::Running => unreachable!("only a final state reaches here"),
    }
    let manifest = answer.manifest.unwrap_or_default();
    Ok(Done {
        id: answer.statement_id,
        columns: manifest
            .schema
            .columns
            .into_iter()
            .map(|column| column.name)
            .collect(),
        chunks: manifest.total_chunk_count,
        links: answer.result.unwrap_or_default().external_links,
    })
}

/// Returns the first cell of each row that has a value.
fn first_cells(rows: Vec<Vec<Option<String>>>) -> Vec<String> {
    rows.into_iter()
        .filter_map(|row| row.into_iter().next().flatten())
        .collect()
}

/// Splits a `catalog.schema` name. A name without a dot is a schema of the session catalog.
fn split_schema(name: &str) -> (Option<&str>, &str) {
    match name.split_once('.') {
        Some((catalog, schema)) => (Some(catalog), schema),
        None => (None, name),
    }
}

fn relation(name: String, kind: Option<&str>) -> Relation {
    Relation {
        name,
        kind: match kind {
            Some("VIEW" | "MATERIALIZED_VIEW") => RelationKind::View,
            _ => RelationKind::Table,
        },
        columns: Columns::NotLoaded,
        foreign_keys: Vec::new(),
        referencing: Vec::new(),
        notes: Vec::new(),
    }
}

#[async_trait]
impl Session for DatabricksSession {
    fn set_trace_context(&mut self, trace: Option<TraceContext>) {
        self.trace = trace;
    }

    fn canceller(&self) -> Arc<dyn Canceller> {
        trace::canceller(self.canceller.clone(), &self.trace)
    }

    async fn apply_context(&mut self, context: &SessionContext) -> Result<(), ExecError> {
        self.read_only = context.read_only;
        match context.schema.as_deref().map(split_schema) {
            Some((catalog, schema)) => {
                self.catalog = catalog
                    .map(str::to_owned)
                    .or_else(|| self.start_catalog.clone());
                self.schema = Some(schema.to_owned());
            }
            None => {
                self.catalog.clone_from(&self.start_catalog);
                self.schema = None;
            }
        }
        Ok(())
    }

    async fn run(&mut self, sql: &str, mut sink: ResultSink) -> Result<StatementEnd, ExecError> {
        if self.read_only && changes_state(sql, Dialect::Databricks) {
            return Err(ExecError::new(text::DATABRICKS_READ_ONLY));
        }
        let read = trace::observe(self.trace.clone(), sql, self.read(sql, &mut sink)).await;
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
        Err(ExecError::new(text::DATABRICKS_NO_TRANSACTIONS))
    }

    async fn reset(&mut self) -> ResetOutcome {
        self.catalog.clone_from(&self.start_catalog);
        self.schema = None;
        self.read_only = false;
        ResetOutcome::Clean
    }

    async fn read_catalog(&mut self) -> Result<Catalog, ExecError> {
        self.read_catalog_of(&[]).await
    }

    async fn read_catalog_of(&mut self, schemas: &[String]) -> Result<Catalog, ExecError> {
        let current = self.current_schema().await?;
        let mut names = schemas.to_vec();
        if !names.is_empty()
            && let Some(current) = &current
        {
            names.push(current.clone());
        }
        let mut schemas: Vec<Schema> = self
            .schema_names(&Self::schema_filter(&names, SCHEMA_NAME))
            .await?
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
        let tables = self
            .query(&format!(
                "SELECT {TABLE_SCHEMA}, table_name, table_type \
                 FROM system.information_schema.tables \
                 WHERE table_schema <> '{INFORMATION_SCHEMA}'{} ORDER BY 1, 2",
                Self::schema_filter(&names, TABLE_SCHEMA)
            ))
            .await?;
        for row in tables {
            let [Some(schema), Some(name), kind] = <[Option<String>; 3]>::try_from(row)
                .map_err(|_| ExecError::new(text::DATABRICKS_BAD_ANSWER))?
            else {
                continue;
            };
            if let Some(&position) = positions.get(&schema) {
                schemas[position]
                    .relations
                    .push(relation(name, kind.as_deref()));
            }
        }
        Ok(Catalog {
            schemas,
            current_schema: current,
        })
    }

    async fn read_schema_names(&mut self) -> Result<Vec<String>, ExecError> {
        self.schema_names("").await
    }

    async fn read_columns(
        &mut self,
        schema: &str,
        relation: &str,
    ) -> Result<Vec<String>, ExecError> {
        let (catalog, schema) = split_schema(schema);
        let literal = |name: &str| quote_literal(name, Dialect::Databricks);
        let catalog = catalog.map_or_else(|| "current_catalog()".to_owned(), &literal);
        let rows = self
            .query(&format!(
                "SELECT column_name FROM system.information_schema.columns \
                 WHERE table_catalog = {catalog} AND table_schema = {} AND table_name = {} \
                 ORDER BY ordinal_position",
                literal(schema),
                literal(relation)
            ))
            .await?;
        Ok(first_cells(rows))
    }
}
