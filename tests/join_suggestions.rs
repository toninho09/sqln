// The condition of a foreign key in the suggestions after `ON`.

use sqln::catalog::{
    Catalog, Columns, ForeignKey, KeyColumn, Relation, RelationKind, RelationRef, Schema,
    group_foreign_keys,
};
use sqln::complete::{SuggestionKind, suggest};
use sqln::sql::Dialect;

fn key(columns: &[&str], schema: &str, target: &str, target_columns: &[&str]) -> ForeignKey {
    let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect();
    ForeignKey {
        columns: names(columns),
        target: RelationRef {
            schema: schema.to_owned(),
            relation: target.to_owned(),
        },
        target_columns: names(target_columns),
    }
}

fn table(name: &str, columns: &[&str], foreign_keys: Vec<ForeignKey>) -> Relation {
    Relation {
        name: name.to_owned(),
        kind: RelationKind::Table,
        columns: Columns::Loaded(columns.iter().map(|column| (*column).to_owned()).collect()),
        foreign_keys,
    }
}

fn catalog() -> Catalog {
    Catalog {
        schemas: vec![
            Schema {
                name: "public".to_owned(),
                relations: vec![
                    table("customers", &["id", "name"], Vec::new()),
                    table(
                        "orders",
                        &["id", "customer_id", "region", "code"],
                        vec![key(&["customer_id"], "public", "customers", &["id"])],
                    ),
                    table(
                        "items",
                        &["order_region", "order_code"],
                        vec![key(
                            &["order_region", "order_code"],
                            "public",
                            "orders",
                            &["region", "code"],
                        )],
                    ),
                    table(
                        "Order Lines",
                        &["order"],
                        vec![key(&["order"], "public", "orders", &["id"])],
                    ),
                    table(
                        "employees",
                        &["id", "manager_id"],
                        vec![key(&["manager_id"], "public", "employees", &["id"])],
                    ),
                    Relation {
                        name: "notes".to_owned(),
                        kind: RelationKind::Table,
                        columns: Columns::NotLoaded,
                        foreign_keys: Vec::new(),
                    },
                ],
                partial: false,
            },
            Schema {
                name: "audit".to_owned(),
                relations: vec![
                    table(
                        "logs",
                        &["customer_id"],
                        vec![key(&["customer_id"], "public", "customers", &["id"])],
                    ),
                    table("customers", &["id"], Vec::new()),
                ],
                partial: false,
            },
        ],
        current_schema: Some("public".to_owned()),
    }
}

fn conditions(text: &str) -> Vec<String> {
    let completion = suggest(text, text.len(), Dialect::Postgres, Some(&catalog()));
    completion
        .items
        .iter()
        .filter(|item| item.kind == SuggestionKind::Join)
        .map(|item| item.insert.clone())
        .collect()
}

#[test]
fn the_key_of_the_new_table_gives_a_condition_with_the_aliases() {
    assert_eq!(
        conditions("SELECT * FROM customers c JOIN orders o ON "),
        ["o.customer_id = c.id"]
    );
}

#[test]
fn the_key_of_an_earlier_table_also_gives_a_condition() {
    assert_eq!(
        conditions("SELECT * FROM orders o JOIN customers c ON "),
        ["o.customer_id = c.id"]
    );
}

#[test]
fn a_table_without_an_alias_uses_its_name() {
    assert_eq!(
        conditions("SELECT * FROM orders JOIN customers ON "),
        ["orders.customer_id = customers.id"]
    );
}

#[test]
fn a_composite_key_joins_its_pairs_in_one_suggestion() {
    assert_eq!(
        conditions("SELECT * FROM orders o JOIN items i ON "),
        ["i.order_region = o.region AND i.order_code = o.code"]
    );
}

#[test]
fn a_name_gets_the_quotes_that_it_needs() {
    assert_eq!(
        conditions("SELECT * FROM orders o JOIN \"Order Lines\" ON "),
        ["\"Order Lines\".\"order\" = o.id"]
    );
}

#[test]
fn a_self_join_gives_both_directions() {
    assert_eq!(
        conditions("SELECT * FROM employees e JOIN employees m ON "),
        ["e.manager_id = m.id", "m.manager_id = e.id"]
    );
}

#[test]
fn the_third_table_reads_each_earlier_table() {
    assert_eq!(
        conditions(
            "SELECT * FROM customers c JOIN orders o ON o.customer_id = c.id JOIN items i ON "
        ),
        ["i.order_region = o.region AND i.order_code = o.code"]
    );
}

#[test]
fn a_key_of_another_schema_needs_the_table_of_that_schema() {
    assert_eq!(
        conditions("SELECT * FROM audit.logs l JOIN customers c ON "),
        ["l.customer_id = c.id"]
    );
    assert_eq!(
        conditions("SELECT * FROM audit.logs l JOIN audit.customers c ON "),
        Vec::<String>::new()
    );
}

#[test]
fn a_partial_word_filters_the_conditions() {
    assert_eq!(
        conditions("SELECT * FROM customers c JOIN orders o ON o"),
        ["o.customer_id = c.id"]
    );
    assert_eq!(
        conditions("SELECT * FROM customers c JOIN orders o ON zz"),
        Vec::<String>::new()
    );
}

#[test]
fn a_condition_leads_the_list_and_the_columns_stay() {
    let text = "SELECT * FROM customers c JOIN orders o ON ";
    let completion = suggest(text, text.len(), Dialect::Postgres, Some(&catalog()));
    assert_eq!(completion.items[0].kind, SuggestionKind::Join);
    assert!(
        completion
            .items
            .iter()
            .any(|item| item.kind == SuggestionKind::Column)
    );
}

#[test]
fn only_the_word_on_opens_the_conditions() {
    for text in [
        "SELECT * FROM customers c JOIN orders o ON o.customer_id = c.id AND ",
        "SELECT * FROM customers c JOIN orders o WHERE ",
        "SELECT * FROM customers c JOIN orders o ON o.",
        "CREATE INDEX i ON ",
        "SELECT * FROM customers ON ",
        "MERGE INTO orders o USING customers c ON ",
    ] {
        assert_eq!(conditions(text), Vec::<String>::new(), "{text}");
    }
}

#[test]
fn a_name_of_with_is_not_a_table_of_the_catalog() {
    let text = "WITH orders AS (SELECT 1 AS id) SELECT * FROM customers c JOIN orders o ON ";
    assert_eq!(conditions(text), Vec::<String>::new());
}

#[test]
fn a_subquery_has_no_conditions() {
    let text = "SELECT * FROM customers c JOIN (SELECT 1 AS id) o ON ";
    assert_eq!(conditions(text), Vec::<String>::new());
}

#[test]
fn a_table_without_loaded_columns_asks_for_them() {
    let text = "SELECT * FROM customers c JOIN notes n ON ";
    let completion = suggest(text, text.len(), Dialect::Postgres, Some(&catalog()));
    let missing: Vec<&str> = completion
        .missing_columns
        .iter()
        .map(|item| item.relation.as_str())
        .collect();
    assert_eq!(missing, ["notes"]);
}

#[test]
fn the_pairs_of_a_query_become_keys_in_their_order() {
    let pair = |key: &str, column: &str, target: &str, target_column: &str| KeyColumn {
        key: key.to_owned(),
        target: RelationRef {
            schema: "s".to_owned(),
            relation: target.to_owned(),
        },
        column: column.to_owned(),
        target_column: target_column.to_owned(),
    };
    let keys = group_foreign_keys([
        pair("a", "x", "t", "1"),
        pair("a", "y", "t", "2"),
        pair("b", "z", "t", "3"),
        pair("c", "w", "u", "4"),
    ]);
    assert_eq!(
        keys,
        [
            key(&["x", "y"], "s", "t", &["1", "2"]),
            key(&["z"], "s", "t", &["3"]),
            key(&["w"], "s", "u", &["4"]),
        ]
    );
}
