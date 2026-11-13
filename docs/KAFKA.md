# Kafka guide

This guide shows how to read Kafka topics with SQLN. Each section has a task and one or more ready examples. Copy an example, replace the topic name and the field paths, and run it with `F5`.

SQLN reads Kafka without a SQL server and without ksqlDB. The SQL here is the SQLN SELECT subset. It runs in the client, after the message decoding. Joins, aggregates, and `ORDER BY` are not supported.

## Contents

- [Connect](#connect)
- [Explore the cluster](#explore-the-cluster)
- [Read a finite number of messages](#read-a-finite-number-of-messages)
- [Watch new messages as they arrive](#watch-new-messages-as-they-arrive)
- [Filter by message content](#filter-by-message-content)
- [Project only the columns you need](#project-only-the-columns-you-need)
- [Work with lists](#work-with-lists)
- [Combine conditions](#combine-conditions)
- [Use the Schema Registry](#use-the-schema-registry)
- [Inspect consumer groups and lag](#inspect-consumer-groups-and-lag)
- [Native commands](#native-commands)
- [Common recipes](#common-recipes)

## Connect

Open a connection to a local cluster without encryption:

```text
kafka://localhost:9092
```

For a cluster with SASL, use the connection form. Enter the user and the password there, so the shell history keeps no password. Choose the SASL mechanism: `plain`, `scram-sha-256`, or `scram-sha-512`.

A URL can also carry settings:

```text
kafkas://kafka.example.com:9093?sasl=scram-sha-256&registry=https://registry.example.com
kafka://localhost:9092?group=reads-billing
```

- `kafkas` verifies the server certificate with the system roots. Add `ca=<file>` for a private CA.
- `registry=<url>` points to the Schema Registry. The form has the same field, with an optional user.
- `group=<id>` sets the `group.id` that reads report to the cluster. Without it, reads use `sqln-read`. SQLN never joins the group and never commits.

## Explore the cluster

List the topics:

```text
TOPICS
```

List the partitions of one topic, with the first and the last offset of each:

```text
PARTITIONS orders
```

Press `Enter` on a topic in the explorer to open its last 500 messages, newest first. Type a filter in the filter field of the tab.

## Read a finite number of messages

A plain `SELECT` reads the retained messages and stops at the last one. It starts at `EARLIEST` by default:

```sql
SELECT * FROM orders;
```

Start at a specific offset or time instead:

```sql
SELECT * FROM orders START AT OFFSET 1500;

SELECT * FROM orders START AT TIMESTAMP '2026-11-10T12:00:00Z';
```

Restrict the read to one partition:

```sql
SELECT * FROM orders PARTITION 2;
```

Two caps control the work:

- `LIMIT` caps the rows that match the filter.
- `SCAN` caps the messages examined, with or without a match.

```sql
SELECT * FROM orders LIMIT 100;

SELECT * FROM orders WHERE value.status = 'failed' LIMIT 100 SCAN 50000;
```

## Watch new messages as they arrive

`EMIT CHANGES` runs a continuous read. It starts at `LATEST` and waits for new messages until you press **Stop**. The live grid keeps a row cap and removes the oldest rows:

```sql
SELECT * FROM orders EMIT CHANGES;
```

Watch from a fixed point in the past:

```sql
SELECT * FROM orders START AT OFFSET 1500 EMIT CHANGES;
```

`EMIT CHANGES` accepts neither `LIMIT` nor `SCAN`. Press **Stop**, or wait for the statement timeout, to end the read.

## Filter by message content

The filter runs in the client, after the decoding. Compare fields, the key, and the metadata:

```sql
SELECT * FROM orders WHERE value.status = 'failed';

SELECT * FROM orders WHERE value.total > 1000;

SELECT * FROM orders WHERE value.paid = false;

SELECT * FROM orders WHERE key = 'customer-42';

SELECT * FROM orders WHERE value.customer.id = 42;

SELECT * FROM orders WHERE value.country != 'BR';
```

Compare two fields of the same message:

```sql
SELECT * FROM orders WHERE value.paid_amount < value.total_amount;
```

Filter on the message metadata:

```sql
SELECT * FROM orders WHERE partition = 2;

SELECT * FROM orders WHERE offset > 1500;

SELECT * FROM orders WHERE timestamp >= '2026-11-10T12:00:00Z';
```

Filter on a message header:

```sql
SELECT * FROM orders WHERE headers.source = 'web';
```

Other conditions:

```sql
-- Absent, null, or missing fields.
SELECT * FROM orders WHERE value.cancel_reason IS NULL;
SELECT * FROM orders WHERE value.discount IS NOT NULL;
SELECT * FROM orders WHERE value.notes IS MISSING;

-- Sets and ranges.
SELECT * FROM orders WHERE value.status IN ('failed', 'retry');
SELECT * FROM orders WHERE value.status NOT IN ('ok');
SELECT * FROM orders WHERE value.total BETWEEN 100 AND 500;
SELECT * FROM orders WHERE value.total NOT BETWEEN 100 AND 500;

-- Text search.
SELECT * FROM orders WHERE value.email LIKE '%@example.com';
SELECT * FROM orders WHERE value.city ILIKE 'sao paulo';
SELECT * FROM orders WHERE key CONTAINS 'eu';
SELECT * FROM orders WHERE value.description MATCHES '^refund #[0-9]+$';
```

Text comparisons with `LIKE`, `ILIKE`, `CONTAINS`, and `MATCHES` use the text of the field. Use `CAST(x AS TEXT)` for a number or a boolean.

## Project only the columns you need

List expressions after `SELECT`. Each one can have an alias with `AS`:

```sql
SELECT
  timestamp,
  partition,
  offset,
  key.id AS customer_id,
  value.status AS status,
  value.total AS total
FROM orders
WHERE value.status = 'failed'
LIMIT 100;
```

Compute new values from the message:

```sql
SELECT
  key.id AS id,
  value.price * value.qty AS total,
  COALESCE(value.nick, value.name, 'unknown') AS name,
  CAST(value.total AS NUMBER) - CAST(value.paid AS NUMBER) AS due
FROM orders;
```

Build text:

```sql
SELECT
  value.customer.first_name || ' ' || value.customer.last_name AS full_name,
  LOWER(value.email) AS email,
  LENGTH(value.notes) AS notes_len,
  TYPEOF(value.total) AS total_type
FROM orders;
```

## Work with lists

Read one item by position. Negative positions count from the end:

```sql
SELECT value.items[0] AS first_item FROM orders;

SELECT value.items[-1] AS last_item FROM orders;
```

Collect a field from every item. The column shows a JSON list of the found values:

```sql
SELECT value.items[*].sku AS skus FROM orders;
```

Count the items:

```sql
SELECT SIZE(value.items) AS count FROM orders WHERE SIZE(value.items) > 2;
```

Test the items in a filter. A path with `[*]` matches when one item matches:

```sql
SELECT * FROM orders WHERE value.items[*].sku = 'a-1';
```

Test membership the other way:

```sql
SELECT * FROM orders WHERE 'vip' IN value.tags;
```

Check one item or all items with `ANY` and `EVERY`:

```sql
-- True when one item has this sku and a quantity over 2.
SELECT * FROM orders
WHERE ANY i IN value.items SATISFIES i.sku = 'a-1' AND i.qty > 2 END;

-- True when every item has a price over 0.
SELECT * FROM orders
WHERE EVERY i IN value.items SATISFIES i.price > 0 END;
```

Expressions generate values on demand. Large list combinations stay under Stop, the statement timeout, and the memory limit. An oversized projection fails without a partial row; reduce it or increase the limit.

## Combine conditions

Join conditions with `AND`, `OR`, and `NOT`. Use parentheses for the order:

```sql
SELECT
  key.id AS id,
  value.price * value.qty AS total,
  COALESCE(value.nick, value.name) AS name
FROM orders
WHERE ANY i IN value.items SATISFIES i.sku = 'a' AND i.qty > 2 END
  AND headers.source IN ('web', 'app')
  AND timestamp >= '2026-11-10T12:00:00Z';
```

Another combination:

```sql
SELECT * FROM orders
WHERE (value.status = 'failed' OR value.attempts > 3)
  AND NOT value.country IN ('BR', 'PT')
  AND key CONTAINS 'eu';
```

## Use the Schema Registry

Connect with `registry=<url>` or set the URL in the connection form. With a registry, Avro, JSON Schema, and Protobuf values become JSON, so field paths such as `value.customer.id` work.

```text
kafkas://kafka.example.com:9093?registry=https://registry.example.com
```

The `schema` column shows the id of the schema of each message. A value that does not decode stays as bytes, with a note in **Messages**. The filter then sees no JSON fields for that message.

## Inspect consumer groups and lag

List the groups of the cluster:

```text
GROUPS
```

Show the committed offsets and the lag of one group:

```text
GROUP billing
```

SQLN never joins a consumer group and never commits an offset. Use `GROUP` only to read the state.

## Native commands

Each native command occupies one line. Each command only reads:

```text
TOPICS
PARTITIONS orders
CONSUME orders FROM earliest WHERE value.customer.id = 42 AND key CONTAINS 'eu' LIMIT 100
CONSUME orders PARTITION 2 FROM @2026-11-10T12:00:00Z SCAN 50000
TAIL orders WHERE value.status = 'failed'
GROUPS
GROUP billing
```

- `CONSUME` reads to the last message of each partition at its start. `TAIL` waits for new messages until **Stop**.
- `FROM` takes `earliest`, `latest`, an offset, or `@` and a UTC time.
- Both `SELECT` and the native commands use the same filter and the same expressions in SQLN.

## Common recipes

Find failed payments from a start time, with the useful columns:

```sql
SELECT
  timestamp,
  key.id AS id,
  value.total AS total,
  value.error AS error
FROM payments
WHERE value.status = 'failed'
  AND timestamp >= '2026-11-10T00:00:00Z'
LIMIT 200;
```

Watch a live stream of refunds in one partition:

```sql
SELECT * FROM refunds PARTITION 0 WHERE value.amount > 100 EMIT CHANGES;
```

Collect the order items of one customer, as JSON lists:

```sql
SELECT
  offset,
  value.items[*].sku AS skus,
  SIZE(value.items) AS count
FROM orders
WHERE key.id = 42
START AT EARLIEST;
```

Measure the delay between the message time and a recent start:

```sql
SELECT offset, timestamp, value.stage AS stage
FROM pipeline
WHERE timestamp >= '2026-11-10T12:00:00Z'
SCAN 100000;
```

Debug tombstones and empty keys:

```sql
SELECT * FROM orders WHERE key IS MISSING LIMIT 50;

SELECT * FROM orders WHERE value IS NULL LIMIT 50;
```

## Rules to keep in mind

- A plain `SELECT` starts at `EARLIEST` and stops at the last message. `EMIT CHANGES` starts at `LATEST` and waits until **Stop**.
- `EMIT CHANGES` accepts neither `LIMIT` nor `SCAN`.
- `LIMIT` caps matching rows. `SCAN` caps examined messages.
- A final `SELECT` can omit `;`. End a statement with `;` before another one.
- Start a line with `#` for a comment.
- Put a name with spaces, dots, or brackets between double quotes after the dot: `value."order id"`.
- Missing paths, tombstones, and values without JSON return NULL.
- Reads have no global order across partitions.
- SQLN never joins a consumer group and never commits an offset.
