//! Native parameter binding with server text results, including extension types.

use std::error::Error;

use bytes::BytesMut;
use tokio_postgres::types::{Format, FromSql, IsNull, ToSql, to_sql_checked};

use super::*;
use crate::sql::parameters::{Kind, Parameter};

impl ToSql for Parameter {
    fn to_sql(
        &self,
        _ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        if self.kind == Kind::Null {
            return Ok(IsNull::Yes);
        }
        out.extend_from_slice(self.value.as_bytes());
        Ok(IsNull::No)
    }
    fn accepts(_ty: &Type) -> bool {
        true
    }
    fn encode_format(&self, _ty: &Type) -> Format {
        Format::Text
    }
    to_sql_checked!();
}

/// Only marked Bind messages use this decoder. The transport requests text results.
struct ServerText<'a>(&'a str);
impl<'a> FromSql<'a> for ServerText<'a> {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Ok(Self(std::str::from_utf8(raw)?))
    }
    fn accepts(_ty: &Type) -> bool {
        true
    }
}

impl PostgresSession {
    pub(super) async fn run_bound(
        &mut self,
        sql: &str,
        parameters: &[Parameter],
        mut sink: ResultSink,
    ) -> Result<StatementEnd, ExecError> {
        self.canceller.requested.store(false, Ordering::SeqCst);
        self.take_notices();
        let command = crate::sql::parameters::command(sql, parameters);
        let result = crate::command_log::observe(
            self.trace.clone(),
            command,
            self.read_bound(sql, parameters, &mut sink),
        )
        .await;
        let notices = self.take_notices();
        sink.notices(notices.texts, notices.more).await;
        let (rows, limit) = sink.finish().await;
        result.map(|outcome| StatementEnd {
            rows,
            has_columns: outcome.has_columns,
            changed: outcome.changed,
            limit,
        })
    }

    async fn read_bound(
        &self,
        sql: &str,
        parameters: &[Parameter],
        sink: &mut ResultSink,
    ) -> Result<Outcome, ExecError> {
        let marked = format!("{}{sql}", text_io::MARKER);
        let map_error = |error: tokio_postgres::Error| {
            let mut error = exec_error(&error, &marked);
            error.offset = error
                .offset
                .map(|offset| offset.saturating_sub(text_io::MARKER.len()));
            error
        };
        let types: Vec<_> = parameters
            .iter()
            .map(|parameter| match parameter.kind {
                // An unknown text parameter takes the type that the query infers,
                // so a uuid, date, or jsonb column accepts the value without a cast.
                Kind::Text | Kind::Null => Type::UNKNOWN,
                // A number stays numeric, because inference in arithmetic can pick an
                // integer type and refuse a decimal text.
                Kind::Number => Type::NUMERIC,
                Kind::Boolean => Type::BOOL,
            })
            .collect();
        let statement = self
            .client
            .prepare_typed(&marked, &types)
            .await
            .map_err(map_error)?;
        if self.canceller.requested.load(Ordering::SeqCst) {
            return Err(ExecError::new(text::STOPPED_BEFORE_SEND));
        }
        let has_columns = !statement.columns().is_empty();
        if has_columns {
            sink.set_columns(
                statement
                    .columns()
                    .iter()
                    .map(|column| column.name().to_owned())
                    .collect(),
            )
            .await;
        }
        let stream = self
            .client
            .query_raw(&statement, parameters.iter())
            .await
            .map_err(map_error)?;
        let mut stream = pin!(stream);
        let mut storing = true;
        let mut stopped_at_cap = false;
        let mut writer = RowWriter::default();
        loop {
            let row = match stream.as_mut().try_next().await {
                Ok(Some(row)) => row,
                Ok(None) => break,
                Err(error) if stopped_at_cap && error.code() == Some(&SqlState::QUERY_CANCELED) => {
                    break;
                }
                Err(error) => return Err(map_error(error)),
            };
            if !storing {
                continue;
            }
            for (index, column) in row.columns().iter().enumerate() {
                let value: Option<ServerText<'_>> = row.try_get(index).map_err(map_error)?;
                match (value, column_kind(column.type_())) {
                    (None, _) => writer.push(Value::Null),
                    (Some(ServerText(text)), ColumnKind::Number) => {
                        writer.push(Value::Number(text))
                    }
                    (Some(ServerText(text)), ColumnKind::Binary) => write_bytea(&mut writer, text),
                    (Some(ServerText(text)), _) => writer.push(Value::Text(text)),
                }
            }
            let flow = sink.push(writer.finish()).await;
            storing = flow == Flow::Continue;
            if flow == Flow::Stop
                && self.transaction == TransactionState::None
                && is_plain_read(sql, Dialect::Postgres)
            {
                stopped_at_cap = self.canceller().cancel().await.is_ok();
            }
        }
        Ok(Outcome {
            has_columns,
            changed: (!has_columns && counts_rows(sql, Dialect::Postgres))
                .then(|| stream.rows_affected())
                .flatten(),
        })
    }
}
