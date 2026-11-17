use super::*;
use crossterm::event::KeyCode;
use sqln::catalog::{ColumnNote, RelationRef};
use sqln::complete::SuggestionKind;
use sqln::editor::KeyboardMode;
use sqln::table::{TableFocus, TableTab};

/// The field paths of each topic, with the type words and the values of each one.
const ORDER_FIELDS: [(&str, &str, &[&str]); 12] = [
    ("value.id", "long", &[]),
    ("value.customer", "null | record", &[]),
    ("value.customer.name", "string", &[]),
    ("value.lines", "array", &[]),
    ("value.lines[*].sku", "string", &[]),
    ("value.lines[*].state", "enum", &["'NEW'", "'DONE'"]),
    ("value.lines[*].parts", "array", &[]),
    ("value.lines[*].parts[*].code", "string", &[]),
    ("value.\"order id\"", "string", &[]),
    (
        "value.state",
        "enum",
        &["'LOW'", "'MEDIUM'", "'HIGH'", r"'it\'s'"],
    ),
    ("value.flags", "null | enum", &["'A'"]),
    ("key.id", "int64", &[]),
];

/// The catalog of the other tests, with the field paths of each topic.
fn loaded() -> Catalog {
    let mut catalog = catalog();
    for relation in &mut catalog.schemas[0].relations {
        relation.columns = Columns::Loaded(ORDER_FIELDS.map(|(path, ..)| path.to_owned()).to_vec());
        relation.notes = ORDER_FIELDS
            .map(|(_, detail, values)| ColumnNote {
                detail: detail.to_owned(),
                values: values.iter().map(|value| (*value).to_owned()).collect(),
            })
            .to_vec();
    }
    catalog
}

fn suggest_at(text: &str, cursor: usize, catalog: &Catalog) -> sqln::complete::Completion {
    suggest(text, cursor, Dialect::Kafka, Some(catalog))
}

fn names(text: &str) -> Vec<(String, String)> {
    suggest_at(text, text.len(), &loaded())
        .items
        .into_iter()
        .map(|item| (item.label, item.insert))
        .collect()
}

fn labels_of(text: &str) -> Vec<String> {
    names(text).into_iter().map(|item| item.0).collect()
}

#[test]
fn a_select_path_suggests_the_next_field_of_the_topic() {
    let text = "SELECT * FROM orders WHERE value.cu";
    let found = suggest_at(text, text.len(), &loaded());
    assert_eq!(&text[found.replace], "cu", "only the typed name changes");
    assert_eq!(labels_of(text), ["customer"]);
    assert_eq!(
        labels_of("SELECT * FROM orders WHERE value.customer."),
        ["name"]
    );
    assert_eq!(labels_of("SELECT * FROM orders WHERE KEY."), ["id"]);
}

#[test]
fn a_path_before_from_reads_the_topic_after_the_cursor() {
    let text = "SELECT key, value. FROM orders";
    let cursor = text.find(" FROM").unwrap();
    let found: Vec<String> = suggest_at(text, cursor, &loaded())
        .items
        .into_iter()
        .map(|item| item.label)
        .collect();
    for name in ["id", "customer", "lines", "order id"] {
        assert!(found.contains(&name.to_owned()), "{name}: {found:?}");
    }
    assert!(!found.contains(&"name".to_owned()), "a nested field waits");
}

#[test]
fn a_name_with_a_space_gets_quotes() {
    let found = names("SELECT * FROM orders WHERE value.ord");
    assert_eq!(found, [("order id".into(), "\"order id\"".into())]);
}

#[test]
fn a_list_item_accepts_each_index_form() {
    for text in [
        "CONSUME orders WHERE value.lines[*].",
        "CONSUME orders WHERE value.lines[0].",
        "TAIL orders WHERE value.lines.0.",
        "SELECT * FROM orders WHERE value.lines[-1].",
    ] {
        assert_eq!(labels_of(text), ["parts", "sku", "state"], "{text}");
    }
    assert!(labels_of("CONSUME orders WHERE value.lines.").is_empty());
}

#[test]
fn a_quoted_topic_name_finds_its_fields() {
    assert_eq!(
        labels_of("SELECT * FROM \"order events\" WHERE key."),
        ["id"]
    );
    assert_eq!(labels_of("CONSUME \"order events\" WHERE key."), ["id"]);
}

#[test]
fn a_topic_without_loaded_fields_asks_for_them() {
    let wanted = RelationRef {
        schema: TOPICS.to_owned(),
        relation: "orders".to_owned(),
    };
    for text in [
        "SELECT * FROM orders WHERE value.",
        "CONSUME orders WHERE value.cu",
    ] {
        let found = suggest_at(text, text.len(), &catalog());
        assert!(found.items.is_empty(), "{text}");
        assert_eq!(
            found.missing_columns,
            std::slice::from_ref(&wanted),
            "{text}"
        );
    }
}

#[test]
fn other_roots_and_unknown_topics_get_no_fields() {
    for text in [
        "SELECT * FROM orders WHERE headers.",
        "SELECT * FROM unknown WHERE value.",
        "SELECT value.",
        "CONSUME unknown WHERE value.",
    ] {
        let found = suggest_at(text, text.len(), &catalog());
        assert!(found.items.is_empty(), "{text}: {:?}", found.items);
        assert!(found.missing_columns.is_empty(), "{text}");
    }
}

#[test]
fn a_kafka_console_waits_for_the_fields_then_lists_them() {
    let mut f = crate::common::Fixture::with_dialect(
        "",
        sqln::editor::KeyboardMode::Conventional,
        Dialect::Kafka,
    );
    f.catalog = catalog();
    f.write("CONSUME orders WHERE value.");
    assert!(f.editor.suggestions().is_none());
    assert_eq!(f.editor.waiting(), Some("orders"));
    let wanted = RelationRef {
        schema: TOPICS.to_owned(),
        relation: "orders".to_owned(),
    };
    assert_eq!(f.editor.take_missing_columns(), [wanted]);
    f.catalog = loaded();
    f.editor.refresh_suggestions(Some(&f.catalog));
    assert_eq!(
        f.labels(),
        ["customer", "flags", "id", "lines", "order id", "state"]
    );
    assert_eq!(f.editor.waiting(), None);
}

/// Returns the label and the kind of each item, with the cursor at the end.
fn kinds(text: &str) -> Vec<(String, SuggestionKind)> {
    suggest_at(text, text.len(), &loaded())
        .items
        .into_iter()
        .map(|item| (item.label, item.kind))
        .collect()
}

fn values_of(text: &str) -> Vec<String> {
    kinds(text)
        .into_iter()
        .filter(|item| item.1 == SuggestionKind::Value)
        .map(|item| item.0)
        .collect()
}

#[test]
fn a_field_shows_the_type_words_of_its_schema() {
    let found = suggest_at("SELECT * FROM orders WHERE value.cu", 35, &loaded());
    let item = &found.items[0];
    assert_eq!(item.label, "customer");
    assert_eq!(item.detail.as_deref(), Some("null | record"));
    let found = suggest_at("CONSUME orders WHERE key.", 25, &loaded());
    assert_eq!(found.items[0].detail.as_deref(), Some("int64"));
}

#[test]
fn a_compared_enum_field_offers_its_values_in_schema_order_first() {
    let all = ["'LOW'", "'MEDIUM'", "'HIGH'", r"'it\'s'"];
    for text in [
        "SELECT * FROM orders WHERE value.state = ",
        "SELECT * FROM orders WHERE value.state != ",
        "SELECT * FROM orders WHERE value.state IN (",
        "SELECT * FROM orders WHERE value.state NOT IN (",
        "CONSUME orders WHERE value.state = ",
        "TAIL orders WHERE value.state IN ('LOW', ",
        "CONSUME orders WHERE value.state=",
    ] {
        let found = kinds(text);
        let values: Vec<&str> = found
            .iter()
            .take_while(|item| item.1 == SuggestionKind::Value)
            .map(|item| item.0.as_str())
            .collect();
        assert_eq!(values, all, "{text}: {found:?}");
    }
    assert_eq!(
        values_of("SELECT * FROM orders WHERE value.flags = "),
        ["'A'"],
        "a union keeps the values of its enum"
    );
    assert!(values_of("SELECT * FROM orders WHERE value.id = ").is_empty());
    assert!(values_of("SELECT * FROM orders WHERE value.state > ").is_empty());
}

#[test]
fn a_typed_value_narrows_the_list_with_or_without_its_quote() {
    assert_eq!(
        values_of("CONSUME orders WHERE value.state = M"),
        ["'MEDIUM'"]
    );
    let text = "SELECT * FROM orders WHERE value.state = 'h";
    let found = suggest_at(text, text.len(), &loaded());
    assert_eq!(
        &text[found.replace.clone()],
        "'h",
        "the quote is replaced too"
    );
    let items: Vec<(String, String)> = found
        .items
        .into_iter()
        .map(|item| (item.label, item.insert))
        .collect();
    assert_eq!(items, [("'HIGH'".to_owned(), "'HIGH'".to_owned())]);
    assert_eq!(
        names("CONSUME orders WHERE value.state = 'it"),
        [(r"'it\'s'".to_owned(), r"'it\'s'".to_owned())]
    );
    assert!(names("CONSUME orders WHERE value.state = 'x").is_empty());
}

#[test]
fn a_compared_field_without_loaded_columns_asks_for_them() {
    let wanted = RelationRef {
        schema: TOPICS.to_owned(),
        relation: "orders".to_owned(),
    };
    let text = "CONSUME orders WHERE value.state = '";
    let found = suggest_at(text, text.len(), &catalog());
    assert!(found.items.is_empty());
    assert_eq!(found.missing_columns, [wanted]);
}

#[test]
fn an_open_item_of_any_or_every_gets_the_fields_of_its_list() {
    for text in [
        "SELECT * FROM orders WHERE ANY i IN value.lines SATISFIES i.",
        "CONSUME orders WHERE EVERY line IN value.lines SATISFIES LINE.",
        "SELECT * FROM orders WHERE ANY i IN value.lines SATISFIES i.sku = 'a' AND i.",
    ] {
        assert_eq!(labels_of(text), ["parts", "sku", "state"], "{text}");
    }
    assert_eq!(
        labels_of(
            "SELECT * FROM orders WHERE ANY i IN value.lines SATISFIES ANY p IN i.parts SATISFIES p."
        ),
        ["code"]
    );
    assert_eq!(
        values_of("CONSUME orders WHERE ANY i IN value.lines SATISFIES i.state = "),
        ["'NEW'", "'DONE'"]
    );
    assert!(
        labels_of(
            "SELECT * FROM orders WHERE ANY i IN value.lines SATISFIES i.sku = 'a' END AND i."
        )
        .is_empty(),
        "END closes the item"
    );
}

#[test]
fn an_accepted_value_writes_its_literal_in_both_editor_modes() {
    for keyboard in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = crate::common::Fixture::with_dialect("", keyboard, Dialect::Kafka);
        f.catalog = loaded();
        if keyboard == KeyboardMode::Vim {
            f.write("i");
        }
        f.write("CONSUME orders WHERE value.state = 'M");
        assert_eq!(f.labels(), ["'MEDIUM'"]);
        f.press(crate::common::key(KeyCode::Tab));
        assert_eq!(f.text(), "CONSUME orders WHERE value.state = 'MEDIUM'");
    }
}

#[test]
fn the_filter_of_a_topic_tab_waits_with_the_kafka_dialect() {
    let relation = RelationRef {
        schema: TOPICS.to_owned(),
        relation: "orders".to_owned(),
    };
    let mut tab = TableTab::new(7, 1, relation.clone(), false, Dialect::Kafka, 100);
    tab.filter_only = true;
    tab.focus = TableFocus::Filter;
    let mut missing = Vec::new();
    for c in "value.".chars() {
        missing = tab.on_field_key(
            crate::common::key(KeyCode::Char(c)),
            Dialect::Kafka,
            Some(&catalog()),
        );
    }
    assert_eq!(missing, [relation]);
    assert_eq!(tab.waiting(), Some(Dialect::Kafka));
    tab.refresh_list(Dialect::Kafka, Some(&loaded()));
    assert_eq!(tab.waiting(), None);
    assert!(tab.list.is_some());
}
