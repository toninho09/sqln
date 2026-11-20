//! The requests of agent clients: dispatch, context, and the targets of a request.

use super::*;

impl App {
    pub(super) fn agent_request(&mut self, request: &Envelope) -> Reply {
        use Operation::*;
        let session = &request.session;
        if matches!(request.operation, RequestAccess {}) {
            if !self.agent.sessions.contains_key(session) {
                self.agent
                    .sessions
                    .retain(|_, session| !session.alive.is_cancelled());
                if self.agent.sessions.len() >= 64 {
                    return Err(Error::new(
                        "ui_busy",
                        "Too many assistant sessions are pending. Review them in SQLN.",
                    ));
                }
                // A remembered client keeps the choice of the user. Other clients get the
                // default access. Both stay at or below the maximum.
                let settings = self.agent.settings;
                let remembered = self.agent.remembered.get(&request.client).copied();
                let default = settings.default_access.access();
                let granted = remembered
                    .or(default)
                    .map(|level| level.min(settings.max_access));
                self.agent.sessions.insert(
                    session.clone(),
                    Session {
                        client: request.client.clone(),
                        state: if granted.is_some() {
                            "authorized"
                        } else {
                            "pending"
                        },
                        alive: request.alive.clone(),
                        requested: Instant::now(),
                        level: granted.unwrap_or(Level::Propose),
                    },
                );
                if let Some(level) = granted {
                    self.ensure_agent_tabs();
                    let client = self.agent.client(session);
                    let notice = if remembered.is_some() {
                        crate::ui::text::agent_remembered(&client, level.choice())
                    } else {
                        crate::ui::text::agent_default_access(&client, level.choice())
                    };
                    self.agent_notice(session, None, notice);
                    self.refresh_tree_consoles();
                } else {
                    // A new request rings once, so the user sees it in another window.
                    self.bell = true;
                }
            }
            return Ok(self.agent_access(session));
        }
        let Some(level) = self
            .agent
            .level(session)
            .filter(|_| !request.alive.is_cancelled())
        else {
            return Err(Error::new(
                "access_required",
                "Call request_access and ask the user to approve this session in SQLN.",
            ));
        };
        if !request.operation.is_inspection()
            && (self.modal_is_open() || self.exiting || self.config_blocked())
        {
            return Err(Error::new(
                "ui_busy",
                "SQLN has an open dialog or blocked configuration. Resolve it before starting this operation.",
            ));
        }
        match &request.operation {
            RequestAccess {} => unreachable!(),
            GetContext { offset, limit } => self.agent_context(session, level, *offset, *limit),
            ReadConsole {
                console_id,
                offset,
                revision,
                messages,
                message_offset,
            } => {
                let console = id(console_id)?;
                let mut page = self.agent_read_console(console, *offset, *revision)?;
                if *messages {
                    page["messages"] = self.agent_messages(console, *message_offset, 0)?;
                }
                Ok(page)
            }
            ReadResult {
                tab_id,
                result,
                offset,
                limit,
                generation,
                row,
                column,
            } => match (row, column) {
                (None, None) => {
                    self.agent_result(tab_id, *result, *offset, *limit, generation.as_deref())
                }
                (Some(row), Some(column)) => {
                    let generation = generation.as_deref().ok_or_else(|| {
                        Error::new(
                            "generation_required",
                            "Use the generation from the result page.",
                        )
                    })?;
                    self.agent_cell(tab_id, *result, *row, *column, generation, *offset)
                }
                _ => Err(Error::new(
                    "invalid_request",
                    "Send both row and column to read a cell, or neither to read rows.",
                )),
            },
            Query {
                connection_id,
                schema,
                sql,
                parameters,
                explain,
                rows,
            } => {
                let console = self.agent_console(session, id(connection_id)?, schema.clone())?;
                self.agent_query(session, console, sql, parameters, *explain, rows_of(*rows))
            }
            RunConsole {
                console_id,
                revision,
                range,
                explain,
                rows,
            } => self.agent_run_console(
                session,
                id(console_id)?,
                *revision,
                range.as_ref(),
                *explain,
                rows_of(*rows),
            ),
            EditConsole {
                console_id: Some(console_id),
                revision,
                edits,
                mode,
                ..
            } => {
                let revision = revision.ok_or_else(|| {
                    Error::new(
                        "invalid_request",
                        "Send the revision from read_console with the edits.",
                    )
                })?;
                if edits.is_empty() {
                    return Err(Error::new(
                        "invalid_request",
                        "Send at least one edit, or omit console_id to open a new console.",
                    ));
                }
                self.agent_edit(session, id(console_id)?, revision, edits, *mode)
            }
            EditConsole {
                console_id: None,
                connection_id,
                schema,
                sql,
                ..
            } => {
                let (Some(connection), Some(sql)) = (connection_id, sql) else {
                    return Err(Error::new(
                        "invalid_request",
                        "Send console_id to edit a console, or connection_id and sql to open one.",
                    ));
                };
                self.agent_place(session, id(connection)?, schema.clone(), sql)
            }
            Describe {
                connection_id,
                schema,
                object,
                er_diagram,
                refresh,
            } => {
                let connection = id(connection_id)?;
                let schema = || {
                    schema.as_deref().ok_or_else(|| {
                        Error::new(
                            "schema_required",
                            "Send the schema. Read the schema names with describe and no object.",
                        )
                    })
                };
                if *er_diagram {
                    let table = object.as_ref().map(|object| object.name.as_str());
                    self.agent_diagram(session, connection, schema()?, table)
                } else if let Some(object) = object {
                    self.agent_ddl(
                        session,
                        connection,
                        schema()?,
                        &object.name,
                        object.kind.as_str(),
                        object.identity.as_deref(),
                    )
                } else {
                    self.agent_catalog(session, connection, *refresh)
                }
            }
            Compare {
                kind,
                source,
                target,
                removals,
                rows,
            } => {
                let kind = match kind {
                    agent::CompareKind::Structure => crate::compare::Kind::Structure,
                    agent::CompareKind::Data => crate::compare::Kind::Data,
                };
                self.agent_compare(session, source, target, *removals, kind, rows_of(*rows))
            }
            Job {
                job_id,
                cancel,
                artifact_offset,
                rows,
            } => self.agent_job(session, job_id, *cancel, *artifact_offset, *rows),
        }
    }

    /// Reads the application state that an agent needs to choose a target.
    pub(super) fn agent_context(
        &self,
        session: &str,
        level: Level,
        offset: usize,
        limit: usize,
    ) -> Reply {
        let connections = self.agent_connections(0, CONTEXT_CONNECTIONS)?;
        let tabs = self.agent_tabs(offset, limit)?;
        let proposals: Vec<_> = self
            .agent
            .pending_proposal_ids()
            .into_iter()
            .filter(|id| self.agent.proposals[id].session == session)
            .collect();
        let mut reply = json!({
            "session_id": session,
            "access": level.protocol_name(),
            "database_access": if level == Level::Full { "read_write" } else { "read_only" },
            "interface_busy": self.modal_is_open(),
            "active_tab": tabs["active_tab"],
            "connections": connections["connections"],
            "connections_total": connections["total"],
            "tabs": tabs["tabs"],
            "next_offset": tabs["next_offset"],
            "tabs_total": tabs["total"],
            "pending_proposals": proposals,
        });
        // A Kafka connection does not accept standard SQL, so the agent gets its guide.
        if self
            .connections
            .iter()
            .take(CONTEXT_CONNECTIONS)
            .any(|connection| connection.dialect == crate::sql::Dialect::Kafka)
        {
            reply["syntax"] = json!({"Kafka": crate::connector::kafka::GUIDE});
        }
        Ok(reply)
    }

    pub(super) fn agent_access(&self, session: &str) -> Value {
        let item = self.agent.sessions.get(session);
        json!({
            "session_id": session,
            "status": item.map_or("not_requested", |session| session.state),
            "access": item.filter(|item| item.state == "authorized").map(|item| item.level.protocol_name()),
        })
    }

    pub(super) fn agent_target(&self, console: ConsoleId) -> Result<usize, Error> {
        let index = self.console_index(console).ok_or_else(missing)?;
        let console = &self.tabs.consoles[index];
        if console.hidden {
            return Err(missing());
        }
        if console.transaction != TransactionState::None {
            return Err(Error::new(
                "transaction_active",
                "The console has an open, failed, or unknown transaction. Use the agent console.",
            ));
        }
        if console.is_busy() || self.association_pending(console.id) {
            return Err(Error::new(
                "console_busy",
                "The console has pending work. Wait or use the agent console.",
            ));
        }
        if console.connection.is_none() {
            return Err(Error::new(
                "no_connection",
                "The user must attach a connection before this console can execute.",
            ));
        }
        Ok(index)
    }

    pub(super) fn agent_console(
        &mut self,
        session: &str,
        connection: ConnectionId,
        schema: Option<String>,
    ) -> Result<ConsoleId, Error> {
        let dialect = self.connection(connection).ok_or_else(missing)?.dialect;
        if let Some(index) = self.tabs.consoles.iter().position(|console| {
            self.agent
                .owned
                .get(&console.id)
                .is_some_and(|owner| owner == session)
                && console.connection == Some(connection)
        }) {
            if schema.is_some() && self.tabs.consoles[index].schema != schema {
                self.agent_target(self.tabs.consoles[index].id)?;
                self.tabs.consoles[index].schema = schema;
            }
            return Ok(self.tabs.consoles[index].id);
        }
        // The session tab that opened at the access decision takes its first connection.
        let index = match self.tabs.consoles.iter().position(|console| {
            console.connection.is_none()
                && self
                    .agent
                    .owned
                    .get(&console.id)
                    .is_some_and(|owner| owner == session)
        }) {
            Some(index) => index,
            None => {
                self.open_agent_tab(session);
                self.tabs.consoles.len() - 1
            }
        };
        self.bind_console(index, connection);
        let console = &mut self.tabs.consoles[index];
        if console.connection != Some(connection) {
            return Err(missing());
        }
        console.editor.set_dialect(dialect);
        console.schema = schema;
        Ok(console.id)
    }
}
