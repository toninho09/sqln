//! The field paths of a schema, for the completion of `value.` and `key.`. The paths
//! follow the JSON of the decoder, so each path names a field of a decoded message.
//! A list gives `[*]` before the fields of its items. A map and a value without a fixed
//! set of names give no fields.

use std::collections::HashSet;

use apache_avro::Schema as AvroSchema;
use apache_avro::schema::{Name, NamesRef, ResolvedSchema};
use prost_reflect::{Kind, MessageDescriptor};
use serde_json::Value as Json;

use super::filter::path_name;
use super::registry::{Parsed, main_file};

/// The most steps after the root in one path.
const DEPTH_LIMIT: usize = 10;
/// The most paths of one root.
const PATH_LIMIT: usize = 2000;
/// The most schema nodes that one walk visits. A schema that uses the same type many
/// times can repeat paths without a new path, so the path limit alone does not stop it.
const STEP_LIMIT: usize = 100_000;

/// Returns the field paths of a schema below `root`, such as `value.customer.id`. Each
/// path that has fields below it also comes first as a path.
pub fn paths(root: &str, parsed: &Parsed) -> Vec<String> {
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
                return Vec::new();
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
    out.paths
}

#[derive(Default)]
struct Out {
    paths: Vec<String>,
    seen: HashSet<String>,
    steps: usize,
}

impl Out {
    /// Adds a path once. It returns false when the list is full.
    fn add(&mut self, path: &str) -> bool {
        if self.paths.len() >= PATH_LIMIT {
            return false;
        }
        if self.seen.insert(path.to_owned()) {
            self.paths.push(path.to_owned());
        }
        true
    }

    /// Counts one visited node. It returns false when the walk must stop.
    fn step(&mut self) -> bool {
        self.steps += 1;
        self.steps <= STEP_LIMIT && self.paths.len() < PATH_LIMIT
    }
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
                if !out.add(&child) {
                    break;
                }
                avro(out, names, &child, depth + 1, &item.schema, stack);
            }
            stack.pop();
        }
        _ => {}
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
        if !out.add(&child) {
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

    /// Adds the fields of a schema node. `allOf`, `anyOf`, `oneOf`, `then`, and `else`
    /// give the fields of each schema. `stack` holds the targets of the open `$ref`, so a
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
        if !out.step() || depth >= DEPTH_LIMIT {
            return;
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
        if let Some(properties) = object.get("properties").and_then(Json::as_object) {
            for (name, inner) in properties {
                let child = field(path, name);
                if !out.add(&child) {
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
    }
}

#[cfg(test)]
mod tests {
    use super::super::filter::parse_path;
    use super::super::registry::parse;
    use super::*;

    fn of(kind: &str, schema: &str, references: &[(String, String)]) -> Vec<String> {
        paths("value", &parse(kind, schema, references).unwrap())
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
