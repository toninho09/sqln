//! Metadata cache in memory. It stays out of the disk, and it reads columns on demand.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationKind {
    Table,
    View,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Columns {
    NotLoaded,
    Loaded(Vec<String>),
}

/// One foreign key of a relation. The two lists of columns have the same order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignKey {
    pub columns: Vec<String>,
    pub target: RelationRef,
    pub target_columns: Vec<String>,
}

/// One pair of columns of a foreign key, as a catalog query returns it.
pub struct KeyColumn {
    /// The name of the key. It separates two keys to the same table.
    pub key: String,
    pub target: RelationRef,
    pub column: String,
    pub target_column: String,
}

/// Joins the pairs of each key. The query returns the pairs of a key in a sequence.
pub fn group_foreign_keys(pairs: impl IntoIterator<Item = KeyColumn>) -> Vec<ForeignKey> {
    let mut keys: Vec<ForeignKey> = Vec::new();
    let mut last = None;
    for pair in pairs {
        let open = last.as_ref() == Some(&pair.key);
        match keys.last_mut().filter(|_| open) {
            Some(key) => {
                key.columns.push(pair.column);
                key.target_columns.push(pair.target_column);
            }
            None => keys.push(ForeignKey {
                columns: vec![pair.column],
                target: pair.target,
                target_columns: vec![pair.target_column],
            }),
        }
        last = Some(pair.key);
    }
    keys
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relation {
    pub name: String,
    pub kind: RelationKind,
    pub columns: Columns,
    /// The keys that leave this relation. They arrive with its columns.
    pub foreign_keys: Vec<ForeignKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub name: String,
    pub relations: Vec<Relation>,
    /// True when the list holds only a part of the relations.
    pub partial: bool,
}

/// Identifies one relation of the catalog.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RelationRef {
    pub schema: String,
    pub relation: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub schemas: Vec<Schema>,
    /// The schema that the session reads without a qualifier.
    pub current_schema: Option<String>,
}

impl Catalog {
    pub fn schema(&self, name: &str) -> Option<&Schema> {
        find_by_name(&self.schemas, name, |schema| &schema.name)
    }

    pub fn is_current(&self, schema: &Schema) -> bool {
        self.current_schema
            .as_deref()
            .is_none_or(|name| name.eq_ignore_ascii_case(&schema.name))
    }

    /// Finds a relation. Without a schema name, the current schema has priority.
    pub fn relation(&self, schema: Option<&str>, name: &str) -> Option<(&Schema, &Relation)> {
        let in_schema = |schema: &'_ Schema| {
            find_by_name(&schema.relations, name, |relation| &relation.name).is_some()
        };
        let found = match schema {
            Some(schema) => self.schema(schema).filter(|found| in_schema(found)),
            None => self
                .schemas
                .iter()
                .find(|schema| self.is_current(schema) && in_schema(schema))
                .or_else(|| self.schemas.iter().find(|schema| in_schema(schema))),
        }?;
        let relation = find_by_name(&found.relations, name, |relation| &relation.name)?;
        Some((found, relation))
    }
}

impl Catalog {
    pub fn set_columns(
        &mut self,
        target: &RelationRef,
        columns: Vec<String>,
        foreign_keys: Vec<ForeignKey>,
    ) {
        let schema = self
            .schemas
            .iter_mut()
            .find(|schema| schema.name == target.schema);
        let relation = schema.and_then(|schema| {
            schema
                .relations
                .iter_mut()
                .find(|relation| relation.name == target.relation)
        });
        if let Some(relation) = relation {
            relation.columns = Columns::Loaded(columns);
            relation.foreign_keys = foreign_keys;
        }
    }
}

/// Prefers an exact match, then a match without case.
fn find_by_name<'a, T>(items: &'a [T], name: &str, key: impl Fn(&T) -> &String) -> Option<&'a T> {
    items.iter().find(|item| key(item) == name).or_else(|| {
        items
            .iter()
            .find(|item| key(item).eq_ignore_ascii_case(name))
    })
}
