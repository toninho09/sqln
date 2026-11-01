// Tests of the connection text. They need no server.

use std::path::PathBuf;

use sqln::connector::target::{
    Address, ClientCertificate, Engine, Secret, Tls, TlsFiles, parse_uri,
};

#[test]
fn a_full_text_gives_each_part() {
    let target = parse_uri("postgres://app:s3cret@db.example.test:6543/sales").unwrap();
    assert_eq!(target.engine, Engine::Postgres);
    assert_eq!(
        target.address,
        Address::Tcp {
            host: "db.example.test".into(),
            port: 6543
        }
    );
    assert_eq!(target.user, "app");
    assert_eq!(target.password, Some(Secret::new("s3cret")));
    assert_eq!(target.database.as_deref(), Some("sales"));
}

#[test]
fn each_engine_has_its_scheme_and_port() {
    for (uri, engine, port) in [
        ("postgres://u@h", Engine::Postgres, 5432),
        ("postgresql://u@h", Engine::Postgres, 5432),
        ("mysql://u@h", Engine::MySql, 3306),
        ("mariadb://u@h", Engine::MySql, 3306),
        ("redis://u@h", Engine::Redis, 6379),
        ("valkey://u@h", Engine::Redis, 6379),
    ] {
        let target = parse_uri(uri).unwrap();
        assert_eq!(target.engine, engine);
        assert_eq!(
            target.address,
            Address::Tcp {
                host: "h".into(),
                port
            }
        );
        assert_eq!(target.password, None);
        assert_eq!(target.database, None);
    }
}

#[test]
fn percent_codes_keep_special_characters() {
    let target = parse_uri("mysql://a%40b:p%40ss%3Aw%2Frd@h/d%20b").unwrap();
    assert_eq!(target.user, "a@b");
    assert_eq!(target.password, Some(Secret::new("p@ss:w/rd")));
    assert_eq!(target.database.as_deref(), Some("d b"));
}

#[test]
fn an_ipv6_address_uses_brackets() {
    let target = parse_uri("postgres://u@[::1]:7000/d").unwrap();
    assert_eq!(
        target.address,
        Address::Tcp {
            host: "::1".into(),
            port: 7000
        }
    );
    assert_eq!(target.tls, Tls::Disabled);
}

#[test]
fn a_socket_comes_from_a_parameter() {
    let target = parse_uri("postgres://u@/d?host=%2Fvar%2Frun%2Fpostgresql").unwrap();
    assert_eq!(
        target.address,
        Address::Socket("/var/run/postgresql".into())
    );
    let target = parse_uri("mysql://u@/d?socket=/tmp/mysql.sock").unwrap();
    assert_eq!(target.address, Address::Socket("/tmp/mysql.sock".into()));
    assert_eq!(target.tls, Tls::Disabled);
    assert!(parse_uri("mysql://u@h/d?socket=/tmp/mysql.sock").is_err());
}

#[test]
fn local_addresses_use_no_tls_and_remote_addresses_verify() {
    for host in ["localhost", "LOCALHOST", "127.0.0.1", "127.1.2.3", "[::1]"] {
        let target = parse_uri(&format!("postgres://u@{host}/d")).unwrap();
        assert_eq!(target.tls, Tls::Disabled, "{host}");
    }
    for host in [
        "db.example.test",
        "10.0.0.5",
        "128.0.0.1",
        "[2001:db8::1]",
        "localhost.example.test",
    ] {
        let target = parse_uri(&format!("mysql://u@{host}/d")).unwrap();
        assert_eq!(target.tls, Tls::Verify(TlsFiles::default()), "{host}");
    }
}

#[test]
fn only_an_explicit_value_removes_tls_from_a_remote_address() {
    let target = parse_uri("postgres://u@db.example.test/d?sslmode=disable").unwrap();
    assert_eq!(target.tls, Tls::Disabled);
    let target = parse_uri("postgres://u@localhost/d?sslmode=require").unwrap();
    assert_eq!(target.tls, Tls::Verify(TlsFiles::default()));
}

#[test]
fn modes_with_a_fallback_to_plain_text_fail() {
    for mode in ["prefer", "allow", "PREFER"] {
        let error = parse_uri(&format!("postgres://u@h/d?sslmode={mode}")).unwrap_err();
        assert!(error.contains("fall back"), "{error}");
    }
}

#[test]
fn tls_files_go_to_the_target() {
    let target = parse_uri(
        "postgres://u@h/d?sslmode=verify-full&sslrootcert=/c/ca.pem&sslcert=/c/a.pem&sslkey=/c/a.key",
    )
    .unwrap();
    assert_eq!(
        target.tls,
        Tls::Verify(TlsFiles {
            ca: Some(PathBuf::from("/c/ca.pem")),
            client: Some(ClientCertificate {
                certificate: "/c/a.pem".into(),
                key: "/c/a.key".into(),
            }),
        })
    );
}

#[test]
fn incomplete_or_contradictory_tls_files_fail() {
    assert!(parse_uri("postgres://u@h/d?sslcert=/a").is_err());
    assert!(parse_uri("postgres://u@h/d?sslkey=/a").is_err());
    assert!(parse_uri("postgres://u@h/d?sslmode=disable&sslrootcert=/a").is_err());
}

#[test]
fn bad_texts_fail_with_a_message() {
    for uri in [
        "",
        "plain text",
        "oracle://u@h/d",
        "postgres://h/d",
        "postgres://u@/d",
        "postgres://u@h:port/d",
        "postgres://u@h:99999/d",
        "postgres://u@[::1/d",
        "postgres://u@h/d?unknown=1",
        "postgres://u%zz@h/d",
    ] {
        assert!(parse_uri(uri).is_err(), "{uri}");
    }
}

#[test]
fn no_error_or_debug_text_shows_the_password() {
    let password = "hunter2-secret";
    for uri in [
        format!("oracle://u:{password}@h/d"),
        format!("postgres://u:{password}@h:port/d"),
        format!("postgres://u:{password}@h/d?unknown=1"),
        format!("postgres://u:{password}%zz@h/d"),
        format!("postgres://u:{password}@/d"),
        format!("postgres:u:{password}@h"),
    ] {
        let error = parse_uri(&uri).unwrap_err();
        assert!(!error.contains(password), "{error}");
    }
    let target = parse_uri(&format!("postgres://u:{password}@h/d")).unwrap();
    assert!(!format!("{target:?}").contains(password));
    assert!(!target.label().contains(password));
}

#[test]
fn the_label_names_the_place() {
    assert_eq!(parse_uri("postgres://u:p@h:1/d").unwrap().label(), "h:1/d");
    assert_eq!(parse_uri("mysql://u@h").unwrap().label(), "h:3306");
    assert_eq!(parse_uri("mysql://u@/d?socket=/s").unwrap().label(), "/s/d");
}

#[test]
fn a_hidden_connection_text_keeps_its_length_and_its_other_parts() {
    use sqln::connector::target::hide_uri_password;
    let hide = |uri: &str| hide_uri_password(uri, '*');
    assert_eq!(
        hide("postgres://u:pass@host/db"),
        "postgres://u:****@host/db"
    );
    assert_eq!(
        hide("postgres://u:p@ss@host/db"),
        "postgres://u:****@host/db"
    );
    assert_eq!(hide("postgres://u@host/db"), "postgres://u@host/db");
    assert_eq!(hide("postgres://host:5432/db"), "postgres://host:5432/db");
    assert_eq!(hide("postgres://u:pa"), "postgres://u:**");
    assert_eq!(hide("not a uri"), "not a uri");
    let uri = "mysql://ação:日本語@host/db?sslmode=disable";
    assert_eq!(hide(uri).chars().count(), uri.chars().count());
    assert!(!hide(uri).contains("日本語"));
}

#[test]
fn redis_works_without_a_user() {
    let target = parse_uri("redis://:s3cret@127.0.0.1/3").unwrap();
    assert_eq!(target.user, "");
    assert_eq!(target.password, Some(Secret::new("s3cret")));
    assert_eq!(target.database.as_deref(), Some("3"));
    assert_eq!(target.tls, Tls::Disabled);

    let target = parse_uri("redis://127.0.0.1").unwrap();
    assert_eq!(target.password, None);
    assert_eq!(target.database, None);
}

#[test]
fn a_redis_database_is_a_number() {
    let error = parse_uri("redis://h/sales").unwrap_err();
    assert!(error.contains("must be a number"), "{error}");
}

#[test]
fn a_remote_redis_address_verifies_tls() {
    let target = parse_uri("redis://cache.example.test").unwrap();
    assert_eq!(target.tls, Tls::Verify(TlsFiles::default()));

    let target = parse_uri("redis://cache.example.test?sslmode=disable").unwrap();
    assert_eq!(target.tls, Tls::Disabled);
}

#[test]
fn the_tls_schemes_always_use_tls() {
    for uri in ["rediss://127.0.0.1", "valkeys://127.0.0.1"] {
        let target = parse_uri(uri).unwrap();
        assert_eq!(target.engine, Engine::Redis);
        assert_eq!(target.tls, Tls::Verify(TlsFiles::default()));
    }
    let error = parse_uri("rediss://h?sslmode=disable").unwrap_err();
    assert!(error.contains("always uses TLS"), "{error}");
}
