//! The field paths of a schema, for the completion of `value.` and `key.`. The paths
//! follow the JSON of the decoder, so each path names a field of a decoded message.
//! A list gives `[*]` before the fields of its items. A map and a value without a fixed
//! set of names give no fields. Each path has a note: the type words of the schema
//! format, and the literals of the values of an enum.

use std::collections::HashMap;

use apache_avro::Schema as AvroSchema;
use apache_avro::schema::{Name, NamesRef, ResolvedSchema};
use prost_reflect::{FieldDescriptor, Kind, MessageDescriptor};
use serde_json::{Map, Value as Json};

use super::filter::path_name;
use super::registry::{Parsed, main_file};
use crate::catalog::ColumnNote;
use crate::sql::Dialect;
use crate::sql::literal::text_literal;

/// The most steps after the root in one path.
const DEPTH_LIMIT: usize = 10;
/// The most paths of one root.
const PATH_LIMIT: usize = 2000;
/// The most schema nodes that one walk visits. A schema that uses the same type many
/// times can repeat paths without a new path, so the path limit alone does not stop it.
const STEP_LIMIT: usize = 100_000;
/// The most values in the note of one path.
const VALUE_LIMIT: usize = 100;

/// Writes each field path with its note as JSON, for the AI completion and the MCP
/// agents: the `path`, the `type` words, and the `values` of an enum.
pub fn field_json(paths: &[String], notes: &[ColumnNote]) -> Vec<Json> {
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let mut field = Map::new();
            field.insert("path".into(), Json::from(path.as_str()));
            if let Some(note) = notes.get(index) {
                if !note.detail.is_empty() {
                    field.insert("type".into(), Json::from(note.detail.as_str()));
                }
                if !note.values.is_empty() {
                    field.insert("values".into(), Json::from(note.values.clone()));
                }
            }
            Json::Object(field)
        })
        .collect()
}

/// Returns the field paths of a schema below `root`, such as `value.customer.id`, with a
/// note for each path. Each path that has fields below it also comes first as a path.
pub fn paths(root: &str, parsed: &Parsed) -> (Vec<String>, Vec<ColumnNote>) {
    let mut out = Out::default();
    match parsed {
        Parsed::Avro { schema, references } => {
            let schemata = references.iter().chain([schema]).collect();
            // A failure leaves the references without fields.
            let names = ResolvedSchema::new_with_schemata(schemata)
                .map(|resolved| resolved.get_names().clone())
                .unwrap_or_default();
            avro(&mut out, &names, root, 0, schema, &mut Vec::new());
        }
        Parsed::Protobuf { pool } => {
            let first = pool
                .get_file_by_name(main_file())
                .and_then(|file| file.messages().next());
            if let Some(message) = first {
                protobuf(&mut out, root, 0, &message, &mut Vec::new());
            }
        }
        Parsed::Json { text, references } => {
            let Ok(main) = serde_json::from_str::<Json>(text) else {
                return (Vec::new(), Vec::new());
            };
            let documents: Vec<(String, Json)> = references
                .iter()
                .filter_map(|(name, text)| Some((name.clone(), serde_json::from_str(text).ok()?)))
                .collect();
            let walk = JsonSchema {
                documents: &documents,
            };
            walk.node(&mut out, root, 0, &main, &main, &mut Vec::new());
        }
    }
    (out.paths, out.notes)
}

#[derive(Default)]
struct Out {
    paths: Vec<String>,
    notes: Vec<ColumnNote>,
    /// The position of each path in `paths`.
    index: HashMap<String, usize>,
    steps: usize,
}

impl Out {
    /// Adds a path once. A path that repeats joins its note to the first one. It returns
    /// false when the list is full.
    fn add(&mut self, path: &str, note: ColumnNote) -> bool {
        if self.paths.len() >= PATH_LIMIT {
            return false;
        }
        match self.index.get(path) {
            Some(&at) => join(&mut self.notes[at], note),
            None => {
                self.index.insert(path.to_owned(), self.paths.len());
                self.paths.push(path.to_owned());
                self.notes.push(note);
            }
        }
        true
    }

    /// Joins a note to a path that the list holds already.
    fn join(&mut self, path: &str, note: ColumnNote) {
        if let Some(&at) = self.index.get(path) {
            join(&mut self.notes[at], note);
        }
    }

    /// Counts one visited node. It returns false when the walk must stop.
    fn step(&mut self) -> bool {
        self.steps += 1;
        self.steps <= STEP_LIMIT && self.paths.len() < PATH_LIMIT
    }
}

/// Joins two notes: the different type words with ` | `, and the values of both.
fn join(note: &mut ColumnNote, other: ColumnNote) {
    let mut words: Vec<&str> = note
        .detail
        .split(" | ")
        .filter(|word| !word.is_empty())
        .collect();
    for word in other.detail.split(" | ") {
        if !word.is_empty() && !words.contains(&word) {
            words.push(word);
        }
    }
    note.detail = words.join(" | ");
    for value in other.values {
        if note.values.len() >= VALUE_LIMIT {
            break;
        }
        if !note.values.contains(&value) {
            note.values.push(value);
        }
    }
}

fn note(detail: &str) -> ColumnNote {
    ColumnNote {
        detail: detail.to_owned(),
        values: Vec::new(),
    }
}

/// Writes the text values of an enum as Kafka literals.
fn texts(values: impl Iterator<Item = impl AsRef<str>>) -> Vec<String> {
    values
        .take(VALUE_LIMIT)
        .map(|value| text_literal(value.as_ref(), Dialect::Kafka))
        .collect()
}

fn field(path: &str, name: &str) -> String {
    format!("{path}.{}", path_name(name))
}

fn items(path: &str) -> String {
    format!("{path}[*]")
}

/// Adds the fields of an Avro schema. The JSON of a union holds only the value of its
/// branch, so a union gives the fields of each branch.
fn avro<'s>(
    out: &mut Out,
    names: &NamesRef<'s>,
    path: &str,
    depth: usize,
    schema: &'s AvroSchema,
    stack: &mut Vec<&'s Name>,
) {
    if !out.step() || depth >= DEPTH_LIMIT {
        return;
    }
    match schema {
        AvroSchema::Ref { name } => {
            if let Some(found) = names.get(name) {
                avro(out, names, path, depth, found, stack);
            }
        }
        AvroSchema::Union(union) => {
            for variant in union.variants() {
                avro(out, names, path, depth, variant, stack);
            }
        }
        AvroSchema::Array(array) => {
            avro(out, names, &items(path), depth + 1, &array.items, stack);
        }
        AvroSchema::Record(record) if !stack.contains(&&record.name) => {
            stack.push(&record.name);
            for item in &record.fields {
                let child = field(path, &item.name);
                if !out.add(&child, avro_note(names, &item.schema)) {
                    break;
                }
                avro(out, names, &child, depth + 1, &item.schema, stack);
            }
            stack.pop();
        }
        _ => {}
    }
}

/// Returns the note of an Avro type. A union gives the words of each branch, and a named
/// type gives the note of its definition.
fn avro_note(names: &NamesRef<'_>, schema: &AvroSchema) -> ColumnNote {
    match schema {
        // A name always leads to a record, an enum, or a fixed type.
        AvroSchema::Ref { name } => names
            .get(name)
            .map_or_else(|| note(name.name()), |found| avro_note(names, found)),
        AvroSchema::Union(union) => {
            let mut all = ColumnNote::default();
            for variant in union.variants() {
                join(&mut all, avro_note(names, variant));
            }
            all
        }
        AvroSchema::Enum(found) => ColumnNote {
            detail: "enum".to_owned(),
            values: texts(found.symbols.iter()),
        },
        other => note(avro_word(other)),
    }
}

/// Returns the word of an Avro type, as a schema writes it.
fn avro_word(schema: &AvroSchema) -> &'static str {
    match schema {
        AvroSchema::Null => "null",
        AvroSchema::Boolean => "boolean",
        AvroSchema::Int => "int",
        AvroSchema::Long => "long",
        AvroSchema::Float => "float",
        AvroSchema::Double => "double",
        AvroSchema::Bytes => "bytes",
        AvroSchema::String => "string",
        AvroSchema::Array(_) => "array",
        AvroSchema::Map(_) => "map",
        AvroSchema::Union(_) => "union",
        AvroSchema::Record(_) => "record",
        AvroSchema::Enum(_) => "enum",
        AvroSchema::Fixed(_) => "fixed",
        AvroSchema::Decimal(_) | AvroSchema::BigDecimal => "decimal",
        AvroSchema::Uuid(_) => "uuid",
        AvroSchema::Date => "date",
        AvroSchema::TimeMillis => "time-millis",
        AvroSchema::TimeMicros => "time-micros",
        AvroSchema::TimestampMillis => "timestamp-millis",
        AvroSchema::TimestampMicros => "timestamp-micros",
        AvroSchema::TimestampNanos => "timestamp-nanos",
        AvroSchema::LocalTimestampMillis => "local-timestamp-millis",
        AvroSchema::LocalTimestampMicros => "local-timestamp-micros",
        AvroSchema::LocalTimestampNanos => "local-timestamp-nanos",
        AvroSchema::Duration(_) => "duration",
        AvroSchema::Ref { .. } => "ref",
    }
}

/// Adds the fields of a Protobuf message, with the JSON names of the decoder. The fields
/// of a `oneof` are fields of the message. A Google type has its own JSON form, so its
/// fields stay out.
fn protobuf(
    out: &mut Out,
    path: &str,
    depth: usize,
    message: &MessageDescriptor,
    stack: &mut Vec<String>,
) {
    if !out.step() || depth >= DEPTH_LIMIT || stack.iter().any(|name| name == message.full_name()) {
        return;
    }
    stack.push(message.full_name().to_owned());
    for item in message.fields() {
        let child = field(path, item.json_name());
        if !out.add(&child, protobuf_note(&item)) {
            break;
        }
        let Kind::Message(inner) = item.kind() else {
            continue;
        };
        if item.is_map() || inner.full_name().starts_with("google.protobuf.") {
            continue;
        }
        if item.is_list() {
            protobuf(out, &items(&child), depth + 2, &inner, stack);
        } else {
            protobuf(out, &child, depth + 1, &inner, stack);
        }
    }
    stack.pop();
}

/// Returns the note of a Protobuf field. A Google type gives its short name, because its
/// JSON form is not an object of fields.
fn protobuf_note(field: &FieldDescriptor) -> ColumnNote {
    if field.is_map() {
        return note("map");
    }
    let mut found = match field.kind() {
        Kind::Message(inner) if inner.full_name().starts_with("google.protobuf.") => {
            note(inner.name())
        }
        Kind::Message(_) => note("message"),
        Kind::Enum(inner) => ColumnNote {
            detail: "enum".to_owned(),
            values: texts(inner.values().map(|value| value.name().to_owned())),
        },
        Kind::Double => note("double"),
        Kind::Float => note("float"),
        Kind::Int32 => note("int32"),
        Kind::Int64 => note("int64"),
        Kind::Uint32 => note("uint32"),
        Kind::Uint64 => note("uint64"),
        Kind::Sint32 => note("sint32"),
        Kind::Sint64 => note("sint64"),
        Kind::Fixed32 => note("fixed32"),
        Kind::Fixed64 => note("fixed64"),
        Kind::Sfixed32 => note("sfixed32"),
        Kind::Sfixed64 => note("sfixed64"),
        Kind::Bool => note("bool"),
        Kind::String => note("string"),
        Kind::Bytes => note("bytes"),
    };
    if field.is_list() {
        found.detail = format!("repeated {}", found.detail);
    }
    found
}

/// Returns the note of one JSON Schema node: its `type`, or the word of its shape, and
/// the scalar items of `enum` and `const`.
fn json_note(object: &Map<String, Json>) -> ColumnNote {
    let detail = match object.get("type") {
        Some(Json::String(word)) => word.clone(),
        Some(Json::Array(words)) => words
            .iter()
            .filter_map(Json::as_str)
            .collect::<Vec<_>>()
            .join(" | "),
        _ if object.contains_key("properties") => "object".to_owned(),
        _ if object.contains_key("items") || object.contains_key("prefixItems") => {
            "array".to_owned()
        }
        _ if object.contains_key("enum") || object.contains_key("const") => "enum".to_owned(),
        _ => String::new(),
    };
    let listed = object
        .get("enum")
        .and_then(Json::as_array)
        .into_iter()
        .flatten();
    let values = listed
        .chain(object.get("const"))
        .filter_map(|value| match value {
            Json::String(text) => Some(text_literal(text, Dialect::Kafka)),
            Json::Number(number) => Some(number.to_string()),
            Json::Bool(flag) => Some(flag.to_string()),
            Json::Null => Some("null".to_owned()),
            _ => None,
        })
        .take(VALUE_LIMIT)
        .collect();
    ColumnNote { detail, values }
}

/// The documents of the references of a JSON Schema, by reference name.
struct JsonSchema<'a> {
    documents: &'a [(String, Json)],
}

impl<'a> JsonSchema<'a> {
    /// Finds the target of a `$ref`: a JSON Pointer in the same document, or a reference
    /// name with an optional pointer. It returns the document and the target.
    fn target(&self, reference: &str, document: &'a Json) -> Option<(&'a Json, &'a Json)> {
        let (name, pointer) = reference.split_once('#').unwrap_or((reference, ""));
        let document = if name.is_empty() {
            document
        } else {
            &self.documents.iter().find(|(known, _)| known == name)?.1
        };
        Some((document, document.pointer(pointer)?))
    }

    /// Adds the fields of a schema node and joins its note to its path. `$ref`, `allOf`,
    /// `anyOf`, `oneOf`, `then`, and `else` describe the same path, so they give their
    /// notes and fields to it. `stack` holds the targets of the open `$ref`, so a
    /// recursive schema stops.
    fn node(
        &self,
        out: &mut Out,
        path: &str,
        depth: usize,
        schema: &'a Json,
        document: &'a Json,
        stack: &mut Vec<*const Json>,
    ) {
        let Some(object) = schema.as_object() else {
            return;
        };
        if !out.step() {
            return;
        }
        if depth > 0 {
            out.join(path, json_note(object));
        }
        if let Some((inner_document, target)) = object
            .get("$ref")
            .and_then(Json::as_str)
            .and_then(|reference| self.target(reference, document))
        {
            let key = std::ptr::from_ref(target);
            if !stack.contains(&key) {
                stack.push(key);
                self.node(out, path, depth, target, inner_document, stack);
                stack.pop();
            }
        }
        for word in ["allOf", "anyOf", "oneOf"] {
            for inner in object
                .get(word)
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
            {
                self.node(out, path, depth, inner, document, stack);
            }
        }
        for word in ["then", "else"] {
            if let Some(inner) = object.get(word) {
                self.node(out, path, depth, inner, document, stack);
            }
        }
        if depth >= DEPTH_LIMIT {
            return;
        }
        if let Some(properties) = object.get("properties").and_then(Json::as_object) {
            for (name, inner) in properties {
                let child = field(path, name);
                if !out.add(&child, ColumnNote::default()) {
                    return;
                }
                self.node(out, &child, depth + 1, inner, document, stack);
            }
        }
        let list = items(path);
        match object.get("items") {
            Some(Json::Array(each)) => each
                .iter()
                .for_each(|inner| self.node(out, &list, depth + 1, inner, document, stack)),
            Some(inner) => self.node(out, &list, depth + 1, inner, document, stack),
            None => {}
        }
        for inner in object
            .get("prefixItems")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
        {
            self.node(out, &list, depth + 1, inner, document, stack);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::filter::parse_path;
    use super::super::registry::parse;
    use super::*;

    fn of(kind: &str, schema: &str, references: &[(String, String)]) -> Vec<String> {
        paths("value", &parse(kind, schema, references).unwrap()).0
    }

    /// Returns the type words and the values of each path.
    fn notes(kind: &str, schema: &str) -> Vec<(String, String, Vec<String>)> {
        let (paths, notes) = paths("value", &parse(kind, schema, &[]).unwrap());
        assert_eq!(paths.len(), notes.len());
        paths
            .into_iter()
            .zip(notes)
            .map(|(path, note)| (path, note.detail, note.values))
            .collect()
    }

    fn row(path: &str, detail: &str, values: &[&str]) -> (String, String, Vec<String>) {
        (
            path.to_owned(),
            detail.to_owned(),
            values.iter().map(|value| (*value).to_owned()).collect(),
        )
    }

    #[test]
    fn avro_notes_give_the_schema_words_and_the_enum_symbols_in_order() {
        let schema = r#"{"type":"record","name":"Order","fields":[
            {"name":"id","type":"long"},
            {"name":"at","type":{"type":"long","logicalType":"timestamp-millis"}},
            {"name":"customer","type":["null",{"type":"record","name":"Customer",
                "fields":[{"name":"name","type":"string"}]}]},
            {"name":"state","type":{"type":"enum","name":"State","symbols":["LOW","MEDIUM","HIGH"]}},
            {"name":"again","type":["null","State"]},
            {"name":"lines","type":{"type":"array","items":"string"}},
            {"name":"tags","type":{"type":"map","values":"string"}},
            {"name":"either","type":["null",
                {"type":"record","name":"A","fields":[{"name":"x","type":"int"}]},
                {"type":"record","name":"B","fields":[{"name":"x","type":"string"}]}]}]}"#;
        assert_eq!(
            notes("AVRO", schema),
            [
                row("value.id", "long", &[]),
                row("value.at", "timestamp-millis", &[]),
                row("value.customer", "null | record", &[]),
                row("value.customer.name", "string", &[]),
                row("value.state", "enum", &["'LOW'", "'MEDIUM'", "'HIGH'"]),
                row(
                    "value.again",
                    "null | enum",
                    &["'LOW'", "'MEDIUM'", "'HIGH'"]
                ),
                row("value.lines", "array", &[]),
                row("value.tags", "map", &[]),
                row("value.either", "null | record", &[]),
                row("value.either.x", "int | string", &[]),
            ]
        );
    }

    #[test]
    fn protobuf_notes_give_the_field_types_and_the_enum_names() {
        let schema = r#"syntax = "proto3";
            import "google/protobuf/timestamp.proto";
            message Order {
                int64 id = 1;
                repeated Line lines = 2;
                repeated string tags = 3;
                map<string, int32> counts = 4;
                google.protobuf.Timestamp at = 5;
                State state = 6;
            }
            message Line { bool gift = 1; }
            enum State { STATE_NEW = 0; STATE_DONE = 1; }"#;
        assert_eq!(
            notes("PROTOBUF", schema),
            [
                row("value.id", "int64", &[]),
                row("value.lines", "repeated message", &[]),
                row("value.lines[*].gift", "bool", &[]),
                row("value.tags", "repeated string", &[]),
                row("value.counts", "map", &[]),
                row("value.at", "Timestamp", &[]),
                row("value.state", "enum", &["'STATE_NEW'", "'STATE_DONE'"]),
            ]
        );
    }

    #[test]
    fn json_schema_notes_join_types_and_values_of_the_same_path() {
        let schema = r##"{"properties": {
            "kind": {"type": ["string", "null"], "enum": ["it's", "b", null]},
            "level": {"enum": [1, 2.5, true]},
            "fixed": {"const": "only"},
            "shape": {"$ref": "#/$defs/shape"},
            "list": {"items": {"type": "string"}}
        }, "$defs": {"shape": {"oneOf": [{"type": "object"}, {"type": "string", "enum": ["round"]}]}}}"##;
        let mut found = notes("JSON", schema);
        found.sort();
        let mut expected = [
            row("value.kind", "string | null", &[r"'it\'s'", "'b'", "null"]),
            row("value.level", "enum", &["1", "2.5", "true"]),
            row("value.fixed", "enum", &["'only'"]),
            row("value.shape", "object | string", &["'round'"]),
            row("value.list", "array", &[]),
        ];
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn a_note_keeps_at_most_the_value_limit() {
        let symbols: Vec<String> = (0..VALUE_LIMIT + 5)
            .map(|index| format!("\"S{index}\""))
            .collect();
        let schema = format!(
            r#"{{"type":"record","name":"R","fields":[{{"name":"e","type":{{"type":"enum","name":"E","symbols":[{}]}}}}]}}"#,
            symbols.join(",")
        );
        let found = notes("AVRO", &schema);
        assert_eq!(found[0].2.len(), VALUE_LIMIT);
        assert_eq!(found[0].2[0], "'S0'");
    }

    #[test]
    fn avro_gives_records_unions_lists_and_references() {
        let address = r#"{"type":"record","name":"Address","namespace":"t",
            "fields":[{"name":"city","type":"string"}]}"#;
        let order = r#"{"type":"record","name":"Order","namespace":"t","fields":[
            {"name":"id","type":"long"},
            {"name":"customer","type":["null",{"type":"record","name":"Customer",
                "fields":[{"name":"name","type":"string"},{"name":"home","type":"t.Address"}]}]},
            {"name":"lines","type":{"type":"array","items":{"type":"record","name":"Line",
                "fields":[{"name":"sku","type":"string"}]}}},
            {"name":"tags","type":{"type":"map","values":"string"}},
            {"name":"state","type":{"type":"enum","name":"State","symbols":["A","B"]}}]}"#;
        let found = of("AVRO", order, &[("t.Address".into(), address.into())]);
        assert_eq!(
            found,
            [
                "value.id",
                "value.customer",
                "value.customer.name",
                "value.customer.home",
                "value.customer.home.city",
                "value.lines",
                "value.lines[*].sku",
                "value.tags",
                "value.state",
            ]
        );
    }

    #[test]
    fn a_recursive_avro_record_stops_at_its_second_use() {
        let node = r#"{"type":"record","name":"Node","fields":[
            {"name":"value","type":"int"},{"name":"next","type":["null","Node"]}]}"#;
        assert_eq!(
            of("AVRO", node, &[]),
            ["value.value", "value.next"],
            "the record does not repeat inside itself"
        );
    }

    #[test]
    fn protobuf_uses_json_names_and_the_first_message() {
        let schema = r#"syntax = "proto3";
            import "google/protobuf/timestamp.proto";
            message Order {
                int64 order_id = 1;
                Customer customer = 2;
                repeated Line lines = 3;
                map<string, Line> by_sku = 4;
                google.protobuf.Timestamp created_at = 5;
                oneof pay { string card = 6; string pix = 7; }
                Order parent = 8;
            }
            message Customer { string full_name = 1; }
            message Line { string sku = 1; }"#;
        assert_eq!(
            of("PROTOBUF", schema, &[]),
            [
                "value.orderId",
                "value.customer",
                "value.customer.fullName",
                "value.lines",
                "value.lines[*].sku",
                "value.bySku",
                "value.createdAt",
                "value.card",
                "value.pix",
                "value.parent",
            ]
        );
    }

    #[test]
    fn json_schema_follows_references_and_combinations() {
        let schema = r##"{
            "type": "object",
            "properties": {
                "order id": {"type": "string"},
                "customer": {"$ref": "#/$defs/customer"},
                "lines": {"type": "array", "items": {"$ref": "#/definitions/line"}},
                "home": {"$ref": "address.json"},
                "extra": {"type": "object", "additionalProperties": {"type": "string"}},
                "node": {"$ref": "#"}
            },
            "allOf": [{"properties": {"note": {"type": "string"}}}],
            "$defs": {"customer": {"oneOf": [
                {"properties": {"name": {"type": "string"}}},
                {"properties": {"company": {"type": "string"}}}
            ]}},
            "definitions": {"line": {"properties": {"sku": {"type": "string"}}}}
        }"##;
        let address = r#"{"properties": {"city": {"type": "string"}}}"#;
        let mut found = of("JSON", schema, &[("address.json".into(), address.into())]);
        // The order of the properties depends on the map of `serde_json`.
        found.sort();
        let mut expected = [
            "value.\"order id\"",
            "value.customer",
            "value.customer.name",
            "value.customer.company",
            "value.lines",
            "value.lines[*].sku",
            "value.home",
            "value.home.city",
            "value.extra",
            "value.node",
            "value.node.\"order id\"",
            "value.node.customer",
            "value.node.customer.name",
            "value.node.customer.company",
            "value.node.lines",
            "value.node.lines[*].sku",
            "value.node.home",
            "value.node.home.city",
            "value.node.extra",
            "value.node.node",
            "value.node.note",
            "value.note",
        ];
        expected.sort_unstable();
        assert_eq!(found, expected);
    }

    #[test]
    fn the_walk_stops_at_the_path_limit_and_the_depth_limit() {
        let fields: Vec<String> = (0..PATH_LIMIT + 10)
            .map(|index| format!(r#""f{index}": {{}}"#))
            .collect();
        let wide = format!(r#"{{"properties": {{{}}}}}"#, fields.join(","));
        assert_eq!(of("JSON", &wide, &[]).len(), PATH_LIMIT);
        let mut deep = String::from("{}");
        for _ in 0..DEPTH_LIMIT + 5 {
            deep = format!(r#"{{"properties": {{"a": {deep}}}}}"#);
        }
        let found = of("JSON", &deep, &[]);
        assert_eq!(found.len(), DEPTH_LIMIT);
        assert_eq!(
            parse_path(found.last().unwrap()).unwrap().1.len(),
            DEPTH_LIMIT
        );
    }

    #[test]
    fn a_quoted_path_name_reads_back_as_the_same_name() {
        for name in [
            "plain_1",
            "order id",
            "a.b",
            "x[0]",
            "say \"hi\"",
            "a,b",
            "",
            "*",
        ] {
            let path = format!("value.{}", path_name(name));
            let (root, segments) = parse_path(&path).unwrap();
            assert_eq!(root, "value");
            assert_eq!(
                segments,
                [super::super::filter::Segment::Name(name.to_owned())],
                "{path}"
            );
        }
    }
}
