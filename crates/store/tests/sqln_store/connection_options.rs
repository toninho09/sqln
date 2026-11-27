// The connection options of the command line on a definition.

use std::path::PathBuf;
use std::time::Duration;

use sqln_session::connector::duckdb::DuckdbTarget;
use sqln_session::connector::sqlite::SqliteTarget;
use sqln_session::connector::target::{ConnectionOptions, OptionName, parse_uri};
use sqln_store::store::connection::{Definition, Source};

fn options(pairs: &[(OptionName, &str)]) -> ConnectionOptions {
    let mut options = ConnectionOptions::default();
    for (name, value) in pairs {
        options.set(*name, value, name.flag()).unwrap();
    }
    options
}

fn network(uri: &str) -> Definition {
    let target = parse_uri(uri).unwrap();
    Definition::new(target.label(), Source::Network(target))
}

#[test]
fn the_options_replace_the_limits_of_a_network_connection() {
    let mut definition = network("postgres://u@h/d");
    definition
        .apply(&options(&[
            (OptionName::ReadOnly, "true"),
            (OptionName::Production, "true"),
            (OptionName::Pool, "2"),
            (OptionName::ConnectTimeout, "7"),
            (OptionName::SqlTimeout, "0"),
            (OptionName::IdleTimeout, "0"),
        ]))
        .unwrap();
    assert!(definition.read_only);
    assert!(definition.production);
    assert_eq!(definition.max_connections, 2);
    assert_eq!(definition.connect_timeout, Duration::from_secs(7));
    assert_eq!(definition.sql_timeout, None);
    assert_eq!(definition.idle_timeout, None);
}

#[test]
fn empty_options_keep_the_definition() {
    let mut definition = network("mysql://u@h/d");
    let before = definition.clone();
    definition.apply(&ConnectionOptions::default()).unwrap();
    assert_eq!(definition, before);
}

#[test]
fn the_sqlite_options_go_to_the_file_target() {
    let mut definition = Definition::sqlite(SqliteTarget::new(PathBuf::from("a.db")));
    definition
        .apply(&options(&[
            (OptionName::ForeignKeys, "off"),
            (OptionName::BusyTimeout, "9"),
        ]))
        .unwrap();
    let Source::Sqlite(target) = &definition.source else {
        panic!("expected SQLite");
    };
    assert_eq!(target.foreign_keys, Some(false));
    assert_eq!(target.busy_timeout, Duration::from_secs(9));
}

#[test]
fn read_only_also_opens_duckdb_in_read_only_mode() {
    let mut definition = Definition::duckdb(DuckdbTarget::new("a.duckdb"));
    definition
        .apply(&options(&[(OptionName::ReadOnly, "true")]))
        .unwrap();
    let Source::Duckdb(target) = &definition.source else {
        panic!("expected DuckDB");
    };
    assert!(definition.read_only);
    assert!(target.read_only);
}

#[test]
fn the_kafka_options_set_the_registry_user_and_the_max_scan() {
    let mut definition = network("kafka://h?registry=https://r.test");
    definition
        .apply(&options(&[
            (OptionName::RegistryUser, "reader"),
            (OptionName::MaxScan, "10"),
        ]))
        .unwrap();
    let Source::Network(target) = &definition.source else {
        panic!("expected Kafka");
    };
    assert_eq!(target.kafka.registry.as_ref().unwrap().user, "reader");
    assert_eq!(target.kafka.max_scan, Some(10));
}

#[test]
fn an_option_of_another_source_is_refused_with_the_reason() {
    let sqlite = || Definition::sqlite(SqliteTarget::new(PathBuf::from("a.db")));
    for (mut definition, option, message) in [
        (
            network("postgres://u@h/d"),
            (OptionName::BusyTimeout, "1"),
            "Busy timeout is valid only for SQLite.",
        ),
        (
            network("postgres://u@h/d"),
            (OptionName::ForeignKeys, "on"),
            "Foreign keys is valid only for SQLite.",
        ),
        (
            network("redis://h/0"),
            (OptionName::MaxScan, "5"),
            "Max scan is valid only for Kafka.",
        ),
        (
            sqlite(),
            (OptionName::RegistryUser, "a"),
            "Registry user is valid only for Kafka.",
        ),
        (
            sqlite(),
            (OptionName::ConnectTimeout, "5"),
            "Connect timeout is valid only for network connections.",
        ),
        (
            network("kafka://h"),
            (OptionName::ReadOnly, "true"),
            "Read only has no effect on Kafka. SQLN only reads from Kafka.",
        ),
        (
            network("kafka://h"),
            (OptionName::RegistryUser, "a"),
            "Registry user needs a Schema Registry. Add registry=<url> to the URI.",
        ),
    ] {
        assert_eq!(definition.apply(&options(&[option])).unwrap_err(), message);
    }
}
