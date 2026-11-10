//! Short, checked transactions for pending table updates.

use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedReceiver;

use super::{
    Command, ConsoleTask, Delivery, ExecEvent, ExecutionEnd, ExecutionId, StatementKey, Waited,
    context,
};
use crate::compare::sql::table_columns_sql;
use crate::connector::TransactionState;
use crate::data_edit::{Change, Failure, Job, Metadata, Outcome};
use crate::exec::sink::Captured;
use crate::pool::Lease;
use crate::sql::{Dialect, literal::text_literal};
use crate::ui::text;

fn failure(message: impl Into<String>) -> Failure {
    Failure {
        message: message.into(),
        uncertain: false,
    }
}

impl ConsoleTask {
    async fn edit_statement(
        &mut self,
        lease: &mut Option<Lease>,
        key: &mut StatementKey,
        sql: &str,
        cap: usize,
        commands: &mut UnboundedReceiver<Command>,
    ) -> Result<Captured, Failure> {
        if self.stop_is_pending(commands).await {
            return Err(failure(text::EDIT_STOPPED));
        }
        let capture = Arc::new(Mutex::new(Captured::default()));
        let active = lease.as_mut().ok_or_else(|| failure(text::EDIT_STOPPED))?;
        let delivery = Delivery {
            cap,
            export: None,
            stream: false,
            timed: true,
            capture: Some(capture.clone()),
            parameters: Vec::new(),
            live: false,
        };
        let (waited, reason) = self
            .run_statement(*key, active, sql, delivery, commands)
            .await;
        key.index += 1;
        if waited.closes_session() {
            *lease = None;
        }
        match waited {
            Waited::Finished {
                result: Ok(end),
                reusable: true,
            } if reason.is_none() => {
                let mut captured = std::mem::take(
                    &mut *capture
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                );
                captured.changed = end.changed;
                if captured.incomplete {
                    Err(failure(
                        if captured.limit == crate::exec::ReadLimit::RowCap {
                            text::EDIT_CONFLICT
                        } else {
                            text::EDIT_BUDGET
                        },
                    ))
                } else {
                    Ok(captured)
                }
            }
            Waited::Finished {
                result: Err(error), ..
            } if reason.is_none() => Err(failure(error.message)),
            _ => Err(failure(text::EDIT_STOPPED)),
        }
    }

    async fn edit_metadata(
        &mut self,
        lease: &mut Option<Lease>,
        key: &mut StatementKey,
        relation: crate::catalog::RelationRef,
        commands: &mut UnboundedReceiver<Command>,
    ) -> Result<Metadata, Failure> {
        let dialect = self.binding.pool.dialect();
        if dialect == Dialect::MySql {
            let sql = format!(
                "SELECT ENGINE FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} AND TABLE_TYPE = 'BASE TABLE'",
                text_literal(&relation.schema, dialect),
                text_literal(&relation.relation, dialect)
            );
            let result = self.edit_statement(lease, key, &sql, 2, commands).await?;
            if result.rows.len() != 1
                || !result.rows[0]
                    .row
                    .text(0)
                    .is_some_and(|engine| engine.eq_ignore_ascii_case("innodb"))
            {
                return Err(failure(text::EDIT_ENGINE));
            }
        }
        let sql = table_columns_sql(dialect, &relation.schema, &relation.relation)
            .ok_or_else(|| failure(text::EDIT_UNAVAILABLE))?;
        let captured = self
            .edit_statement(lease, key, &sql, 4096, commands)
            .await?;
        let rows = captured.rows.iter().map(|row| row.row.clone()).collect();
        Metadata::from_rows(relation, dialect, rows).map_err(failure)
    }

    pub(super) async fn run_edit(
        &mut self,
        execution: ExecutionId,
        job: Job,
        commands: &mut UnboundedReceiver<Command>,
    ) {
        let console = self.console;
        self.emit(ExecEvent::Queued { console, execution }).await;
        let job = match job {
            Job::Review { metadata, changes } => {
                self.emit(ExecEvent::Started { console, execution }).await;
                let result = tokio::task::spawn_blocking(move || {
                    crate::data_edit::review(&metadata, &changes)
                })
                .await;
                let result = match result {
                    Ok(Ok(text)) => Ok(Outcome::Review(text)),
                    Ok(Err(error)) => Err(failure(error)),
                    Err(_) => Err(failure(text::EDIT_STOPPED)),
                };
                let end = if result.is_ok() {
                    ExecutionEnd::Completed
                } else {
                    ExecutionEnd::Failed
                };
                self.emit(ExecEvent::EditFinished {
                    console,
                    execution,
                    result,
                })
                .await;
                self.finish(execution, end, None).await;
                return;
            }
            job => job,
        };
        if self.held.is_some() {
            self.emit(ExecEvent::EditFinished {
                console,
                execution,
                result: Err(failure(text::EDIT_MANUAL_TRANSACTION)),
            })
            .await;
            self.finish(execution, ExecutionEnd::Refused, None).await;
            return;
        }
        let Some(mut active) = self.acquire(execution, commands).await else {
            return;
        };
        self.emit(ExecEvent::Started { console, execution }).await;
        self.transaction = TransactionState::None;
        let applying = matches!(&job, Job::Apply { .. });
        let mut key = StatementKey {
            console,
            execution,
            index: 0,
        };
        let result = if self.settings.read_only
            || !self.binding.pool.capabilities().data_edit
            || !self.binding.pool.capabilities().transactions
        {
            Err(failure(text::EDIT_UNAVAILABLE))
        } else if let Err(error) = active
            .session()
            .apply_context(&context(&self.settings, None, false))
            .await
        {
            Err(failure(error.message))
        } else {
            let mut lease = Some(active);
            let result = self.edit_job(&mut lease, &mut key, job, commands).await;
            if applying && result.is_err() && lease.is_some() {
                // A failed cleanup drops the session. No pending transaction enters the pool.
                if self
                    .edit_statement(&mut lease, &mut key, "ROLLBACK", 1, commands)
                    .await
                    .is_err()
                {
                    lease = None;
                }
            }
            if let Some(mut active) = lease
                && active.session().transaction_state().await == TransactionState::None
            {
                active.release().await;
            }
            let end = if result.is_ok() {
                ExecutionEnd::Completed
            } else {
                ExecutionEnd::Failed
            };
            self.emit(ExecEvent::EditFinished {
                console,
                execution,
                result,
            })
            .await;
            self.finish(execution, end, None).await;
            return;
        };
        self.emit(ExecEvent::EditFinished {
            console,
            execution,
            result,
        })
        .await;
        self.finish(execution, ExecutionEnd::Failed, None).await;
    }

    async fn edit_job(
        &mut self,
        lease: &mut Option<Lease>,
        key: &mut StatementKey,
        job: Job,
        commands: &mut UnboundedReceiver<Command>,
    ) -> Result<Outcome, Failure> {
        match job {
            Job::Review { .. } => unreachable!("review runs without a database session"),
            Job::Inspect { relation, dialect } => {
                if dialect != self.binding.pool.dialect() {
                    return Err(failure(text::EDIT_UNAVAILABLE));
                }
                self.edit_metadata(lease, key, relation, commands)
                    .await
                    .map(Outcome::Metadata)
            }
            Job::Apply { metadata, changes } => {
                if metadata.dialect != self.binding.pool.dialect() || changes.is_empty() {
                    return Err(failure(text::EDIT_NO_CHANGES));
                }
                let begin = if metadata.dialect == Dialect::Sqlite {
                    "BEGIN IMMEDIATE"
                } else {
                    "BEGIN"
                };
                self.edit_statement(lease, key, begin, 1, commands).await?;
                let columns: Vec<_> = metadata
                    .table
                    .columns
                    .iter()
                    .map(|col| col.name.clone())
                    .collect();
                // Hold the row locks until commit. Validate all rows before any write.
                for change in &changes {
                    if let Some(original) = change.original() {
                        let sql = metadata.lock_sql(&original.row).map_err(failure)?;
                        let captured = self.edit_statement(lease, key, &sql, 2, commands).await?;
                        if captured.columns != columns
                            || captured.rows.len() != 1
                            || captured.rows[0].row != original.row
                        {
                            return Err(failure(text::EDIT_CONFLICT));
                        }
                    }
                }
                let current = self
                    .edit_metadata(lease, key, metadata.relation.clone(), commands)
                    .await?;
                if current != metadata {
                    return Err(failure(text::EDIT_SCHEMA_CHANGED));
                }
                for change in &changes {
                    match change {
                        Change::Update(patch) => {
                            let sql = metadata.update_sql(patch).map_err(failure)?;
                            // Earlier triggers can change another pending row on this session.
                            let locked = metadata.lock_sql(&patch.original.row).map_err(failure)?;
                            let captured = self
                                .edit_statement(lease, key, &locked, 2, commands)
                                .await?;
                            if captured.rows.len() != 1
                                || captured.rows[0].row != patch.original.row
                            {
                                return Err(failure(text::EDIT_CONFLICT));
                            }
                            let result = self.edit_statement(lease, key, &sql, 1, commands).await?;
                            // The lock proves identity even when MySQL reports no changed value.
                            if !(matches!(result.changed, Some(1))
                                || metadata.dialect == Dialect::MySql && result.changed == Some(0))
                            {
                                return Err(failure(text::EDIT_CONFLICT));
                            }
                        }
                        Change::Delete { original } => {
                            let locked = metadata.lock_sql(&original.row).map_err(failure)?;
                            let captured = self
                                .edit_statement(lease, key, &locked, 2, commands)
                                .await?;
                            if captured.rows.len() != 1 || captured.rows[0].row != original.row {
                                return Err(failure(text::EDIT_CONFLICT));
                            }
                            let sql = metadata.delete_sql(&original.row).map_err(failure)?;
                            let result = self.edit_statement(lease, key, &sql, 1, commands).await?;
                            if result.changed != Some(1) {
                                return Err(failure(text::EDIT_CONFLICT));
                            }
                        }
                        Change::Insert { current, columns } => {
                            let sql = metadata
                                .insert_sql(&current.row, columns)
                                .map_err(failure)?;
                            let result = self.edit_statement(lease, key, &sql, 1, commands).await?;
                            if result.changed != Some(1) {
                                return Err(failure(text::EDIT_CONFLICT));
                            }
                        }
                    }
                }
                if let Err(mut error) = self.edit_statement(lease, key, "COMMIT", 1, commands).await
                {
                    error.uncertain = true;
                    error.message = format!("{} {}", text::EDIT_COMMIT_UNKNOWN, error.message);
                    return Err(error);
                }
                Ok(Outcome::Applied)
            }
        }
    }
}
