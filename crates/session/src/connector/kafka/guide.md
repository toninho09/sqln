# SQLN Kafka language

A Kafka connection does not accept standard SQL or ksqlDB. Write one statement: a SELECT, which can use more lines, or a command on one line. SQLN decodes each message and filters it in the client.

## SELECT

SELECT * | item [AS alias], ... FROM topic [clauses] [EMIT CHANGES]

Clauses, in any order, each one time:
- PARTITION n
- START AT EARLIEST | LATEST | OFFSET n | TIMESTAMP '2026-11-10T12:00:00Z'
- WHERE condition
- LIMIT n: the most matching rows.
- SCAN n: the most messages examined.

A plain SELECT reads from EARLIEST to the current end. EMIT CHANGES comes last, starts at LATEST, waits for new messages, and refuses LIMIT and SCAN.

## Commands

TOPICS | PARTITIONS topic | GROUPS | GROUP group_id
CONSUME topic [PARTITION n] [FROM earliest|latest|offset|@2026-11-10T12:00:00Z] [WHERE condition] [LIMIT n] [SCAN n]
TAIL topic [PARTITION n] [FROM ...] [WHERE condition]

## Columns and paths

Columns: partition, offset, timestamp, key, value, headers, schema.
Paths read the decoded JSON: value.customer.id, key.id, headers.source, value."order id", value.items[0], value.items[-1] (last), value.items[*].sku (each item), value.items[0:2] (end excluded).

## Conditions

= != < <= > >=, IS [NOT] NULL, IS [NOT] MISSING, [NOT] IN ('a', 'b'), 'vip' IN value.tags, [NOT] BETWEEN a AND b, LIKE, ILIKE, CONTAINS, MATCHES (regex). LIKE, ILIKE, CONTAINS, and MATCHES need a text between quotes at the right.
AND, OR, NOT, and parentheses. A path with [*] matches when one item matches.
ANY i IN value.items SATISFIES i.qty > 2 END, or EVERY ... END.

## Expressions

+ - * / %, || (join texts), CASE WHEN ... THEN ... ELSE ... END, CAST(x AS TEXT|NUMBER|BOOLEAN).
Text: LOWER, UPPER, LENGTH, TRIM, LTRIM, RTRIM, LEFT, RIGHT, SUBSTRING(x, start, count) or SUBSTRING(x, 'regex'), SPLIT_PART(x, ':', n), REPLACE, REGEXP_REPLACE(x, 'regex', 'new', 'g'), CONCAT, CONCAT_WS, OCTET_LENGTH.
Number: ABS, ROUND(x, digits), FLOOR, CEIL, GREATEST, LEAST.
Null and type: COALESCE, NULLIF, TYPEOF.
Lists: SIZE, KEYS, ARRAY_TO_STRING(list, ', ').
Time: NOW, TO_TIMESTAMP (seconds), TO_TIMESTAMP_MS, EPOCH, EPOCH_MS, TO_CHAR(timestamp, 'YYYY-MM-DD HH24:MI').
Literals: 'text' ('it''s' or 'it\'s'), numbers, true, false, null.
A time is milliseconds since 1970, in UTC. Compare timestamp with '2026-11-10T12:00:00Z' or NOW() - INTERVAL '1 hour'. INTERVAL units: ms, s, min, h, d, week.

## Not supported

JOIN, GROUP BY, COUNT and other aggregates, ORDER BY, DISTINCT, HAVING, UNION, subqueries, topic prefixes (orders.key), * with other items, <>, unary minus, writes, and -- comments. Start a line with # for a comment.

## Rules

- Put spaces around operators.
- Write each condition with an operator: value.paid = true, not value.paid.
- Use single quotes for text. Use double quotes only for names.
- = and IN compare text with its case: 'FAILED' is not 'failed'. Write enum values exactly as the schema writes them. CONTAINS and ILIKE ignore case; MATCHES ignores case with (?i).
- Bound a read of a large topic with LIMIT, SCAN, START AT, or a time condition.
- Partitions have no global order.
- A missing path, a wrong type, or a value without JSON gives NULL. Text functions need text: CAST(value.n AS TEXT).

## Examples

```sql
SELECT timestamp, key, value.status AS status, value.total AS total
FROM orders
WHERE value.status IN ('FAILED', 'RETRY') AND timestamp >= NOW() - INTERVAL '1 hour'
LIMIT 100 SCAN 100000;
```

```sql
SELECT offset, value.items[*].sku AS skus, SIZE(value.items) AS count
FROM orders PARTITION 2 START AT OFFSET 1500
WHERE ANY i IN value.items SATISFIES i.qty > 2 END
LIMIT 50;
```

```sql
SELECT SPLIT_PART(key, ':', 1) AS tenant,
  CASE WHEN value.total >= 1000 THEN 'large' ELSE 'small' END AS size
FROM orders
WHERE value.email ILIKE '%@example.com' AND value.cancel_reason IS NULL
LIMIT 20;
```

```sql
SELECT * FROM refunds WHERE value.amount > 100 EMIT CHANGES;
```

```sql
CONSUME orders FROM @2026-11-10T12:00:00Z WHERE key CONTAINS 'eu' LIMIT 100
```
