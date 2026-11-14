# Kafka guide

This guide shows how to read Kafka topics with SQLN. Each section has a task and one or more ready examples. Copy an example, replace the topic name and the field paths, and run it with `F5`.

SQLN reads Kafka without a SQL server and without ksqlDB. The SQL here is the SQLN SELECT subset. It runs in the client, after the message decoding. Joins, aggregates, and `ORDER BY` are not supported.

## Contents

- [Connect](#connect)
- [Explore the cluster](#explore-the-cluster)
- [Read a finite number of messages](#read-a-finite-number-of-messages)
- [Watch new messages as they arrive](#watch-new-messages-as-they-arrive)
- [Follow the progress of a read](#follow-the-progress-of-a-read)
- [Filter by message content](#filter-by-message-content)
- [Project only the columns you need](#project-only-the-columns-you-need)
- [Transform text](#transform-text)
- [Work with numbers](#work-with-numbers)
- [Choose a value with conditions](#choose-a-value-with-conditions)
- [Work with time](#work-with-time)
- [Work with lists](#work-with-lists)
- [Combine conditions](#combine-conditions)
- [Use the Schema Registry](#use-the-schema-registry)
- [Inspect consumer groups and lag](#inspect-consumer-groups-and-lag)
- [Native commands](#native-commands)
- [AI completion and MCP](#ai-completion-and-mcp)
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
- `topics=<patterns>` limits the topics of the catalog. The form has the same **Topics** field, and `config.json` has `topics`.

### Show only some topics

A cluster can have many topics. Set a topic filter to see only the topics that you use:

```text
kafka://localhost:9092?topics=orders.*,billing-*,!*.dlq
```

- Separate the patterns with commas. `*` matches any text.
- A pattern that starts with `!` removes its topics. Without another pattern, all other topics stay.
- The case of the letters must match, as in Kafka.

The filter changes the navigator, the completion, the AI completion, and the MCP catalog. A read of another topic still runs, and `TOPICS` lists all topics. The consumer groups stay.

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

## Follow the progress of a read

The summary line under the grid shows the counts of a read while it runs. A narrow screen cuts the end of the line, so the important counts come first:

```text
Reading 45% · 12,345 messages scanned · 200 matched · 1,234 msg/s · 4.2 MiB
56 rows received. Live. · 5,000 messages scanned · lag 3,400 · 120 msg/s · 4.2 MiB
```

- **Percent:** a finite read only. It uses the end that comes first: the last offset of each partition at the start, `SCAN`, or `LIMIT`.
- **Matched:** only with a filter.
- **Lag:** `EMIT CHANGES` and `TAIL` only. It counts the messages between the read and the end of each partition. SQLN reads the end again every 2 seconds.
- **Speed:** the messages scanned in the last second. It appears after one second.
- **Size:** the bytes of the keys, the values, and the headers.

At the end, the summary keeps the counts and gives the reason:

```text
200 rows read. 12,345 messages scanned, 4.2 MiB. The read stopped at LIMIT.
```

A topic tab shows the same counts while it loads.

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

## Transform text

The text functions follow the Postgres functions with the same names. They read only a text; use `CAST(x AS TEXT)` for a number. A wrong type, null, or a missing field gives NULL.

Split a composite key, such as `tenant-7:customer:42`. The first part is 1, and a negative part counts from the end:

```sql
SELECT SPLIT_PART(key, ':', 1) AS tenant, SPLIT_PART(key, ':', -1) AS id FROM orders;

SELECT * FROM orders WHERE SPLIT_PART(key, ':', 1) = 'tenant-7';
```

Cut and clean a text:

```sql
SELECT
  # SUBSTRING keeps characters 5 to 7.
  SUBSTRING(value.code, 5, 3) AS number,
  LEFT(value.code, 3) AS prefix,
  RIGHT(value.code, 3) AS suffix,
  # TRIM removes the spaces at both ends.
  # RTRIM removes the zeros at the end.
  TRIM(value.name) AS name,
  RTRIM(value.ref, '0') AS ref,
  REPLACE(value.code, '-', '/') AS code
FROM orders;
```

Use a regular expression. `SUBSTRING(x, 'regex')` gives the first group, or the full match:

```sql
SELECT SUBSTRING(value.description, '#([0-9]+)') AS ticket FROM orders;

SELECT REGEXP_REPLACE(value.email, '@.*$', '@hidden') AS email FROM orders;

# g replaces each match. i ignores case.
SELECT REGEXP_REPLACE(value.phone, '[^0-9]', '', 'g') AS digits FROM orders;

# \1 is the first group. Write two backslashes because of the text escapes.
SELECT REGEXP_REPLACE(value.code, '([A-Z]+)-([0-9]+)', '\\2-\\1') AS code FROM orders;
```

Join texts. `CONCAT` and `CONCAT_WS` ignore NULL; `||` gives NULL with a NULL:

```sql
SELECT
  CONCAT(value.first_name, ' ', value.last_name) AS name,
  CONCAT_WS(', ', value.city, value.state, value.country) AS place
FROM orders;
```

Find large messages. `OCTET_LENGTH(value)` counts the bytes that Kafka sent, before the decoding:

```sql
SELECT offset, OCTET_LENGTH(key) AS key_bytes, OCTET_LENGTH(value) AS value_bytes
FROM orders
WHERE OCTET_LENGTH(value) > 500000;
```

## Work with numbers

```sql
SELECT
  # ROUND rounds half away from zero.
  ROUND(value.total * 1.1, 2) AS with_tax,
  ROUND(value.total, -2) AS hundreds,
  ABS(value.balance) AS balance,
  FLOOR(value.score) AS low,
  CEIL(value.score) AS high,
  GREATEST(value.a, value.b, value.c) AS largest,
  LEAST(value.a, value.b) AS smallest
FROM metrics;
```

`GREATEST` and `LEAST` ignore NULL and missing fields. Values of different types give NULL.

## Choose a value with conditions

`CASE` takes the value of the first condition that is true. Without `ELSE`, no match gives NULL. A condition accepts the full filter, also `ANY … END`:

```sql
SELECT
  key,
  CASE
    WHEN value.total >= 1000 THEN 'large'
    WHEN value.total >= 100 THEN 'medium'
    ELSE 'small'
  END AS size
FROM orders;
```

A `CASE` gives a value, not a condition. Compare it with another value in a filter:

```sql
SELECT * FROM orders
WHERE CASE WHEN value.country = 'BR' THEN 500 ELSE 100 END < value.total;
```

`NULLIF(a, b)` gives NULL when `a` equals `b`. Use it to treat an empty text as NULL:

```sql
SELECT COALESCE(NULLIF(value.nick, ''), value.name) AS name FROM users;
```

## Work with time

A time is a number of milliseconds since 1970, in UTC, as the `timestamp` column. A column shows a time as UTC text, such as `2026-11-10T12:00:00.000Z`.

Read the messages of the last hour, or watch their age:

```sql
SELECT * FROM orders WHERE timestamp >= NOW() - INTERVAL '1 hour';

SELECT offset, timestamp, NOW() - timestamp AS age_ms FROM orders EMIT CHANGES;
```

- `NOW()` is the time when SQLN examines each message.
- `INTERVAL` takes numbers and units: `ms`, `s`, `min`, `h`, `d`, and `week`, also with long names, such as `'1 hour 30 minutes'`. It refuses months and years, because their length changes.
- A time minus a time gives milliseconds.

Convert epoch numbers in the payload. `TO_TIMESTAMP` reads seconds, as Postgres; `TO_TIMESTAMP_MS` reads milliseconds:

```sql
SELECT
  TO_TIMESTAMP(value.created_at) AS created,
  TO_TIMESTAMP_MS(value.updated_at) AS updated
FROM orders
WHERE TO_TIMESTAMP_MS(value.updated_at) >= '2026-11-10';
```

A text next to a time becomes a time, as with `timestamp`.

Convert a time to an epoch number, or format it. `EPOCH`, `EPOCH_MS`, and `TO_CHAR` need a time:

```sql
# EPOCH gives 1731500000.123. EPOCH_MS gives 1731500000123.
SELECT
  EPOCH(timestamp) AS seconds,
  EPOCH_MS(timestamp) AS millis,
  EPOCH_MS(timestamp) - value.sent_ms AS delay_ms,
  TO_CHAR(timestamp, 'YYYY-MM-DD HH24:MI') AS minute
FROM orders;
```

`TO_CHAR` reads `YYYY`, `YY`, `MM`, `DD`, `HH24`, `HH12`, `HH`, `MI`, `SS`, `MS`, `US`, `AM`, `PM`, `Mon`, `Month`, `Dy`, `Day`, and `TZ`. The case of `MON`, `Mon`, and `mon` sets the case of the name. Names have no padding. Text between double quotes stays the same: `'YYYY-MM-DD"T"HH24'`.

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

Read a slice of the items. The first item is 0, and the item at the end position is not in the slice. The column shows a JSON list:

```sql
SELECT value.items[0:2].sku AS first_two FROM orders;

SELECT value.items[-3:] AS last_three FROM orders;
```

Count the items:

```sql
SELECT SIZE(value.items) AS count FROM orders WHERE SIZE(value.items) > 2;
```

Join the items in one text. `ARRAY_TO_STRING` reads a list, or all values of a path with `[*]` or a slice. Without a third text, it ignores NULL items:

```sql
SELECT ARRAY_TO_STRING(value.tags, ', ') AS tags FROM orders;

SELECT ARRAY_TO_STRING(value.items[*].sku, '|') AS skus FROM orders;
```

List the names of the fields of an object, in alphabetical order:

```sql
SELECT KEYS(value) AS fields, SIZE(KEYS(value.extra)) AS extra_count FROM events;
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

### Complete field paths

Type `value.` or `key.` in a `SELECT`, a `CONSUME`, a `TAIL`, or the filter of a topic tab. The list shows the fields of the latest schema of the subject `<topic>-value` or `<topic>-key`:

```sql
SELECT value.customer.name FROM orders WHERE value.items[*].sku = 'A1';
```

1. The first path of a topic reads its schemas. The note `Reading the fields of <topic>.` shows below the cursor during the read.
2. After the read, each field name shows at the next dot, with the type of its schema, such as `null | record` or `int64`. A list item accepts `[*]`, `[0]`, or `.0` before the next dot.
3. A name with a space or a dot gets double quotes when you accept it.
4. After `=`, `!=`, `IN (`, or `NOT IN (`, an enum field shows its values first, in the order of the schema. Type the start of a value, with or without `'`, to narrow the list.
5. The item of an open `ANY` or `EVERY` gets the fields of the items of its list:

   ```sql
   SELECT * FROM orders
   WHERE ANY line IN value.lines SATISFIES line.state = 'DONE' END;
   ```

6. Expand a topic in the navigator to see the same paths.
7. Use **Refresh** on the catalog to read a new schema version.

The completion uses only subjects with the topic name. JSON without a schema and map keys get no fields. A Protobuf schema gives the fields of its first message.

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

## AI completion and MCP

The AI completion works in a Kafka console, as in a SQL console. See [AI completion](AI_COMPLETION.md).

- Each request sends a compact syntax guide of this language, so the model writes SQLN Kafka and not standard SQL.
- A request sends the topic of the statement at the cursor, with its Schema Registry fields. Without a topic, it sends the topic names that start with the typed text first.

An MCP agent gets the same guide in `syntax.Kafka` of `get_context`. It reads the fields of a topic with `describe`, and it runs finite reads without full access. `TAIL` and `EMIT CHANGES` do not run through MCP. See [MCP](MCP.md).

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

Measure the age of each message and the delay between the producer and Kafka:

```sql
SELECT
  offset,
  timestamp,
  value.stage AS stage,
  NOW() - timestamp AS age_ms,
  EPOCH_MS(timestamp) - value.sent_ms AS producer_delay_ms
FROM pipeline
WHERE timestamp >= NOW() - INTERVAL '30 minutes'
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
- The summary line shows the scanned messages, the speed, and the size while a read runs.
- A final `SELECT` can omit `;`. End a statement with `;` before another one.
- Start a line with `#` for a comment.
- Put a name with spaces, dots, or brackets between double quotes after the dot: `value."order id"`.
- Missing paths, tombstones, and values without JSON return NULL.
- Functions follow Postgres. They use commas between their values, such as `SUBSTRING(x, 2, 3)`. A wrong type gives NULL.
- A time is a number of milliseconds in UTC. A time column shows UTC text. An interval shows milliseconds.
- Slices start at 0 and do not include the end item: `value.items[0:2]`.
- Reads have no global order across partitions.
- SQLN never joins a consumer group and never commits an offset.
