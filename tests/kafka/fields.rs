use super::*;
use sqln::catalog::RelationRef;

const ORDER_PATHS: [&str; 7] = [
    "value.id",
    "value.customer",
    "value.customer.name",
    "value.lines",
    "value.lines[*].sku",
    "value.\"order id\"",
    "key.id",
];

/// The catalog of the other tests, with the field paths of each topic.
fn loaded() -> Catalog {
    let mut catalog = catalog();
    for relation in &mut catalog.schemas[0].relations {
        relation.columns = Columns::Loaded(ORDER_PATHS.map(str::to_owned).to_vec());
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
        assert_eq!(labels_of(text), ["sku"], "{text}");
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
    let mut f =
        common::Fixture::with_dialect("", sqln::editor::KeyboardMode::Conventional, Dialect::Kafka);
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
    assert_eq!(f.labels(), ["customer", "id", "lines", "order id"]);
    assert_eq!(f.editor.waiting(), None);
}
