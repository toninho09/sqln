//! The MCP agent on a Kafka connection: the read policy, the syntax guide, and the field
//! paths of a topic.

use super::*;
use crate::common::mock::{MockConnector, RunHandler};
use sqln::catalog::ColumnNote;
use sqln::connector::StatementEnd;
use sqln::connector::kafka::GUIDE;
use sqln::grid::Value as Cell;
use std::sync::Arc;

fn kafka_harness() -> Harness {
    let handler: RunHandler = Arc::new(|_, mut sink| {
        Box::pin(async move {
            sink.set_columns(vec!["key".to_owned()]).await;
            sink.push([Cell::Text("order-1")].into_iter().collect())
                .await;
            let (rows, limit) = sink.finish().await;
            Ok(StatementEnd {
                rows,
                limit,
                has_columns: true,
                ..StatementEnd::default()
            })
        })
    });
    let connector = MockConnector::new(Dialect::Kafka, handler);
    connector.set_columns(
        "orders",
        vec!["value.status".into(), "value.total".into()],
        vec![
            ColumnNote {
                detail: "enum".into(),
                values: vec!["'OPEN'".into(), "'PAID'".into()],
            },
            ColumnNote {
                detail: "double".into(),
                values: Vec::new(),
            },
        ],
    );
    Harness::options(
        Options {
            connector: Some(Arc::new(connector)),
            ..Default::default()
        },
        KeyboardMode::Conventional,
    )
}

fn run(h: &mut Harness, sql: &str) -> Value {
    let job = h.query(sql);
    h.wait_job(&job)
}

#[test]
fn kafka_reads_pass_the_read_policy_and_other_commands_do_not() {
    for sql in [
        "TOPICS",
        "PARTITIONS orders",
        "GROUPS",
        "GROUP billing",
        "CONSUME orders FROM earliest LIMIT 5",
        "SELECT key, value.status FROM orders LIMIT 5;",
        "# a note\nSELECT key FROM orders WHERE value.total > 10 SCAN 100",
        // The runner refuses these reads outside a console, in every access level.
        "TAIL orders",
        "SELECT * FROM orders EMIT CHANGES",
    ] {
        assert!(
            agent::policy::validate(&[sql.into()], Dialect::Kafka).is_ok(),
            "{sql}"
        );
    }
    for sql in [
        "DELETE orders",
        "PRODUCE orders x",
        "SQLN.TOPIC orders 5",
        "",
    ] {
        let error = agent::policy::validate(&[sql.into()], Dialect::Kafka).unwrap_err();
        assert_eq!(error.code, "read_only_refused", "{sql}");
    }
    let error = agent::policy::validate(
        &["SELECT * FROM orders ORDER BY offset".into()],
        Dialect::Kafka,
    )
    .unwrap_err();
    assert_eq!(error.code, "invalid_sql");
    for part in ["SELECT accepts", "Position: byte", "syntax.Kafka"] {
        assert!(error.message.contains(part), "{part}: {}", error.message);
    }
}

#[test]
fn an_agent_reads_kafka_without_full_access_but_not_a_read_that_does_not_end() {
    let mut h = kafka_harness();
    h.authorize();
    assert_eq!(h.access()["access"], "propose");
    let reply = run(&mut h, "SELECT key FROM orders LIMIT 1");
    assert_eq!(reply["status"], "completed", "{reply}");
    assert!(reply.to_string().contains("order-1"), "{reply}");
    let reply = run(&mut h, "SELECT key FROM orders ORDER BY offset");
    assert!(reply.to_string().contains("syntax.Kafka"), "{reply}");
    for sql in ["TAIL orders", "SELECT * FROM orders EMIT CHANGES"] {
        let reply = run(&mut h, sql);
        assert_eq!(reply["status"], "failed", "{sql}: {reply}");
        assert!(reply.to_string().contains("does not end"), "{sql}: {reply}");
    }

    let mut h = kafka_harness();
    h.grant(3);
    assert_eq!(h.access()["access"], "full");
    let reply = run(&mut h, "TAIL orders");
    assert_eq!(reply["status"], "failed", "{reply}");
    assert!(reply.to_string().contains("does not end"), "{reply}");
}

#[test]
fn the_context_carries_the_kafka_guide_only_with_a_kafka_connection() {
    let mut h = kafka_harness();
    h.authorize();
    let context = h.tool(json!({"method":"get_context"})).unwrap();
    assert_eq!(context["connections"][0]["dialect"], "Kafka");
    assert_eq!(context["syntax"]["Kafka"], GUIDE);

    let mut h = Harness::new("no-kafka-guide");
    h.authorize();
    let context = h.tool(json!({"method":"get_context"})).unwrap();
    assert!(context.get("syntax").is_none(), "{context}");
}

#[test]
fn describe_reads_the_fields_of_a_kafka_topic() {
    let mut h = kafka_harness();
    h.authorize();
    let connection = h.connection();
    let fields = |h: &mut Harness, topic: &str| -> Value {
        let reply = h
            .tool(
                json!({"method":"describe","connection_id":connection,"schema":"topics",
                "object":{"name":topic,"kind":"table"}}),
            )
            .unwrap();
        let job = h.wait_job(reply["job_id"].as_str().unwrap());
        assert_eq!(job["status"], "completed", "{job}");
        assert_eq!(job["artifact_format"], "json", "{job}");
        serde_json::from_str(job["artifact"]["text"].as_str().unwrap()).unwrap()
    };
    let orders = fields(&mut h, "orders");
    assert_eq!(
        orders,
        json!({"topic":"orders","fields":[
            {"path":"value.status","type":"enum","values":["'OPEN'","'PAID'"]},
            {"path":"value.total","type":"double"},
        ]})
    );
    let empty = fields(&mut h, "payments");
    assert_eq!(empty["fields"], json!([]));
    assert!(
        empty["message"]
            .as_str()
            .unwrap()
            .contains("Schema Registry"),
        "{empty}"
    );
    let error = h
        .tool(
            json!({"method":"describe","connection_id":connection,"schema":"groups",
            "object":{"name":"billing","kind":"table"}}),
        )
        .unwrap_err();
    assert_eq!(error.code, "unsupported");
    assert!(error.message.contains("topics"), "{}", error.message);
}
