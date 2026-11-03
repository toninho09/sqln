//! The ER diagram of a schema: the model, its catalog queries, its layout, and its
//! Mermaid text. The model and the layout depend only on the rows of the two queries.

pub mod layout;
pub mod mermaid;
pub mod sql;
pub mod tab;

use std::collections::{BTreeMap, BTreeSet};

use crate::catalog::RelationRef;

/// What a diagram shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Focus {
    /// All the tables of a schema.
    Schema(String),
    /// One table and the tables that its foreign keys join, one step away.
    Table(RelationRef),
}

impl Focus {
    /// Returns the schema that the queries read.
    pub fn schema(&self) -> &str {
        match self {
            Self::Schema(schema) => schema,
            Self::Table(table) => &table.schema,
        }
    }

    /// Returns the name of the tab.
    pub fn name(&self) -> &str {
        match self {
            Self::Schema(schema) => schema,
            Self::Table(table) => &table.relation,
        }
    }
}

/// One column, as the column query returns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnRow {
    pub relation: RelationRef,
    pub name: String,
    pub ty: String,
    pub nullable: bool,
    /// The place of the column in the primary key, from 1.
    pub primary: Option<u32>,
}

/// One pair of columns of a foreign key, as the key query returns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRow {
    pub relation: RelationRef,
    /// The name of the key. SQLite gives a number.
    pub key: String,
    pub column: String,
    pub target: RelationRef,
    /// SQLite gives no column when the key reads the primary key of the target.
    pub target_column: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub ty: String,
    pub nullable: bool,
    pub primary: Option<u32>,
    /// True when a foreign key of the diagram leaves this column.
    pub foreign: bool,
    /// True when a foreign key of the diagram reads this column.
    pub referenced: bool,
}

impl Column {
    /// Returns true for a column that the "keys only" form shows.
    pub fn is_key(&self) -> bool {
        self.primary.is_some() || self.foreign || self.referenced
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entity {
    pub relation: RelationRef,
    /// True for a table of another schema. It shows only its name.
    pub stub: bool,
    pub columns: Vec<Column>,
}

impl Entity {
    /// Returns the positions of the columns that a box shows.
    pub fn shown(&self, keys_only: bool) -> Vec<usize> {
        (0..self.columns.len())
            .filter(|&index| !keys_only || self.columns[index].is_key())
            .collect()
    }
}

/// One foreign key between two entities of the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub name: String,
    /// The entity that holds the key.
    pub child: usize,
    pub child_columns: Vec<usize>,
    /// The entity that the key reads.
    pub parent: usize,
    /// `None` for a column that the model does not know, as in a stub.
    pub parent_columns: Vec<Option<usize>>,
}

impl Link {
    /// Returns true when the link joins the entity to another entity or to itself.
    pub fn touches(&self, entity: usize) -> bool {
        self.child == entity || self.parent == entity
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Model {
    /// The tables of the schema in name order, and then the stubs in name order.
    pub entities: Vec<Entity>,
    /// In the order of the child, and then of the name.
    pub links: Vec<Link>,
}

impl Model {
    /// Builds the model from the rows of the two queries.
    pub fn build(columns: Vec<ColumnRow>, keys: Vec<KeyRow>, focus: &Focus) -> Self {
        let mut tables: BTreeMap<(String, String), Entity> = BTreeMap::new();
        for row in columns {
            let entry = tables
                .entry((row.relation.schema.clone(), row.relation.relation.clone()))
                .or_insert_with(|| Entity {
                    relation: row.relation,
                    stub: false,
                    columns: Vec::new(),
                });
            entry.columns.push(Column {
                name: row.name,
                ty: row.ty,
                nullable: row.nullable,
                primary: row.primary,
                foreign: false,
                referenced: false,
            });
        }
        let mut entities: Vec<Entity> = tables.into_values().collect();
        let keys = group_keys(keys);

        // A side of a key that is not a table of the query becomes a stub.
        let mut stubs: BTreeSet<(String, String)> = BTreeSet::new();
        for key in &keys {
            for side in [&key.child, &key.target] {
                if find_entity(&entities, side).is_none() {
                    stubs.insert((side.schema.clone(), side.relation.clone()));
                }
            }
        }
        entities.extend(stubs.into_iter().map(|(schema, relation)| Entity {
            relation: RelationRef { schema, relation },
            stub: true,
            columns: Vec::new(),
        }));

        let mut links: Vec<Link> = keys
            .into_iter()
            .filter_map(|key| resolve(&entities, key))
            .collect();
        links.sort_by(|a, b| (a.child, &a.name, a.parent).cmp(&(b.child, &b.name, b.parent)));

        let mut model = Self { entities, links };
        if let Focus::Table(table) = focus {
            model = model.around(table);
        }
        model.mark_keys();
        model
    }

    /// Keeps one table and the tables that its keys join.
    fn around(self, table: &RelationRef) -> Self {
        let Some(center) = find_entity(&self.entities, table) else {
            return Self::default();
        };
        let mut keep = vec![false; self.entities.len()];
        keep[center] = true;
        for link in &self.links {
            if link.child == center {
                keep[link.parent] = true;
            }
            if link.parent == center {
                keep[link.child] = true;
            }
        }
        let mut place = vec![None; self.entities.len()];
        let mut entities = Vec::new();
        for (index, entity) in self.entities.into_iter().enumerate() {
            if keep[index] {
                place[index] = Some(entities.len());
                entities.push(entity);
            }
        }
        let links = self
            .links
            .into_iter()
            .filter_map(|link| {
                Some(Link {
                    child: place[link.child]?,
                    parent: place[link.parent]?,
                    ..link
                })
            })
            .collect();
        Self { entities, links }
    }

    fn mark_keys(&mut self) {
        for link in &self.links {
            for &column in &link.child_columns {
                self.entities[link.child].columns[column].foreign = true;
            }
            for column in link.parent_columns.iter().flatten() {
                self.entities[link.parent].columns[*column].referenced = true;
            }
        }
    }

    /// Returns true when a column of the key accepts `NULL`, so a child can have no parent.
    pub fn is_optional(&self, link: &Link) -> bool {
        let child = &self.entities[link.child];
        link.child_columns.is_empty()
            || link
                .child_columns
                .iter()
                .any(|&column| child.columns[column].nullable)
    }

    /// Returns true when each column of the key is in the primary key of the child.
    pub fn is_identifying(&self, link: &Link) -> bool {
        let child = &self.entities[link.child];
        !link.child_columns.is_empty()
            && link
                .child_columns
                .iter()
                .all(|&column| child.columns[column].primary.is_some())
    }
}

/// The pairs of one key, joined.
struct KeyGroup {
    name: String,
    child: RelationRef,
    columns: Vec<String>,
    target: RelationRef,
    target_columns: Vec<Option<String>>,
}

/// Joins the pairs of each key. A key name repeats in other tables, so the table is part
/// of the identity.
fn group_keys(rows: Vec<KeyRow>) -> Vec<KeyGroup> {
    let mut groups: BTreeMap<(String, String, String), KeyGroup> = BTreeMap::new();
    for row in rows {
        let identity = (
            row.relation.schema.clone(),
            row.relation.relation.clone(),
            row.key.clone(),
        );
        let group = groups.entry(identity).or_insert_with(|| KeyGroup {
            name: row.key,
            child: row.relation,
            columns: Vec::new(),
            target: row.target,
            target_columns: Vec::new(),
        });
        group.columns.push(row.column);
        group.target_columns.push(row.target_column);
    }
    groups.into_values().collect()
}

/// Finds an entity. An exact name has priority over a name with another case, because
/// SQLite keeps the case that the key text uses.
fn find_entity(entities: &[Entity], relation: &RelationRef) -> Option<usize> {
    entities
        .iter()
        .position(|entity| entity.relation == *relation)
        .or_else(|| {
            entities.iter().position(|entity| {
                entity.relation.schema == relation.schema
                    && entity
                        .relation
                        .relation
                        .eq_ignore_ascii_case(&relation.relation)
            })
        })
}

fn find_column(entity: &Entity, name: &str) -> Option<usize> {
    let columns = &entity.columns;
    columns
        .iter()
        .position(|column| column.name == name)
        .or_else(|| {
            columns
                .iter()
                .position(|column| column.name.eq_ignore_ascii_case(name))
        })
}

fn resolve(entities: &[Entity], key: KeyGroup) -> Option<Link> {
    let child = find_entity(entities, &key.child)?;
    let parent = find_entity(entities, &key.target)?;
    let child_columns = key
        .columns
        .iter()
        .filter_map(|name| find_column(&entities[child], name))
        .collect();
    let target = &entities[parent];
    let parent_columns = if key.target_columns.iter().all(Option::is_none) {
        primary_key(target)
    } else {
        key.target_columns
            .iter()
            .map(|name| name.as_deref().and_then(|name| find_column(target, name)))
            .collect()
    };
    Some(Link {
        name: key.name,
        child,
        child_columns,
        parent,
        parent_columns,
    })
}

/// Returns the columns of the primary key in key order.
fn primary_key(entity: &Entity) -> Vec<Option<usize>> {
    let mut places: Vec<(u32, usize)> = entity
        .columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| Some((column.primary?, index)))
        .collect();
    places.sort_unstable();
    places.into_iter().map(|(_, index)| Some(index)).collect()
}
