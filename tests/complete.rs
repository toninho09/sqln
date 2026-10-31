use sqln::catalog::{Catalog, Columns, Relation, RelationKind, RelationRef, Schema};
use sqln::complete::{Completion, SuggestionKind, suggest};
use sqln::sql::Dialect;

fn relation(name: &str, kind: RelationKind, columns: &[&str]) -> Relation {
    Relation {
        name: name.to_owned(),
        kind,
        columns: Columns::Loaded(columns.iter().map(|column| (*column).to_owned()).collect()),
    }
}

fn catalog() -> Catalog {
    Catalog {
        schemas: vec![
            Schema {
                name: "public".to_owned(),
                relations: vec![
                    relation("users", RelationKind::Table, &["id", "name", "Email Address"]),
                    relation("orders", RelationKind::Table, &["id", "user_id", "total"]),
                    relation("active_users", RelationKind::View, &["id", "name"]),
                    relation("Order Items", RelationKind::Table, &["sku", "order"]),
                ],
            },
            Schema {
                name: "sales".to_owned(),
                relations: vec![relation("invoices", RelationKind::Table, &["number", "due"])],
            },
        ],
        current_schema: Some("public".to_owned()),
    }
}

/// `|` marks the cursor.
fn run_with(text: &str, dialect: Dialect, catalog: Option<&Catalog>) -> Completion {
    let cursor = text.find('|').expect("the text marks the cursor");
    let sql = text.replacen('|', "", 1);
    suggest(&sql, cursor, dialect, catalog)
}

fn labels_with(text: &str, dialect: Dialect, catalog: Option<&Catalog>) -> Vec<String> {
    run_with(text, dialect, catalog).items.into_iter().map(|item| item.label).collect()
}

fn labels(text: &str) -> Vec<String> {
    labels_with(text, Dialect::Postgres, Some(&catalog()))
}

fn apply(text: &str, dialect: Dialect, catalog: Option<&Catalog>, label: &str) -> String {
    let cursor = text.find('|').unwrap();
    let mut sql = text.replacen('|', "", 1);
    let completion = suggest(&sql, cursor, dialect, catalog);
    let item = completion.items.iter().find(|item| item.label == label).expect(label);
    sql.replace_range(completion.replace.clone(), &item.insert);
    sql
}

#[test]
fn the_plan_example_suggests_columns_after_an_alias() {
    assert_eq!(labels("SELECT u.|\nFROM users AS u;"), ["Email Address", "id", "name"]);
}

#[test]
fn alias_without_as_and_partial_column() {
    assert_eq!(labels("SELECT o.to| FROM orders o"), ["total"]);
}

#[test]
fn expression_lists_columns_and_aliases_before_keywords() {
    let found = run_with("SELECT | FROM users u JOIN orders o ON true", Dialect::Postgres, Some(&catalog()));
    let kinds: Vec<_> = found.items.iter().map(|item| item.kind).collect();
    let first_keyword = kinds.iter().position(|kind| *kind == SuggestionKind::Keyword).unwrap();
    assert!(first_keyword > 0);
    assert!(kinds[first_keyword..].iter().all(|kind| *kind == SuggestionKind::Keyword));
    let names: Vec<_> = found.items.iter().map(|item| item.label.as_str()).collect();
    assert!(names.contains(&"user_id") && names.contains(&"name"));
    assert!(names.contains(&"u") && names.contains(&"o"));
}

#[test]
fn relation_position_lists_tables_views_and_schemas() {
    let found = labels("SELECT * FROM |");
    for name in ["users", "orders", "active_users", "public", "sales", "sales.invoices"] {
        assert!(found.iter().any(|label| label == name), "{name} missing in {found:?}");
    }
    assert!(!found.iter().any(|label| label == "SELECT"));
}

#[test]
fn second_relation_after_a_comma_and_after_join() {
    assert!(labels("SELECT * FROM users, |").contains(&"orders".to_owned()));
    assert!(labels("SELECT * FROM users u JOIN or|").contains(&"orders".to_owned()));
    assert!(labels("UPDATE us|").contains(&"users".to_owned()));
    assert!(labels("INSERT INTO us|").contains(&"users".to_owned()));
    assert!(labels("DELETE FROM us|").contains(&"users".to_owned()));
}

#[test]
fn schema_qualifier_lists_its_relations_only() {
    assert_eq!(labels("SELECT * FROM sales.|"), ["invoices"]);
    assert_eq!(labels("SELECT * FROM sales.inv|"), ["invoices"]);
}

#[test]
fn table_and_schema_qualifiers_list_columns() {
    assert_eq!(labels("SELECT users.na| FROM users"), ["name"]);
    assert_eq!(labels("SELECT public.users.na| FROM public.users"), ["name"]);
    assert_eq!(labels("SELECT sales.invoices.| FROM sales.invoices"), ["due", "number"]);
    assert_eq!(labels("SELECT orders.to|"), ["total"], "a relation out of FROM still resolves");
}

#[test]
fn aliases_of_a_schema_relation_resolve_in_that_schema() {
    assert_eq!(labels("SELECT i.| FROM sales.invoices AS i"), ["due", "number"]);
}

#[test]
fn keywords_follow_an_operand() {
    assert_eq!(labels("SELECT id fr|"), ["from"]);
    assert_eq!(labels("SELECT id fr| users"), ["from"]);
    assert_eq!(labels("SELECT * fr|"), ["from"]);
    let found = labels("SELECT * FROM users wh|");
    assert!(found.contains(&"when".to_owned()) && found.contains(&"where".to_owned()), "{found:?}");
    assert!(!found.contains(&"wh".to_owned()), "the partial word is not an alias");
}

#[test]
fn keyword_case_follows_the_typed_text() {
    assert_eq!(labels("sel|"), ["select"]);
    assert_eq!(labels("SEL|"), ["SELECT"]);
    assert_eq!(labels("Sel|"), ["SELECT"]);
}

#[test]
fn statement_start_lists_only_statement_words() {
    let found = labels("|");
    assert!(found.contains(&"SELECT".to_owned()));
    assert!(!found.contains(&"FROM".to_owned()));
    assert!(labels("SELECT 1;\n|").contains(&"INSERT".to_owned()));
    assert!(labels("EXPLAIN |").contains(&"SELECT".to_owned()));
}

#[test]
fn each_dialect_has_its_own_keywords() {
    let with = |text, dialect| labels_with(text, dialect, None);
    assert_eq!(with("prag|", Dialect::Sqlite), ["pragma"]);
    assert!(with("prag|", Dialect::Postgres).is_empty());
    assert_eq!(with("SELECT 1 ILI|", Dialect::Postgres), ["ILIKE"]);
    assert!(with("SELECT 1 ILI|", Dialect::MySql).is_empty());
    assert_eq!(with("SELECT 1 STRAIGHT|", Dialect::MySql), ["STRAIGHT_JOIN"]);
}

#[test]
fn without_a_catalog_keywords_and_aliases_remain() {
    let found = labels_with("SELECT | FROM users u", Dialect::Postgres, None);
    assert!(found.contains(&"u".to_owned()));
    assert!(found.contains(&"CASE".to_owned()));
    assert!(labels_with("SELECT * FROM |", Dialect::Postgres, None).is_empty());
    assert!(labels_with("SELECT u.| FROM users u", Dialect::Postgres, None).is_empty());
}

#[test]
fn names_get_the_quotes_of_the_dialect() {
    let catalog = catalog();
    assert_eq!(
        apply("SELECT * FROM Ord|", Dialect::Postgres, Some(&catalog), "Order Items"),
        "SELECT * FROM \"Order Items\""
    );
    assert_eq!(
        apply("SELECT * FROM Ord|", Dialect::MySql, Some(&catalog), "Order Items"),
        "SELECT * FROM `Order Items`"
    );
    assert_eq!(
        apply("SELECT * FROM Ord|", Dialect::Sqlite, Some(&catalog), "Order Items"),
        "SELECT * FROM \"Order Items\""
    );
}

#[test]
fn reserved_column_gets_quotes() {
    let catalog = catalog();
    assert_eq!(
        apply("SELECT i.ord| FROM \"Order Items\" i", Dialect::Postgres, Some(&catalog), "order"),
        "SELECT i.\"order\" FROM \"Order Items\" i"
    );
}

#[test]
fn opening_quote_stays_in_the_inserted_name() {
    let catalog = catalog();
    assert_eq!(
        apply("SELECT * FROM \"us|", Dialect::Postgres, Some(&catalog), "users"),
        "SELECT * FROM \"users\""
    );
    assert_eq!(
        apply("SELECT * FROM `us|", Dialect::MySql, Some(&catalog), "users"),
        "SELECT * FROM `users`"
    );
    assert_eq!(
        apply("SELECT * FROM [us|", Dialect::Sqlite, Some(&catalog), "users"),
        "SELECT * FROM [users]"
    );
}

#[test]
fn quoted_alias_and_quoted_qualifier_resolve() {
    assert_eq!(labels("SELECT \"u\".na| FROM users \"u\""), ["name"]);
    assert_eq!(labels("SELECT \"public\".\"users\".na| FROM public.users"), ["name"]);
    assert_eq!(labels("SELECT x.sk| FROM \"Order Items\" AS x"), ["sku"]);
}

#[test]
fn accepted_name_replaces_the_whole_word_under_the_cursor() {
    let catalog = catalog();
    assert_eq!(
        apply("SELECT * FROM us|ers_old", Dialect::Postgres, Some(&catalog), "users"),
        "SELECT * FROM users"
    );
}

#[test]
fn comments_strings_and_numbers_have_no_suggestions() {
    for text in [
        "-- SEL|",
        "SELECT 1 /* us|",
        "SELECT 'us|",
        "SELECT 'us|ers'",
        "SELECT 12|",
        "SELECT 1.|",
    ] {
        assert!(labels(text).is_empty(), "{text}");
    }
}

#[test]
fn text_after_a_closed_literal_has_suggestions() {
    assert_eq!(labels("/* a */ SEL|"), ["SELECT"]);
    assert_eq!(labels("-- a\nSEL|"), ["SELECT"]);
    assert_eq!(labels("SELECT 'a' fr|"), ["from"]);
}

#[test]
fn mysql_double_quote_is_a_string() {
    assert!(labels_with("SELECT \"us|", Dialect::MySql, Some(&catalog())).is_empty());
}

#[test]
fn subquery_sees_outer_references_but_not_sibling_subqueries() {
    let found = labels("SELECT (SELECT | FROM orders o) FROM users u");
    assert!(found.contains(&"o".to_owned()) && found.contains(&"u".to_owned()), "{found:?}");
    let found = labels("SELECT (SELECT 1 FROM orders o), | FROM users u");
    assert!(found.contains(&"u".to_owned()), "{found:?}");
    assert!(!found.contains(&"o".to_owned()), "{found:?}");
}

#[test]
fn derived_table_alias_is_visible_without_columns() {
    let found = labels("SELECT | FROM (SELECT id FROM orders) AS recent, users u");
    assert!(found.contains(&"recent".to_owned()), "{found:?}");
    assert!(found.contains(&"u".to_owned()), "{found:?}");
    assert!(labels("SELECT recent.| FROM (SELECT id FROM orders) recent").is_empty());
}

#[test]
fn insert_column_list_uses_the_target_relation() {
    assert_eq!(labels("INSERT INTO orders (|"), ["id", "total", "user_id"]);
    assert_eq!(labels("INSERT INTO sales.invoices (number, d|"), ["due"]);
}

#[test]
fn update_and_delete_expressions_use_the_target() {
    assert!(labels("UPDATE users SET na|").contains(&"name".to_owned()));
    assert!(labels("DELETE FROM users WHERE na|").contains(&"name".to_owned()));
}

#[test]
fn new_names_have_no_suggestions() {
    assert!(labels("SELECT id AS |").is_empty());
    assert!(labels("CREATE TABLE |").is_empty());
}

#[test]
fn only_the_statement_at_the_cursor_counts() {
    let found = labels("SELECT * FROM users u;\nSELECT | FROM orders o;\nSELECT * FROM sales.invoices i;");
    assert!(found.contains(&"o".to_owned()), "{found:?}");
    assert!(!found.contains(&"u".to_owned()) && !found.contains(&"i".to_owned()), "{found:?}");
}

#[test]
fn single_exact_match_closes_the_list() {
    assert!(labels("SELECT|").is_empty());
    assert!(labels("select|").is_empty());
    assert_eq!(labels("SELECT id FROM orders WHERE id = 1 AND tot|"), ["total"]);
}

#[test]
fn name_in_the_middle_ranks_after_name_at_the_start() {
    assert_eq!(labels("SELECT * FROM us|"), ["users", "active_users"]);
}

#[test]
fn missing_columns_are_reported_and_keep_the_other_suggestions() {
    let mut catalog = catalog();
    catalog.schemas[0].relations[0].columns = Columns::NotLoaded;
    let wanted = RelationRef { schema: "public".to_owned(), relation: "users".to_owned() };

    let found = run_with("SELECT u.| FROM users u", Dialect::Postgres, Some(&catalog));
    assert!(found.items.is_empty());
    assert_eq!(found.missing_columns, std::slice::from_ref(&wanted));

    let found = run_with("SELECT | FROM users u", Dialect::Postgres, Some(&catalog));
    assert_eq!(found.missing_columns, [wanted]);
    assert!(found.items.iter().any(|item| item.label == "u"));

    let found = run_with("SELECT * FROM us|", Dialect::Postgres, Some(&catalog));
    assert!(found.missing_columns.is_empty(), "a relation list does not need columns");
}

#[test]
fn multibyte_text_keeps_the_replace_range() {
    let catalog = catalog();
    assert_eq!(
        apply("SELECT 'ação', u.na| FROM users u", Dialect::Postgres, Some(&catalog), "name"),
        "SELECT 'ação', u.name FROM users u"
    );
}

#[test]
fn any_cursor_in_any_text_does_not_fail() {
    let catalog = catalog();
    let samples = [
        "SELECT (((", ")))", "...", "a.b.c.d.", "\"", "`", "[", "'", "$$", "--", "/*", "FROM", ",",
        "SELECT 'ação' , \"日本\".x FROM a.b.c AS d, (SELECT e) f JOIN",
        "INSERT INTO", "INSERT INTO (", "INTO . (", "( ) . x", "FROM (", "FROM ) x",
    ];
    for text in samples {
        for (cursor, _) in text.char_indices().chain([(text.len(), ' ')]) {
            for dialect in [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite] {
                suggest(text, cursor, dialect, Some(&catalog));
                suggest(text, cursor, dialect, None);
            }
        }
    }
}
