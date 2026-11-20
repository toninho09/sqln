// The Kafka command parser reads text from the editor. It may refuse any text, but it
// must not panic, and an error offset stays inside the line.

use proptest::prelude::*;
use sqln_session::connector::kafka::command::parse;

/// Text made of the words, operators, and values of Kafka commands and filters.
fn command_line() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        Just("SELECT ".to_owned()),
        Just("CONSUME ".to_owned()),
        Just("TAIL ".to_owned()),
        Just("TOPICS ".to_owned()),
        Just("FROM ".to_owned()),
        Just("WHERE ".to_owned()),
        Just("LIMIT ".to_owned()),
        Just("AND ".to_owned()),
        Just("OR ".to_owned()),
        Just("NOT ".to_owned()),
        Just("IN ".to_owned()),
        Just("LIKE ".to_owned()),
        Just("ANY(".to_owned()),
        Just("CAST(".to_owned()),
        Just("COALESCE(".to_owned()),
        Just(" AS ".to_owned()),
        Just("orders ".to_owned()),
        Just("value.items[*]".to_owned()),
        Just("value.a[0]".to_owned()),
        Just("key".to_owned()),
        Just("@2026-01-02T00:00:01Z ".to_owned()),
        Just("@".to_owned()),
        Just("'".to_owned()),
        Just("\"".to_owned()),
        Just("\\".to_owned()),
        Just("(".to_owned()),
        Just(")".to_owned()),
        Just("[".to_owned()),
        Just("]".to_owned()),
        Just(", ".to_owned()),
        Just(";".to_owned()),
        Just(" = ".to_owned()),
        Just(" ~ ".to_owned()),
        Just(" || ".to_owned()),
        Just("é".to_owned()),
        "[0-9]{1,20}",
        "-?[0-9]{1,3}\\.[0-9]{0,3}",
        "[a-z_]{1,5}",
    ];
    proptest::collection::vec(piece, 0..20).prop_map(|pieces| pieces.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    #[test]
    fn kafka_commands_never_panic(line in prop_oneof![command_line(), any::<String>()]) {
        if let Err(error) = parse(&line)
            && let Some(offset) = error.offset
        {
            prop_assert!(offset <= line.len(), "offset {offset} in {line:?}: {error:?}");
        }
    }
}
