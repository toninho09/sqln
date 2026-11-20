//! The dialect names of the MCP replies.

use sqln_exec::dialect::features;
use sqln_sql::sql::Dialect;

#[test]
fn agent_dialect_names_stay_fixed() {
    let names: Vec<_> = Dialect::ALL
        .iter()
        .map(|dialect| features(*dialect).agent_name())
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        [
            "Databricks",
            "DuckDb",
            "Kafka",
            "MySql",
            "Postgres",
            "Redis",
            "Sqlite"
        ]
    );
    assert_eq!(features(Dialect::Kafka).agent_name(), "Kafka");
}
