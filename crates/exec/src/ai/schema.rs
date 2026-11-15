//! Structural metadata transforms. Database readers live in the executor.
use super::{Memory, Object, Snapshot};
use crate::catalog::{Catalog, ColumnNote};
use crate::grid::{Budget, Row};
use crate::sql::Dialect;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

pub enum Event {
    Waiting(u64, u64),
    Reading(u64, u64),
    Loaded(u64, u64, Result<Arc<Snapshot>, String>),
}
pub struct Builder {
    objects: BTreeMap<(String, String), Object>,
    memory: Memory,
}
impl Builder {
    pub fn new(catalog: &Catalog, budget: &Arc<Budget>) -> Result<Self, String> {
        let mut builder = Self {
            objects: BTreeMap::new(),
            memory: Memory::new(budget),
        };
        for schema in &catalog.schemas {
            if schema.partial {
                return Err(
                    "The catalog is incomplete. Limit the schemas and refresh before using AI."
                        .into(),
                );
            }
            for relation in &schema.relations {
                builder
                    .memory
                    .charge(schema.name.len() + relation.name.len() + 512)?;
                builder.objects.insert(
                    (schema.name.clone(), relation.name.clone()),
                    Object {
                        schema: schema.name.clone(),
                        name: relation.name.clone(),
                        kind: format!("{:?}", relation.kind),
                        metadata: Vec::new(),
                        targets: Vec::new(),
                    },
                );
            }
            for object in &schema.objects {
                builder.memory.charge(
                    schema.name.len() + object.name.len() + object.detail.len() * 2 + 512,
                )?;
                builder.objects.insert(
                    (schema.name.clone(), object.label.clone()),
                    Object {
                        schema: schema.name.clone(),
                        name: object.name.clone(),
                        kind: format!("{:?}", object.kind),
                        metadata: vec![json!({"signature":object.detail})],
                        targets: Vec::new(),
                    },
                );
            }
        }
        Ok(builder)
    }
    pub fn add(
        &mut self,
        schema: &str,
        section: &str,
        columns: &[String],
        row: &Row,
        all_columns: bool,
    ) -> Result<(), String> {
        let index = if section == "definition" {
            2
        } else {
            usize::from(all_columns)
        };
        let Some(name) = row.text(index) else {
            return Ok(());
        };
        let Some(object) = self.objects.get_mut(&(schema.to_owned(), name)) else {
            return Ok(());
        };
        let mut data = serde_json::Map::new();
        for (i, column) in columns.iter().enumerate().skip(index + 1) {
            data.insert(
                column.clone(),
                row.text(i).map_or(Value::Null, Value::String),
            );
        }
        let value = json!({"section":section,"values":data});
        self.memory.charge(value.to_string().len() * 2 + 128)?;
        if section == "constraints"
            && row.text(2).as_deref() == Some("f")
            && let Some(target) = row.text(7)
        {
            object
                .targets
                .push((row.text(6).unwrap_or_else(|| schema.to_owned()), target));
        }
        object.metadata.push(value);
        Ok(())
    }
    pub fn finish(self) -> Snapshot {
        Snapshot {
            objects: self.objects.into_values().collect(),
            _memory: self.memory,
        }
    }
}

/// The field paths of one Kafka topic, with a note for each path.
pub type TopicFields = (String, Vec<String>, Vec<ColumnNote>);

/// Builds the snapshot of a Kafka connection: one object for each topic and each group of
/// the catalog. Only the topics in `fields` get their field paths, in the `columns`
/// section, so a reduced context keeps them.
pub fn kafka_snapshot(
    catalog: &Catalog,
    fields: &[TopicFields],
    budget: &Arc<Budget>,
) -> Result<Snapshot, String> {
    use crate::connector::kafka::{GROUPS, TOPICS};
    let mut objects = Vec::new();
    for schema in &catalog.schemas {
        let kind = match schema.name.as_str() {
            TOPICS => "Topic",
            GROUPS => "Group",
            _ => continue,
        };
        for relation in &schema.relations {
            let paths = fields
                .iter()
                .find(|(topic, ..)| schema.name == TOPICS && topic == &relation.name);
            let metadata = paths.map_or_else(Vec::new, |(_, paths, notes)| {
                crate::connector::kafka::field_json(paths, notes)
                    .into_iter()
                    .map(|values| json!({"section": "columns", "values": values}))
                    .collect()
            });
            objects.push(Object {
                schema: schema.name.clone(),
                name: relation.name.clone(),
                kind: kind.to_owned(),
                metadata,
                targets: Vec::new(),
            });
        }
    }
    Snapshot::new(objects, budget)
}

/// Comments supplement existing structural readers. SQLite has no comment catalog.
pub fn comments_sql(dialect: Dialect, schema: &str) -> Option<String> {
    crate::dialect::features(dialect)
        .ai_comments_sql(&crate::structure::name_literal(schema, dialect))
}
