//! Metadata cache in memory. It stays out of the disk, and it reads columns on demand.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RelationKind {
    #[default]
    Table,
    View,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Columns {
    #[default]
    NotLoaded,
    Loaded(Vec<String>),
}

/// One foreign key of a relation. The two lists of columns have the same order.
/// `columns` belong to the relation that holds the key in its list, and `target` is the
/// other table. A key that reaches the relation has the same shape: its `columns` are the
/// columns of the relation that the key reads.
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

/// Joins the pairs of each key. The query returns the pairs of a key in a sequence. Two
/// keys of two tables can have the same name, so a key also needs the same table.
pub fn group_foreign_keys(pairs: impl IntoIterator<Item = KeyColumn>) -> Vec<ForeignKey> {
    let mut keys: Vec<ForeignKey> = Vec::new();
    let mut last = None;
    for pair in pairs {
        let open = last.as_ref() == Some(&pair.key)
            && keys.last().is_some_and(|key| key.target == pair.target);
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relation {
    pub name: String,
    pub kind: RelationKind,
    pub columns: Columns,
    /// The keys that leave this relation. They arrive with its columns.
    pub foreign_keys: Vec<ForeignKey>,
    /// The keys of other tables that reach this relation. They arrive with its columns.
    pub referencing: Vec<ForeignKey>,
}

/// The group of the explorer that holds an object. The order is the order of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Routines,
    Sequences,
    Triggers,
    Types,
}

impl Group {
    pub const ALL: [Self; 4] = [Self::Routines, Self::Sequences, Self::Triggers, Self::Types];
}

/// A kind of object of a schema that is not a relation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    Function,
    Procedure,
    Sequence,
    Trigger,
    Enum,
    Domain,
}

impl ObjectKind {
    /// Reads the kind that a catalog query writes in lowercase, such as `function`.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "function" => Self::Function,
            "procedure" => Self::Procedure,
            "sequence" => Self::Sequence,
            "trigger" => Self::Trigger,
            "enum" => Self::Enum,
            "domain" => Self::Domain,
            _ => return None,
        })
    }

    pub fn group(self) -> Group {
        match self {
            Self::Function | Self::Procedure => Group::Routines,
            Self::Sequence => Group::Sequences,
            Self::Trigger => Group::Triggers,
            Self::Enum | Self::Domain => Group::Types,
        }
    }
}

/// One object of a schema that is not a relation, such as a function or a trigger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub name: String,
    pub kind: ObjectKind,
    /// The arguments of a routine on PostgreSQL, or the table of a trigger.
    pub detail: String,
    /// The text of the tree. It is unique in the group, so two overloads stay apart.
    pub label: String,
}

impl Object {
    pub fn new(name: String, kind: ObjectKind, detail: String) -> Self {
        let label = match kind {
            ObjectKind::Function | ObjectKind::Procedure if !detail.is_empty() => {
                format!("{name}({detail})")
            }
            ObjectKind::Trigger if !detail.is_empty() => format!("{name} ({detail})"),
            _ => name.clone(),
        };
        Self {
            name,
            kind,
            detail,
            label,
        }
    }
}

/// Gives each object to its schema, and puts the objects of each schema in the order of
/// the tree: by group, then by label. A row has the schema, the name, the kind in
/// lowercase, and the detail. A row of an unknown kind or schema stays out.
pub fn add_objects<I>(schemas: &mut [Schema], rows: I)
where
    I: IntoIterator<Item = [String; 4]>,
{
    for [schema, name, kind, detail] in rows {
        let Some(kind) = ObjectKind::parse(&kind) else {
            continue;
        };
        if let Some(found) = schemas.iter_mut().find(|item| item.name == schema) {
            found.objects.push(Object::new(name, kind, detail));
        }
    }
    for schema in schemas {
        schema
            .objects
            .sort_by_cached_key(|object| (object.kind.group(), object.label.to_lowercase()));
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schema {
    pub name: String,
    pub relations: Vec<Relation>,
    /// True when the list holds only a part of the relations.
    pub partial: bool,
    /// The routines, sequences, triggers, and types, in the order of the tree.
    pub objects: Vec<Object>,
}

impl Schema {
    /// Returns the objects of one group.
    pub fn objects_of(&self, group: Group) -> impl Iterator<Item = (usize, &Object)> {
        self.objects
            .iter()
            .enumerate()
            .filter(move |(_, object)| object.kind.group() == group)
    }
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

    /// Keeps the schemas of the list and the current schema. An empty list keeps all.
    pub fn keep_schemas(&mut self, names: &[String]) {
        if names.is_empty() {
            return;
        }
        let current = self.current_schema.as_deref();
        self.schemas
            .retain(|schema| current == Some(&schema.name) || names.contains(&schema.name));
    }
}

impl Catalog {
    pub fn set_columns(
        &mut self,
        target: &RelationRef,
        columns: Vec<String>,
        foreign_keys: Vec<ForeignKey>,
        referencing: Vec<ForeignKey>,
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
            relation.referencing = referencing;
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
