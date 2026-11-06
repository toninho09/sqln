//! Structural metadata transforms. Database readers live in the executor.
use super::{Memory, Object, Snapshot};
use crate::catalog::Catalog;
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

/// Comments supplement existing structural readers. SQLite has no comment catalog.
pub fn comments_sql(dialect: Dialect, schema: &str) -> Option<String> {
    let literal = crate::structure::name_literal(schema, dialect);
    match dialect {
        Dialect::Postgres => Some(format!(
            "SELECT c.relname::text AS relation, a.attname::text AS column_name, pg_catalog.col_description(c.oid,a.attnum) AS comment FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace LEFT JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped WHERE n.nspname={literal} AND c.relkind IN ('r','p','f','v','m') UNION ALL SELECT c.relname::text,NULL::text,pg_catalog.obj_description(c.oid,'pg_class') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname={literal} AND c.relkind IN ('r','p','f','v','m')"
        )),
        Dialect::MySql => Some(format!(
            "SELECT TABLE_NAME AS relation,COLUMN_NAME AS column_name,COLUMN_COMMENT AS comment FROM information_schema.COLUMNS WHERE TABLE_SCHEMA={literal} UNION ALL SELECT TABLE_NAME,NULL,TABLE_COMMENT FROM information_schema.TABLES WHERE TABLE_SCHEMA={literal}"
        )),
        _ => None,
    }
}
