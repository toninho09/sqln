#!/bin/sh
# Source this file before the server tests: . scripts/test-env.sh
ROOT="$(pwd)"
export SQLN_MYSQL_URL='mysql://root:spike@127.0.0.1:33069/spike'
export SQLN_CERTS="$ROOT/.test-certs"
export SQLN_PG_TLS_PORT=54330
export SQLN_PG_PLAIN_PORT=54329
export SQLN_MYSQL_TLS_PORT=33070
# tests/servers.rs uses these connection texts.
export SQLN_TEST_POSTGRES='postgres://postgres:spike@127.0.0.1:54329/spike'
export SQLN_TEST_MYSQL='mysql://root:spike@127.0.0.1:33069/spike'
# tests/redis.rs uses these connection texts.
export SQLN_TEST_REDIS='redis://:spike@127.0.0.1:63799/0'
export SQLN_TEST_REDIS_OPEN='redis://127.0.0.1:63801/0'
export SQLN_TEST_VALKEY='valkey://:spike@127.0.0.1:63800/0'
export SQLN_REDIS_TLS_PORT=63802
# tests/kafka.rs uses these connection texts and the registry URL.
export SQLN_TEST_KAFKA='kafka://127.0.0.1:19092'
export SQLN_TEST_KAFKA_SASL='kafka://spike@127.0.0.1:19094?sasl=plain'
export SQLN_TEST_SCHEMA_REGISTRY='http://127.0.0.1:18081'
# spikes/ uses a URL, and spikes/drivers/ uses tokio-postgres key=value text.
export SQLN_PG_URL_SQLX='postgres://postgres:spike@127.0.0.1:54329/spike'
export SQLN_PG_URL_DRIVERS='host=127.0.0.1 port=54329 user=postgres password=spike dbname=spike'
