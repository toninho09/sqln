//! Agent reads: definitions, fields, catalogs, DDL, diagrams, and comparisons.

use super::*;

impl App {
    /// The DDL text is small, as in the user viewer, so the interface thread writes it.
    /// The rows leave the grids for the call and then go back, so nothing is copied.
    pub(super) fn agent_definition(
        &mut self,
        console: ConsoleId,
        dialect: Dialect,
        target: &crate::definition::Target,
        name: &str,
    ) -> Result<Text, Error> {
        let index = self.console_index(console).ok_or_else(missing)?;
        let grids = &mut self.tabs.consoles[index].results;
        let rows: Vec<_> = grids
            .iter_mut()
            .map(|grid| std::mem::take(&mut grid.table.rows))
            .collect();
        let text = crate::definition::definition_text(dialect, target, name, &rows);
        for (grid, rows) in grids.iter_mut().zip(rows) {
            grid.table.rows = rows;
        }
        text.map_err(|message| Error::new("definition_failed", message))
            .and_then(|text| Text::new(text, &self.budget))
    }

    /// Reads the field paths of a Kafka topic from its Schema Registry schemas.
    pub(super) fn agent_fields(
        &mut self,
        session: &str,
        connection: ConnectionId,
        topic: &str,
    ) -> Reply {
        let console = self.agent_console(session, connection, None)?;
        self.agent_target(console)?;
        let job = self.new_agent_job(
            session,
            vec![console],
            vec![connection],
            None,
            Output::Fields(topic.to_owned()),
        )?;
        self.agent_job_reply(session, &job)
    }

    pub(in crate::app) fn agent_catalog(
        &mut self,
        session: &str,
        connection: ConnectionId,
        refresh: bool,
    ) -> Reply {
        let item = self.connection(connection).ok_or_else(missing)?;
        if !refresh && let Some(catalog) = &item.catalog {
            let value = catalog_value(catalog);
            let text = serde_json::to_string(&value)
                .map_err(|_| Error::new("encoding_failed", "Cannot encode the cached catalog."))?;
            if text.len() <= agent::PAGE_BYTES / 3 {
                return Ok(value);
            }
            let artifact = Text::new(text, &self.budget)?;
            let job = self.new_agent_job(
                session,
                Vec::new(),
                vec![connection],
                None,
                Output::Catalog(connection, self.counters.catalog_reads),
            )?;
            self.complete_agent_job(&job, Some(artifact));
            return self.agent_job_reply(session, &job);
        }
        let console = self.agent_console(session, connection, None)?;
        self.agent_target(console)?;
        self.counters.catalog_reads += 1;
        let read = self.counters.catalog_reads;
        let job = self.new_agent_job(
            session,
            vec![console],
            vec![connection],
            None,
            Output::Catalog(connection, read),
        )?;
        if let Some(item) = self.connection_mut(connection) {
            item.catalog_read = read;
        }
        self.agent_job_reply(session, &job)
    }

    pub(in crate::app) fn on_agent_metadata(
        &mut self,
        id: &str,
        result: Result<AgentMetadata, String>,
    ) {
        let Some(job) = self.agent.jobs.get(id) else {
            return;
        };
        if finished(job.state) || job.cancel.is_cancelled() {
            return;
        }
        if job.connections.iter().any(|(id, generation)| {
            self.connection(*id)
                .is_none_or(|connection| connection.generation != *generation)
        }) {
            self.fail_agent_job(id, "The connection changed during the metadata read.");
            return;
        }
        let text = match result {
            Ok(AgentMetadata::Catalog(catalog)) => {
                let Output::Catalog(connection, read) = job.output else {
                    self.fail_agent_job(id, "The metadata response has the wrong type.");
                    return;
                };
                let value = catalog_value(&catalog);
                self.on_catalog_loaded(connection, read, Ok(catalog));
                serde_json::to_string(&value).map_err(|_| "Cannot encode the catalog.".to_owned())
            }
            Ok(AgentMetadata::Fields(paths, notes)) => {
                let Output::Fields(topic) = &job.output else {
                    self.fail_agent_job(id, "The metadata response has the wrong type.");
                    return;
                };
                serde_json::to_string(&fields_value(topic, &paths, &notes))
                    .map_err(|_| "Cannot encode the fields.".to_owned())
            }
            Err(message) => Err(message),
        };
        match text.and_then(|text| Text::new(text, &self.budget).map_err(|error| error.message)) {
            Ok(text) => {
                self.complete_agent_job(id, Some(text));
            }
            Err(message) => self.fail_agent_job(id, &message),
        }
    }

    pub(in crate::app) fn agent_ddl(
        &mut self,
        session: &str,
        connection: ConnectionId,
        schema: &str,
        name: &str,
        kind: &str,
        identity: Option<&str>,
    ) -> Reply {
        let item = self.connection(connection).ok_or_else(missing)?;
        if item.dialect == Dialect::Kafka {
            if schema != crate::connector::kafka::TOPICS || kind != "table" {
                return Err(Error::new("unsupported", text::AGENT_KAFKA_DESCRIBE));
            }
            return self.agent_fields(session, connection, name);
        }
        let target = crate::definition::Target {
            schema: schema.into(),
            name: name.into(),
            detail: identity.unwrap_or("").into(),
            kind: match kind {
                "table" => crate::definition::Kind::Table,
                "view" => crate::definition::Kind::View,
                kind => {
                    crate::definition::Kind::Object(ObjectKind::parse(kind).ok_or_else(supported)?)
                }
            },
        };
        let dialect = item.dialect;
        let label = item.plain_name().to_owned();
        let statements =
            crate::definition::definition_sql(dialect, &target).ok_or_else(supported)?;
        let console = self.agent_console(session, connection, Some(schema.into()))?;
        let count = statements.len();
        self.queue_agent_draft(
            session,
            console,
            Draft {
                plan: RunPlan {
                    statements,
                    rows: vec![0; count],
                    starts: vec![0; count],
                    ordinals: Vec::new(),
                },
                options: RunOptions {
                    schema: Some(schema.into()),
                    only_schema: true,
                    row_cap: Some(usize::MAX),
                    ..Default::default()
                },
                revision: None,
                explain: None,
                internal: true,
                split: None,
                read_only: true,
                confirmed: false,
            },
            Output::Ddl(target, label, dialect),
            0,
        )
    }

    pub(in crate::app) fn agent_diagram(
        &mut self,
        session: &str,
        connection: ConnectionId,
        schema: &str,
        table: Option<&str>,
    ) -> Reply {
        let dialect = self.connection(connection).ok_or_else(missing)?.dialect;
        let focus = table.map_or_else(
            || crate::diagram::Focus::Schema(schema.into()),
            |table| {
                crate::diagram::Focus::Table(RelationRef {
                    schema: schema.into(),
                    relation: table.into(),
                })
            },
        );
        let console = self
            .manager
            .as_mut()
            .and_then(|manager| manager.open_console(connection))
            .ok_or_else(missing)?;
        let opened = self.next_tab_place();
        let mut tab = crate::diagram_tab::DiagramTab::new(console, connection, focus, opened);
        let Some(statements) = tab.statements(dialect) else {
            self.close_agent_consoles(&[console]);
            return Err(supported());
        };
        let job = match self.new_agent_job(
            session,
            vec![console],
            vec![connection],
            None,
            Output::Diagram(console),
        ) {
            Ok(job) => job,
            Err(error) => {
                self.close_agent_consoles(&[console]);
                return Err(error);
            }
        };
        tab.phase = Phase::Waiting;
        tab.agent = true;
        let notice = text::agent_opened(&self.agent.client(session), &tab.label());
        self.agent_notice(session, Some(connection), notice);
        self.tabs.diagrams.push(tab);
        if let Some(item) = self.agent.jobs.get_mut(&job) {
            item.state = "waiting_credentials";
        }
        self.authorize(
            connection,
            Use::Diagram {
                console,
                connection,
                statements,
            },
        );
        self.agent_job_reply(session, &job)
    }

    pub(in crate::app) fn agent_compare(
        &mut self,
        session: &str,
        source: &agent::Side,
        target: &agent::Side,
        removals: bool,
        kind: crate::compare::Kind,
        rows: usize,
    ) -> Reply {
        let sides = [source, target]
            .map(|side| {
                Ok(crate::compare::Side {
                    connection: id(&side.connection_id)?,
                    schema: side.schema.clone(),
                    table: side.table.clone(),
                })
            })
            .into_iter()
            .collect::<Result<Vec<_>, Error>>()?;
        for side in &sides {
            let item = self.connection(side.connection).ok_or_else(missing)?;
            if item.dialect.is_command_line()
                || (kind == crate::compare::Kind::Data && side.table.is_none())
            {
                return Err(supported());
            }
        }
        let mut reads: Vec<crate::compare_tab::SideRead> = Vec::new();
        for side in sides {
            let connection = side.connection;
            let read = self
                .connection(connection)
                .map(|item| (item.dialect, text::agent_tab_label(item.plain_name())))
                .and_then(|(dialect, name)| {
                    let console = self.manager.as_mut()?.open_console(connection)?;
                    Some(crate::compare_tab::SideRead::new(
                        console, side, dialect, name,
                    ))
                });
            let Some(read) = read else {
                let opened: Vec<_> = reads.iter().map(|read| read.id).collect();
                self.close_agent_consoles(&opened);
                return Err(missing());
            };
            reads.push(read);
        }
        let consoles = reads.iter().map(|read| read.id).collect::<Vec<_>>();
        let connections = reads.iter().map(|read| read.side.connection).collect();
        let first = consoles[0];
        let Ok(sides) = reads.try_into() else {
            self.close_agent_consoles(&consoles);
            return Err(missing());
        };
        let opened = self.next_tab_place();
        let mut tab = crate::compare_tab::CompareTab::new(opened, kind, sides, removals);
        let requests = (0..2)
            .map(|side| {
                let read = &tab.sides[side];
                Ok((
                    read.id,
                    read.side.connection,
                    read.side.schema.clone(),
                    tab.statements(side).ok_or_else(supported)?,
                ))
            })
            .collect::<Result<Vec<_>, Error>>();
        let started = requests.and_then(|requests| {
            self.new_agent_job(
                session,
                consoles.clone(),
                connections,
                None,
                Output::Compare(first),
            )
            .map(|job| (job, requests))
        });
        let (job, requests) = match started {
            Ok(started) => started,
            Err(error) => {
                self.close_agent_consoles(&consoles);
                return Err(error);
            }
        };
        tab.agent = true;
        tab.begin(&self.budget);
        let notice = text::agent_opened(&self.agent.client(session), &tab.label());
        let connection = tab.sides[0].side.connection;
        self.agent_notice(session, Some(connection), notice);
        self.tabs.compares.push(tab);
        if let Some(item) = self.agent.jobs.get_mut(&job) {
            item.state = "waiting_credentials";
            item.rows = rows;
        }
        for (console, connection, schema, statements) in requests {
            self.authorize(
                connection,
                Use::Compare {
                    console,
                    connection,
                    schema,
                    statements,
                    untimed: false,
                },
            );
        }
        self.agent_job_reply(session, &job)
    }
}

/// Writes the field paths of a Kafka topic for the agent. An empty list says why.
pub(super) fn fields_value(
    topic: &str,
    paths: &[String],
    notes: &[crate::catalog::ColumnNote],
) -> Value {
    let fields = crate::connector::kafka::field_json(paths, notes);
    let mut value = json!({"topic": topic, "fields": fields});
    if paths.is_empty() {
        value["message"] = json!(text::AGENT_KAFKA_NO_FIELDS);
    }
    value
}

pub(super) fn catalog_value(catalog: &crate::catalog::Catalog) -> Value {
    json!({"current_schema":catalog.current_schema,"schemas":catalog.schemas.iter().map(|schema|json!({"name":schema.name,"relations":schema.relations.iter().map(|relation|json!({"name":relation.name,"kind":format!("{:?}",relation.kind).to_ascii_lowercase(),"columns":match &relation.columns {crate::catalog::Columns::Loaded(columns)=>Some(columns),_=>None}})).collect::<Vec<_>>(),"objects":schema.objects.iter().map(|object|json!({"name":object.name,"kind":format!("{:?}",object.kind).to_ascii_lowercase(),"detail":object.detail})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
}
