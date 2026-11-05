//! Bounded snapshots for diagram output.
use crate::diagram::{Column, Entity, Link, Model};
use serde_json::{Value, json};
use std::mem::size_of;

pub(super) fn diagram_bytes(model: &Model) -> usize {
    model.entities.capacity() * size_of::<Entity>()
        + model.links.capacity() * size_of::<Link>()
        + model
            .entities
            .iter()
            .map(|entity| {
                entity.relation.schema.capacity()
                    + entity.relation.relation.capacity()
                    + entity.columns.capacity() * size_of::<Column>()
                    + entity
                        .columns
                        .iter()
                        .map(|column| column.name.capacity() + column.ty.capacity())
                        .sum::<usize>()
            })
            .sum::<usize>()
        + model
            .links
            .iter()
            .map(|link| {
                link.name.capacity()
                    + link.child_columns.capacity() * size_of::<usize>()
                    + link.parent_columns.capacity() * size_of::<Option<usize>>()
            })
            .sum::<usize>()
}
pub(super) fn diagram_value(model: &Model, partial: bool) -> Value {
    json!({"mermaid":crate::diagram::mermaid::to_mermaid(model,false),"partial":partial,
        "entities":model.entities.iter().map(|entity|json!({"schema":entity.relation.schema,"table":entity.relation.relation,"stub":entity.stub,"columns":entity.columns.iter().map(|column|json!({"name":column.name,"type":column.ty,"nullable":column.nullable,"primary":column.primary,"foreign":column.foreign,"referenced":column.referenced})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "links":model.links.iter().map(|link|json!({"name":link.name,"child":link.child,"parent":link.parent,"child_columns":link.child_columns,"parent_columns":link.parent_columns})).collect::<Vec<_>>()})
}
