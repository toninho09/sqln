use sqln::connector::kafka::GUIDE;
use sqln::connector::kafka::command::parse;
use sqln::connector::kafka::filter::function_names;

/// The AI completion sends the common rules and the guide in each Kafka request, and the
/// request counts bytes as tokens.
const SYSTEM_LIMIT: usize = 8192;

fn examples() -> Vec<&'static str> {
    GUIDE
        .split("```sql\n")
        .skip(1)
        .map(|block| block.split("\n```").next().expect("a closed block"))
        .collect()
}

#[test]
fn each_guide_example_parses() {
    let examples = examples();
    assert!(examples.len() >= 5, "the guide keeps its examples");
    for sql in examples {
        assert!(parse(sql).is_ok(), "the example does not parse: {sql}");
    }
}

#[test]
fn the_guide_names_each_function() {
    for name in function_names() {
        let found = GUIDE.match_indices(name).any(|(at, _)| {
            let before = GUIDE[..at].chars().next_back();
            let after = GUIDE[at + name.len()..].chars().next();
            !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                && !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        assert!(found, "the guide does not name {name}");
    }
}

#[test]
fn the_kafka_system_message_fits_the_ai_request() {
    let system = sqln::ai::instruction(sqln::sql::Dialect::Kafka);
    assert!(system.ends_with(GUIDE));
    assert!(
        system.len() <= SYSTEM_LIMIT,
        "the system message has {} bytes",
        system.len()
    );
}

#[test]
fn the_unsupported_forms_of_the_guide_stay_refused() {
    for sql in [
        "SELECT * FROM t -- note",
        "SELECT * FROM t WHERE value.a <> 1",
        "SELECT *, value.x FROM t",
        "SELECT t.key FROM t",
        "SELECT -value.a FROM t",
        "SELECT COUNT(*) FROM t",
        "SELECT * FROM t ORDER BY offset",
        "SELECT * FROM t WHERE value.paid",
    ] {
        assert!(parse(sql).is_err(), "the parser accepts {sql}");
    }
    assert!(parse("SELECT *\n# note\nFROM t").is_ok());
    // A `#` line also stays permitted inside the columns of a SELECT, with indentation.
    assert!(parse("SELECT\n  # a note.\n  value.a,\n  value.b\nFROM t").is_ok());
    assert!(parse("# a note.\nSELECT * FROM t;").is_ok());
}
