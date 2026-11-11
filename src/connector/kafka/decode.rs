//! Decodes a key or a value of a Kafka message. A payload of the Confluent wire format
//! starts with the byte 0 and a schema id of 4 bytes. The decoder never loses the bytes:
//! a failure keeps them and gives a notice.

use std::io::Cursor;

use prost_reflect::{DynamicMessage, MessageDescriptor};
use serde_json::Value as Json;

use super::registry::{Parsed, RegistryClient, main_file};
use crate::ui::text;

/// A decoded key or value.
pub enum Decoded {
    /// A tombstone, or a message without a key.
    Null,
    Text(String),
    Bytes(Vec<u8>),
    /// A payload of the registry, as JSON.
    Json {
        json: Json,
        text: String,
        id: u32,
        kind: &'static str,
    },
}

impl Decoded {
    /// Returns the text that the filter reads. Bytes have no text.
    pub fn text(&self) -> Option<&str> {
        match self {
            Decoded::Null | Decoded::Bytes(_) => None,
            Decoded::Text(text) => Some(text),
            Decoded::Json { text, .. } => Some(text),
        }
    }

    /// Returns the schema that decoded the payload, as `12 avro`.
    pub fn schema(&self) -> Option<String> {
        match self {
            Decoded::Json { id, kind, .. } => Some(format!("{id} {kind}")),
            _ => None,
        }
    }
}

/// Decodes bytes without the registry: UTF-8 text, or bytes.
fn plain(bytes: &[u8]) -> Decoded {
    match std::str::from_utf8(bytes) {
        Ok(text) => Decoded::Text(text.to_owned()),
        Err(_) => Decoded::Bytes(bytes.to_vec()),
    }
}

/// Reads a variable-length integer with the zigzag code, as Protobuf writes it.
fn zigzag(bytes: &[u8], pos: &mut usize) -> Option<i64> {
    let mut value: u64 = 0;
    for shift in (0..64).step_by(7) {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            let half = i64::try_from(value >> 1).ok()?;
            return Some(if value & 1 == 0 { half } else { -half - 1 });
        }
    }
    None
}

/// Finds the message of the index list of the Confluent Protobuf format. An empty list
/// means the first message of the file.
fn protobuf_message(
    pool: &prost_reflect::DescriptorPool,
    payload: &[u8],
) -> Option<(MessageDescriptor, usize)> {
    let mut pos = 0;
    let count = zigzag(payload, &mut pos)?;
    let indexes: Vec<i64> = if count == 0 {
        vec![0]
    } else {
        (0..count)
            .map(|_| zigzag(payload, &mut pos))
            .collect::<Option<_>>()?
    };
    let file = pool.get_file_by_name(main_file())?;
    let first = usize::try_from(*indexes.first()?).ok()?;
    let mut message = file.messages().nth(first)?;
    for index in &indexes[1..] {
        let child = message
            .child_messages()
            .nth(usize::try_from(*index).ok()?)?;
        message = child;
    }
    Some((message, pos))
}

/// Decodes the payload after the schema id.
fn with_schema(parsed: &Parsed, payload: &[u8]) -> Result<Json, String> {
    let bad = || text::KAFKA_DECODE_FAILED.to_owned();
    match parsed {
        Parsed::Avro { schema, references } => {
            let value = apache_avro::from_avro_datum_schemata(
                schema,
                references.iter().collect(),
                &mut Cursor::new(payload),
                None,
            )
            .map_err(|_| bad())?;
            Json::try_from(value).map_err(|_| bad())
        }
        Parsed::Json => serde_json::from_slice(payload).map_err(|_| bad()),
        Parsed::Protobuf { pool } => {
            let (message, start) = protobuf_message(pool, payload).ok_or_else(bad)?;
            let message = DynamicMessage::decode(message, &payload[start..]).map_err(|_| bad())?;
            serde_json::to_value(&message).map_err(|_| bad())
        }
    }
}

/// Decodes a key or a value. The second item is a notice for a payload that the
/// registry could not decode.
pub async fn decode(
    bytes: Option<&[u8]>,
    registry: Option<&RegistryClient>,
) -> (Decoded, Option<String>) {
    let Some(bytes) = bytes else {
        return (Decoded::Null, None);
    };
    let (Some(registry), [0, a, b, c, d, payload @ ..]) = (registry, bytes) else {
        return (plain(bytes), None);
    };
    let id = u32::from_be_bytes([*a, *b, *c, *d]);
    let decoded = match registry.schema(id).await {
        Ok(parsed) => with_schema(&parsed, payload).map(|json| (json, parsed.kind())),
        Err(message) => Err(message),
    };
    match decoded {
        Ok((json, kind)) => {
            let text = json.to_string();
            (
                Decoded::Json {
                    json,
                    text,
                    id,
                    kind,
                },
                None,
            )
        }
        // A framed payload is binary, so it stays bytes.
        Err(message) => (
            Decoded::Bytes(bytes.to_vec()),
            Some(text::kafka_decode_notice(id, &message)),
        ),
    }
}

/// Decodes a payload with a known schema, for the tests of the wire format.
pub fn decode_with(parsed: &Parsed, bytes: &[u8]) -> Result<Json, String> {
    match bytes {
        [0, _, _, _, _, payload @ ..] => with_schema(parsed, payload),
        _ => Err(text::KAFKA_DECODE_FAILED.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::registry::parse;
    use super::*;

    #[test]
    fn zigzag_reads_negative_and_positive_numbers() {
        let mut pos = 0;
        assert_eq!(zigzag(&[0x00], &mut pos), Some(0));
        pos = 0;
        assert_eq!(zigzag(&[0x01], &mut pos), Some(-1));
        pos = 0;
        assert_eq!(zigzag(&[0x02], &mut pos), Some(1));
        pos = 0;
        assert_eq!(zigzag(&[0xac, 0x02], &mut pos), Some(150));
        assert_eq!(pos, 2);
    }

    #[test]
    fn avro_decodes_a_record_to_json() {
        let schema = r#"{"type":"record","name":"Order","fields":[
            {"name":"id","type":"long"},{"name":"name","type":"string"}]}"#;
        let parsed = parse("AVRO", schema, &[]).unwrap();
        // id 7 is the zigzag 14; the text has 3 bytes.
        let bytes = [0, 0, 0, 0, 1, 14, 6, b'A', b'n', b'a'];
        let json = decode_with(&parsed, &bytes).unwrap();
        assert_eq!(json, serde_json::json!({"id": 7, "name": "Ana"}));
    }

    #[test]
    fn avro_uses_the_schemas_of_the_references() {
        let address =
            r#"{"type":"record","name":"Address","fields":[{"name":"city","type":"string"}]}"#;
        let person =
            r#"{"type":"record","name":"Person","fields":[{"name":"home","type":"Address"}]}"#;
        let parsed = parse("AVRO", person, &[("Address".into(), address.into())]).unwrap();
        let bytes = [0, 0, 0, 0, 2, 4, b'R', b'J'];
        let json = decode_with(&parsed, &bytes).unwrap();
        assert_eq!(json, serde_json::json!({"home": {"city": "RJ"}}));
    }

    #[test]
    fn protobuf_reads_the_index_list_and_nested_messages() {
        let schema = r#"syntax = "proto3";
            message Order { int64 id = 1; message Line { string sku = 1; } }
            message Other { bool flag = 1; }"#;
        let parsed = parse("PROTOBUF", schema, &[]).unwrap();
        // An empty index list names the first message. Field 1, varint 7.
        let json = decode_with(&parsed, &[0, 0, 0, 0, 3, 0, 0x08, 7]).unwrap();
        assert_eq!(json, serde_json::json!({"id": "7"}));
        // The list [0, 0] names `Order.Line`. Field 1, text "ab".
        let line = [0, 0, 0, 0, 3, 4, 0, 0, 0x0a, 2, b'a', b'b'];
        assert_eq!(
            decode_with(&parsed, &line).unwrap(),
            serde_json::json!({"sku": "ab"})
        );
        // The list [1] names `Other`.
        let other = [0, 0, 0, 0, 3, 2, 2, 0x08, 1];
        assert_eq!(
            decode_with(&parsed, &other).unwrap(),
            serde_json::json!({"flag": true})
        );
    }

    #[test]
    fn protobuf_imports_a_reference_and_a_google_type() {
        let money = r#"syntax = "proto3"; package shop; message Money { int64 cents = 1; }"#;
        let schema = r#"syntax = "proto3"; package shop;
            import "money.proto"; import "google/protobuf/timestamp.proto";
            message Sale { Money price = 1; google.protobuf.Timestamp at = 2; }"#;
        let parsed = parse("PROTOBUF", schema, &[("money.proto".into(), money.into())]).unwrap();
        let json = decode_with(&parsed, &[0, 0, 0, 0, 4, 0, 0x0a, 2, 0x08, 5]).unwrap();
        assert_eq!(json, serde_json::json!({"price": {"cents": "5"}}));
    }

    #[test]
    fn json_schema_payloads_are_json_after_the_header() {
        let parsed = parse("JSON", "{}", &[]).unwrap();
        let json = decode_with(&parsed, b"\0\0\0\0\x05{\"a\":1}").unwrap();
        assert_eq!(json, serde_json::json!({"a": 1}));
    }

    #[tokio::test]
    async fn without_a_registry_bytes_stay_text_or_bytes() {
        let (text, notice) = decode(Some(b"hello"), None).await;
        assert_eq!(text.text(), Some("hello"));
        assert!(notice.is_none());
        let (bytes, _) = decode(Some(&[0, 0, 0, 0, 1, 0xff]), None).await;
        assert!(matches!(bytes, Decoded::Bytes(ref data) if data == &[0, 0, 0, 0, 1, 0xff]));
        let (null, _) = decode(None, None).await;
        assert!(matches!(null, Decoded::Null));
    }
}
