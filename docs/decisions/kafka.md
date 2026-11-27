# Decisions — Kafka

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## Kafka AI completion, MCP reads, and topic filter — 2026-11-13

The user asks for prompts that tell the AI completion and the MCP agents how to build correct Kafka queries, and for a limit on the context of a cluster with many topics. The user approves the points below.
This entry replaces decision 24 of "Kafka and Schema Registry", the Scope row of "Explicit AI SQL completion" for Kafka, and the Kafka row of the read-only protection of the MCP guide.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| AI completion | Kafka consoles offer the AI completion. Redis stays out | Keep Kafka out | User choice |
| MCP reads | Without full access, `query` and `run_console` run the known Kafka commands: SELECT, `CONSUME`, `TOPICS`, `PARTITIONS`, `GROUPS`, and `GROUP`. `TAIL` and `EMIT CHANGES` refuse in every access level, because they do not end | Keep the refusal of each Kafka command | User choice. Each Kafka command only reads |
| Field paths | The AI request and the MCP `describe` get the field paths of the Schema Registry, with their types and enum values | Topic names only | User choice. Without fields, the model guesses paths |
| Large clusters | Two limits: an automatic focus on the topic of the statement, and a topic filter of the connection | One of the two | User choice |
| Focus | A known topic after `FROM`, `CONSUME`, `TAIL`, or `PARTITIONS` goes alone, with its fields. Without a known topic, the request sends the names of the topics, then of the groups, as the context permits, first the names that start with the typed text | All topics in each request | Approved with the plan. A statement reads one topic |
| Filter syntax | A list with commas. `*` matches any text. A pattern that starts with `!` removes its topics. Without an include pattern, all topics match before the excludes. The case of the letters must match | Only includes; a regular expression | User choice |
| Filter effect | Visibility only: the catalog, the navigator, the local completion, the AI completion, and the MCP `describe`. A read of another topic still runs, and `TOPICS` lists all topics | Also refuse the reads of other topics | User choice. Same rule as the schema filter of SQL connections; `TOPICS` is a query, not the catalog |
| Filter places | The **Topics** field of the Kafka form, `topics` in `config.json`, and `topics=` in the connection text | Form and `config.json` only | User choice. Same places as the consumer group |
| Groups | The filter applies to topics only | Topics and groups | User choice. A group name does not hold the topic name |

Implementation decisions:

| Point | Decision | Reason |
|---|---|---|
| One guide | `src/connector/kafka/guide.md` is the compact syntax guide. The AI completion sends it after the common rules, and `get_context` returns it as `syntax.Kafka` when a listed connection is Kafka. A test parses each example and checks that the guide names each function. The Kafka system message, the rules and the guide, stays at 8 KiB or less. The guide holds no rule of the agent tools: the MCP instructions and the `query` description say that `TAIL` and `EMIT CHANGES` refuse | DRY. [Kafka](../KAFKA.md) stays the guide for people |
| AI rules | The system message holds the common rules, then the language of the dialect: one line for a SQL dialect, or the Kafka guide. The metadata budget subtracts its size, which is fixed for each dialect | The leading bytes stay stable for the prompt cache |
| Case of values | The common rules tell the model to write names and enum values exactly as metadata writes them. The guide says that `=` and `IN` compare text with its case, and that `CONTAINS`, `ILIKE`, and `MATCHES` with `(?i)` ignore case | The Kafka filter compares text with its case, so `'failed'` does not match `'FAILED'` |
| Empty fields | The request omits an empty `metadata` list, an empty `targets` list, and an absent `schema`, for all dialects | The model reads fewer bytes without a loss of facts |
| AI fields cache | One snapshot holds the fields of at most 4 topics, the newest last. A request on a known topic without fields reads the snapshot again. A name that the catalog does not know reads no fields | Parallel requests on different topics do not cancel the reads of each other |
| Omitted count | With a focus topic, the other topics do not count as omitted context | The reduced-context notice stays for a context that does not fit |
| Registry failure | The AI request sends the topic without fields. The local completion of the editor shows the error | The completion still works without the registry |
| MCP syntax error | Without full access, a Kafka statement that does not parse refuses with `invalid_sql`, the reason, the byte position, and a pointer to `syntax.Kafka` | The agent can correct its query |
| MCP fields | `describe` with `schema: "topics"` and an object of kind `table` returns `{topic, fields}` as a JSON artifact. An empty list has a message with the reason. Other Kafka objects refuse with the supported form | No protocol change: `VERSION` stays 2 |
| Compatibility | `topics` is optional, and `version` stays 1. Older SQLN binaries refuse a file with `topics`, because they refuse unknown fields | Same rule as the other optional keys |

## Kafka Postgres functions — 2026-11-13

The user asks for the Postgres functions that are useful to read topics, without aggregates, `ORDER BY`, `JOIN`, `unnest`, the jsonb operators, and hash functions. The user approves the points below.
This entry removes `CASE`, date functions, `keys`, and slices from the "Out of scope" line of "Kafka SQL expressions — 2026-11-12". The other rules of that entry stay: two-valued logic, `[*]` values one at a time, absent and null, the temporary memory limit, Stop, and the timeout.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Slices | `value.items[a:b]`: the first item is 0, and the item at `b` is not in the slice. A negative position counts from the end. `[:b]` and `[a:]` are permitted | 1-based with the end included, as Postgres arrays | User choice. Same base as `[0]` and `[-1]` |
| `NOW()` | SQLN reads the clock one time for each message. The filter and the columns of one message get the same value | One time for the full read, as a Postgres transaction | User choice. `NOW() - timestamp` gives the real age in `EMIT CHANGES` |
| Epoch to time | `TO_TIMESTAMP(x)` reads seconds, as Postgres. `TO_TIMESTAMP_MS(x)` reads milliseconds | One unit | User choice |
| Time to epoch | `EPOCH(t)` gives seconds, with a fraction. `EPOCH_MS(t)` gives whole milliseconds | No reverse function | User choice |
| Intervals | A number of milliseconds, also in a column | Text such as `00:05:12` | User choice. The grid sorts and compares a number |
| Syntax | Only the forms with commas, which Postgres also accepts: `SUBSTRING(x, 2, 3)`, `TRIM(x, 'ab')` | `SUBSTRING(x FROM 2 FOR 3)`, `TRIM(BOTH … FROM x)` | Approved with the plan. One argument syntax |
| `CASE` | Only `CASE WHEN condition THEN value … [ELSE value] END`. A condition accepts the full filter, also `ANY … END`. Without `ELSE` and without a match, the value is null | `CASE x WHEN a THEN …` | Approved with the plan |
| Text arguments | The regular expression and the replacement of `REGEXP_REPLACE`, its flags, the format of `TO_CHAR`, and the texts of `ARRAY_TO_STRING` are texts between quotes. SQLN checks them before the read | Any value | Approved with the plan. A bad pattern refuses before the read |
| Wrong type | A strict function gives no value for a wrong type, null, or an absent value, as `LOWER`. Text functions read only a JSON text; use `CAST(x AS TEXT)` | Automatic conversion | Approved with the plan. Same rule as the existing functions |
| `OCTET_LENGTH` | With `key` or `value` alone: the bytes that Kafka sent, before the decoding. With another text: its UTF-8 bytes | The bytes of the decoded text | Approved with the plan. Finds the messages near `message.max.bytes` |
| `KEYS` | A list of the names in alphabetical order | The order of the message | Approved with the plan. The JSON of SQLN does not keep the order |
| Interval units | `ms`, `s`, `min`, `h`, `d`, and `week`, with their long and plural forms. Months, years, and `m` alone refuse | 30 days for a month | Approved with the plan. Their length changes; `m` can be a minute or a month |
| Time zone | UTC | The local zone | Same rule as `timestamp` |

Functions:

| Group | Functions and rules |
|---|---|
| Text | `SPLIT_PART(x, sep, n)`: `n` starts at 1, a negative `n` counts from the end, and a part outside the text gives `''`. `SUBSTRING(x, start [, count])` counts characters with the Postgres rule for a start before 1. `SUBSTRING(x, 'regex')` gives the first group or the match, and null without a match. `LEFT` and `RIGHT` with a negative count remove characters from the other end. `TRIM`, `LTRIM`, and `RTRIM` remove spaces, or the characters of the second value. `REPLACE` replaces each match. `REGEXP_REPLACE(x, 'regex', 'replacement' [, 'flags'])` replaces the first match, or each match with `g`; `i` ignores case; `\1` to `\9` and `\&` in the replacement are written `'\\1'` because of the backslash escapes. `CONCAT` and `CONCAT_WS` ignore null and absent values, and write another type as JSON text |
| Numbers | `ABS`, `FLOOR`, `CEIL`, and `ROUND(x [, places])`. `ROUND` rounds half away from zero on the decimal text of the number, so `ROUND(2.675, 2)` gives 2.68. Whole numbers keep all digits. `GREATEST` and `LEAST` ignore null and absent values; mixed types give no value |
| Conditions | `NULLIF(a, b)` and `CASE` |
| Lists | A slice reads its items as `[*]` does: a column shows a JSON list, one item that matches is enough, an empty slice gives NULL, and `SIZE` refuses it. `KEYS(x)` gives the names of an object. `ARRAY_TO_STRING(list, 'sep' [, 'null text'])` joins the items of a list or all values of a path with `[*]` or a slice |
| Time | A time is a number of milliseconds since 1970, as `timestamp` in an expression. `NOW()`, `TO_TIMESTAMP`, `TO_TIMESTAMP_MS`, time plus or minus an interval, and `COALESCE`, `NULLIF`, `GREATEST`, `LEAST`, or `CASE` of times give a time. A time minus a time gives an interval. A column shows a time as `YYYY-MM-DDTHH:MM:SS.sssZ`, as the `timestamp` column. A text next to a time becomes a time before the read. `INTERVAL '1 hour 30 minutes'` is a number of milliseconds. `EPOCH`, `EPOCH_MS`, and `TO_CHAR` refuse a value that is not a time. `TO_CHAR` reads `YYYY YY MM DD HH24 HH12 HH MI SS MS US AM PM Mon Month Dy Day TZ`; names have no padding; text between double quotes and other characters stay the same |

Compatibility:

- No saved format changes. Existing commands and saved scripts keep their syntax and results.
- An `ANY` or `EVERY` item named `case` keeps its meaning; `CASE` starts an expression only when no item has that name.
- The completion has a limit of 50 items, so the function names come after the other words with the same match. An empty filter list shows the message columns and the condition words first.

Implementation decisions:

| Point | Decision | Reason |
|---|---|---|
| Modules | `filter/functions.rs` holds the function table, the argument checks, and the evaluation. `filter/time.rs` reads intervals and `TO_CHAR` formats | `expr.rs` keeps the expression tree and the parser |
| One table | The completion, the keyword colors, and the parser read the names and the argument counts from one table | DRY |
| Arguments | One visitor generates the combinations of the argument values without a list. `CONCAT`, `CONCAT_WS`, `GREATEST`, and `LEAST` give null for an argument without a value | Same Cartesian rule as arithmetic |
| Message facts | `filter::Meta` holds `NOW()` and the received bytes of the key and the value. The read creates it one time for each message and gives it to the filter and to the columns | The row does not keep these facts |
| Memory | Functions that generate text or lists use the temporary memory limit | Same rule as `\|\|` and `CAST` |

## Kafka schema field completion — 2026-11-13

The user asks for the completion of the fields of Avro, Protobuf, and JSON Schema values, and approves the points below.
This entry replaces decision 23 of "Kafka and Schema Registry" and the words "It uses no database reads" of the Completion row of "Kafka SELECT queries".

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Source | The latest schema of the subjects `<topic>-value` and `<topic>-key` in the Schema Registry. A topic without a subject or without a registry has no fields | A sample of messages | User choice. JSON without a schema has no suggestions |
| Time of the read | The first `value.` or `key.` path of a topic in the editor or in the filter of a topic tab | The catalog read | User choice. Only the topics in use cost a read |
| Note | While the read runs, an empty list shows `Reading the columns of <relation>.` below the cursor, in the hint style. A Kafka list shows `Reading the fields of <topic>.`. The note takes no keys and no clicks. A key that closes a list also closes the note | A status message | User choice: the note is at the place of the list |
| Note scope | All connections: a SQL list that waits for the columns of a table shows the same note | Only Kafka | User choice. One rule in the editor and in the filter fields |
| Store | The paths are the columns of the topic in the catalog, until **Refresh** of the catalog | A second cache | Reuses the column flow of SQL |
| Navigator | An expanded topic shows its paths, and the expansion reads them | Only the completion | User choice |
| Roots | `value` and `key` | Only `value` | User choice |
| Failure | The status line gives the reason, and the note closes. A later path asks again | A silent empty list | No silent failure |
| Type | Each field shows the type words of its schema format beside its name, such as `null \| record`, `int64`, or `repeated message` | Neutral JSON kinds | User choice |
| Values | After `field =`, `field !=`, `field IN (`, and `field NOT IN (`, also after the items of the list, the list offers the values of an enum field first, in the order of the schema. A typed prefix narrows them, with or without the open quote `'`. After an open quote, only values show | Alphabetical order | User choice. The order of an enum often has a meaning |
| Items | `ANY` and `EVERY` items that are open before the cursor get the fields of the items of their list, also when they are nested. `END` closes the last open item | No fields | Same paths as the list |
| Literal | A Kafka text literal uses backslash escapes: `\'`, `\\`, `\n`, `\r`, `\t`, and `\xHH` for other control characters | Doubled quotes | SELECT and the line commands both read backslash escapes; only SELECT reads doubled quotes |

Mapping rules: the paths follow the JSON of the decoder.

| Format | Fields | No fields below |
|---|---|---|
| Avro | Record fields; each branch of a union; `[*]` for the items of an array; named types and references | Map, enum, fixed, primitive and logical types |
| Protobuf | The first message of the schema, as the index `[0]` of the wire format; JSON names, such as `orderId`; the fields of a `oneof`; `[*]` for repeated messages | Map; each `google.protobuf.*` type; enum and scalar types |
| JSON Schema | `properties`; `[*]` for `items` and `prefixItems`; `$ref` to the same document or to a reference name; each schema of `allOf`, `anyOf`, `oneOf`, `then`, and `else` | `additionalProperties`, `patternProperties`, a remote `$ref` |

Type and value rules: a path that two schemas describe joins their type words with ` | ` and their values.

| Format | Type words | Values |
|---|---|---|
| Avro | `null`, `boolean`, `int`, `long`, `float`, `double`, `bytes`, `string`, `record`, `enum`, `array`, `map`, `fixed`, and the logical names, such as `timestamp-millis`; a union joins its branches; a name gives the words of its type | The symbols of an enum, also in a union |
| Protobuf | The scalar names, such as `int64`; `message`; `enum`; `map`; the short name of a `google.protobuf.*` type, such as `Timestamp`; `repeated` before the type of a list | The names of the enum values, as the decoder writes them |
| JSON Schema | `type`, or its list joined; without `type`: `object`, `array`, or `enum` from the shape | The scalar items of `enum` and `const` |

Implementation limits: 10 steps after the root, 2000 paths for each root, 100 values for each path, and 100,000 visited schema nodes. A recursive type stops at its second use in one path. The catalog keeps one note for each column; a source without notes keeps the default `Session::read_noted_columns`.

Known limitations: only the subject names of the TopicNameStrategy; Protobuf uses only the first message; a new schema shows after **Refresh** of the catalog.

## Kafka read progress — 2026-11-13

The user asks for the count of consumed messages while a Kafka command runs, and approves the choices below.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Place | The summary line under the grid of the console result and of the topic tab | The status line | Each result has its own counts. The status line has less space at 80 columns |
| Counts | Scanned messages; matched messages, only with a filter; bytes of the keys, values, and headers | Matched messages without a filter | Without a filter, the two counts are equal |
| Percent | Only for a finite read: the largest part of its three ends, the offsets to the high offset of each partition at the start, `SCAN`, and `LIMIT` | A percent for `TAIL` | `TAIL` has no end |
| Speed | `msg/s` of the scanned messages in the last second. The screen measures it at each frame; the first speed comes after one second | An average of the whole read | An average hides a pause of `TAIL` |
| Lag | Only for `TAIL`: the sum of the high offset minus the next offset of each partition. A check of the high offsets runs each 2 seconds on a task, and the read never waits for it. A failed check removes the lag | Statistics of librdkafka; a check that stops the read | The statistics give the lag only for committed offsets, and SQLN never commits |
| End | The console summary adds the counts and the reason: the end of each partition, `LIMIT`, the `SCAN` limit, or a stop. `TAIL` gives no reason, because it always ends with **Stop**. The topic tab shows the counts only while it loads | Keep `N rows read.` | The user choice |
| Delivery | The connector writes atomic counters, and `ExecEvent::Columns` carries them once | One event for each change | Same reason as the export progress: the event queue has 256 places |

These choices follow the rules above on the points the rules left open.

| Subject | Choice | Reason |
|---|---|---|
| Order of the parts | State, scanned, matched, lag, speed, bytes | A narrow screen cuts the end of the line |
| Notice in **Messages** | `Scanned N messages; M matched.` stays, with the rule 12 of "Kafka and Schema Registry" | The Messages history keeps it |
| Row cap or data budget | The summary of the grid gives the reason, and the Kafka text adds only the counts | The grid text already explains the limit |

## Kafka SQL expressions — 2026-11-12

The user asks for a Kafka filter that covers most of what `jq` does, with a syntax that SQL users know. The references are PartiQL and SQL++. The user approves the points below.
The filter applies to `WHERE` in SELECT, CONSUME, TAIL, and the filter field of the topic tab. The expressions also apply to the SELECT columns.

| Point | Approved rule | Rejected |
|---|---|---|
| Condition on one item | `ANY i IN value.items SATISFIES i.sku = 'a' AND i.qty > 2 END`; `EVERY … END` | `EXISTS(value.items[*] WHERE …)`; `ANY(value.items, i -> …)` |
| Absent field | `IS [NOT] NULL` is true for an absent field or null. `IS [NOT] MISSING` is true only for an absent field. `value.x = null` keeps its rule | Only `IS NULL` |
| Columns | The expressions apply to `WHERE` and to the SELECT columns | Only `WHERE` |
| Special names | `value."order id"."a.b"`: double quotes after a dot. `""` is one quote. A text alone in double quotes stays a literal | `value['order id']` |

Syntax:

| Feature | Syntax | `jq` |
|---|---|---|
| Roots | `partition`, `offset`, `timestamp`, `key`, `value`, `headers`, `schema` | Fields of `kcat -J` |
| Fields of the key and headers | `key.id`, `headers.trace` | `.key \| fromjson \| .id` |
| Field and field | `value.paid < value.total`; a value on either side | `.paid < .total` |
| Text of a field | `value.email CONTAINS 'x'`, `value.email MATCHES 'regex'` | `contains`, `test` |
| LIKE | `[NOT] LIKE`, `[NOT] ILIKE`; `%`, `_`, and `\` before a literal character | `startswith`, `endswith` |
| List of values | `value.status [NOT] IN ('a', 'b')` | `IN("a", "b")` |
| Item of a list field | `'vip' IN value.tags`, with any value on the left | `index("vip")` |
| Range | `[NOT] BETWEEN low AND high` | `. >= low and . <= high` |
| Arithmetic | `+ - * / %`, with spaces before and after; `\|\|` joins texts | `+ - * / %` |
| Functions | `LOWER`, `UPPER`, `LENGTH` (characters of a text), `SIZE` (items of a list), `COALESCE(a, b, …)`, `CAST(x AS NUMBER\|TEXT\|BOOLEAN)`, `TYPEOF(x)` | `ascii_downcase`, `length`, `//`, `tonumber`, `tostring`, `type` |
| Time | `timestamp >= '2026-11-10T12:00:00Z'`, with the time reader of `START AT TIMESTAMP` | `.ts >= …` |

Rules that follow from these choices:

- The logic keeps two values. A comparison with an absent value or null is false, and `NOT` inverts the result. There is no three-valued logic.
- `key` alone compares as text. `value` alone compares as decoded JSON. `CONTAINS`, `MATCHES`, `LIKE`, and `ILIKE` on `key`, `value`, or `headers` alone read the whole text. On another value, they read only a JSON text.
- `key.x` decodes the key as JSON. `headers.x` reads the JSON object of the `headers` column.
- With `[*]` on either side, one pair of values that matches is enough. `BETWEEN` needs one value inside the range. A column with `[*]` shows a JSON list. Arithmetic with `[*]` acts on each item.
- Arithmetic accepts only numbers. Division by zero, another type, or an absent value gives an absent value. A whole result has no fraction. Whole numbers keep all their digits.
- A `CAST` that fails gives an absent value. Null stays null. `TYPEOF` gives `null`, `boolean`, `number`, `string`, `array`, or `object`.
- `EVERY` is true for an empty list. `ANY` and `EVERY` are false when the field is not a list. The item name is a new simple name, not a message column. `ANY` can be inside `ANY`.
- `timestamp` next to a text converts the text when the filter loads. A bad time refuses before the read.
- `partition`, `offset`, `timestamp`, `SIZE`, `LENGTH`, and arithmetic give numbers. A comparison of such a value with a text, `true`, `false`, or `null` refuses before the read. `CONTAINS`, `MATCHES`, `LIKE`, and `ILIKE` refuse such a value.
- `PARTITION`, `OFFSET`, and `TIMESTAMP` stay clauses. At the start of a condition, the word is a root. After a complete condition, the word starts the next clause.
- A column without `AS` uses its written text as the label. A message column alone keeps its value and type.
- A key or a value without UTF-8 text is absent for the expressions.

Compatibility:

- `,` is an operator in the line commands too, for `IN (…)` and `COALESCE`. A group id with a comma needs quotes, such as `GROUP 'a,b'`.
- `value.order-id` and `-5` stay words, because arithmetic needs spaces.
- The text reader keeps its backslash escapes, so `LIKE '100\\%'` finds the text `100%`.
- No saved format changes. Existing commands and saved scripts keep their syntax and results.
- Conditions that the earlier entries refused now run: `key > 'a'`, `key.id = 1`, `'a' = value.b`, `SIZE(key)`, `'a' IN key`, and `SELECT key.field`.
- Quoted names remove the known limitation of the array entry for a name that holds `[`. A backslash stays literal inside such a name. The script lexer and the command parser use the same rule.

Out of scope: `..`, `[*]` on objects, `keys`, `to_entries`, objects built in a column, `map`, slices, `CASE`, date functions, aggregates, and ORDER BY.

Implementation decisions:

| Point | Decision | Reason |
|---|---|---|
| Module | `connector/kafka/filter/expr.rs` holds the expression tree, its parser, and its evaluation. `filter.rs` holds the conditions | The filter and the SELECT columns use one evaluation |
| JSON reads | `Filter::needs` and the projection decode the key, the value, and the headers only when an expression reads their fields | A text filter does not parse JSON |
| Parentheses | A parenthesis followed by an operator holds a value, as in `(value.a + 1) > 2`. Other parentheses group conditions | One pass, without backtracking |
| `IN (…)` | `x IN (a, b)` is `x = a OR x = b`. Each comparison uses the same operand checks and timestamp conversion as `=` | Same results with `[*]` |
| Evaluation | A visitor generates one value at a time, including Cartesian products. Conditions stop when their result is known | No fixed list length or operation cap |
| Cancellation | Message evaluation runs in a blocking task. Paths, combinations, and quantifier items check the existing stop token | The runner can process Stop and its normal deadline while evaluation runs |
| Temporary memory | `filter/evaluation.rs` bounds generated text and projection buffers by the configured memory limit, per read. Reservations leave scope on success, error, and cancellation | List combinations must not allocate their full intermediate results |
| Delivery | The temporary ceiling also applies to live reads, export, and streaming. It does not cap total output | The user approves the same ceiling for temporary buffers in these modes |
| Failure | A temporary-memory failure ends the statement with a reason and a next action. No partial row leaves the projection | Do not silently truncate a list |

The temporary ceiling counts generated buffers, not the source message or its decoded JSON. Projection cells and the final row share this ceiling while they coexist. A visitor can stop before later combinations; it does not change result order, `COALESCE`, missing-field, or NULL rules. This delivery adds no setting or saved-format change.

## Kafka consumer group — 2026-11-11

The user asks for a consumer group setting, because some clusters permit only group ids with a specific prefix. The user approves the points below.
This entry changes point 9 of the Kafka entry only for the group id. A read still never subscribes and never commits.

| Point | Approved rule | Rejected |
|---|---|---|
| Value | The setting is the complete `group.id` of the reads. Without it, the reads use `sqln-read` | Prefix + `sqln-read`; prefix + separator + `sqln-read` |
| Places | The **Consumer group** field of the Kafka form, `consumerGroup` in `config.json`, and `group=` in the connection text | Only the form and `config.json` |
| Check | SQLN removes the spaces at the ends. An empty value means `sqln-read`. A space or a control character inside is refused | Kafka topic characters only; any text |

Implementation decisions:

| Point | Decision | Reason |
|---|---|---|
| Form and text | A field value replaces `group=` of the connection text. An empty field keeps `group=` | Same rule as the registry URL |
| Format | `consumerGroup` is optional and `version` stays 1. A file with the field cannot load in an earlier SQLN, because entries refuse unknown fields | Same rule as `sasl` and `schemaRegistry` |
| `GROUP <id>` | Still reads the offsets of `<id>`, not the setting | The command names its group |

## Max scan of Kafka reads — 2026-11-27

The user asks that each Kafka read without its own `SCAN` gets one, also with `LIMIT` or a start, for the console reads and the agent reads. Settings hold the default, and a connection can replace it. This entry changes point 4 of "MCP context, Kafka bounds, and IPC 3" in [ai-mcp-agent.md](ai-mcp-agent.md).

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Rule | A read with its own `SCAN` keeps it, also above the default. A read without `SCAN` gets `SCAN <max scan>` | A hard maximum that lowers an explicit `SCAN` | User choice |
| Places | **Kafka max scan** in Settings (`maxScan` of the preferences, Global and Project), and the **Max scan** field of the Kafka form (`maxScan` of the connection). The connection value wins. Not in the connection text | Only the connection | User choice |
| Value | Empty means 100000. A value must be a number greater than 0 | — | `SCAN 0` reads nothing |
| Console reads | The editor text stays the same. Messages shows `SQLN added SCAN N to this read`, as it shows an added `LIMIT` | No message | User choice. The result also shows that the read ended at its scan |
| Export and batch | An export and `sqln --execute` read all messages | The same `SCAN` | They also skip the row cap, and they read everything on purpose |
| Format | Both `maxScan` keys are optional and `version` stays 1. A file with them cannot load in an earlier SQLN, because entries refuse unknown fields | — | Same rule as `consumerGroup` |

## Kafka array operators — 2026-11-11

The user asks for array operators in the Kafka filter and SELECT, and approves the points below.
The filter applies to `WHERE` in SELECT, CONSUME, TAIL, and the filter field of the topic tab.

| Point | Approved rule | Rejected |
|---|---|---|
| Index | `value.items[0].sku`; `[-1]` is the last item. The form `value.items.0` stays | — |
| Any item | `value.items[*].sku = 'a'` is true when one item matches. Several `[*]` are permitted | `ANY(value.items.sku)` |
| Contains | `'vip' IN value.tags`, the same as `value.tags[*] = 'vip'` | `HAS`; `CONTAINS` with a path |
| Size | `SIZE(value.items) > 2` in `WHERE`; `SIZE(value.items) [AS n]` as a column | `LENGTH`; `ARRAY_LENGTH` |
| `[*]` in a column | JSON text of a list with the found values; no value gives NULL | Refuse `[*]` in SELECT |

Rules that follow from these choices:

- An index outside the list gives an absent field: the condition is false and the column is NULL.
- `[*]` with an operator needs one item that matches, so `value.items[*].sku != 'a'` is true when one item differs. `NOT value.items[*].sku = 'a'` means that no item is `'a'`.
- `[*]` and `IN` on a value that is not a list are false. `IN` compares with the type, as `=`.
- `SIZE` counts only lists. Another type or an absent field gives a false condition and a NULL column. `SIZE` compares with a number and refuses `[*]` in its field.
- Saved scripts keep their syntax. No saved format changes.

Known limitation: a field name that holds `[` is read as an index.

## Kafka SELECT queries

The user approves SELECT syntax and keeps CONSUME and TAIL as alternative forms.
This entry replaces the console and completion restrictions of the Kafka entry below.
The read-only scope, consumer isolation, filters, and Schema Registry rules stay unchanged.

| Point | Approved rule |
|---|---|
| Finite read | `SELECT * FROM topic` uses CONSUME: EARLIEST by default, with the high offsets captured at the start |
| Continuous read | End SELECT with `EMIT CHANGES` to use TAIL: LATEST by default, until Stop, with the existing live row cap |
| Start | `START AT EARLIEST`, `START AT LATEST`, `START AT OFFSET n`, or `START AT TIMESTAMP 'time'`; time follows the existing UTC and RFC 3339 parser |
| Clauses | `PARTITION n`, `START AT …`, `WHERE …`, `LIMIT n`, and `SCAN n` occur at most once, in any order after the topic; EMIT CHANGES comes last and refuses LIMIT and SCAN |
| Columns | `*`, or a comma-separated list of message columns and `value.field` paths, with optional `AS alias`; no expressions, joins, aggregates, or ORDER BY |
| Scripts | SELECT can span lines and ends at a semicolon or the end of the script. A semicolon is required before another statement. Native commands keep one command per line. A line that starts with `#` is a comment |
| Compatibility | Existing commands and saved scripts keep their syntax. Both forms use the same read execution. No saved format changes |
| Completion | SELECT offers message columns, catalog topics, and its clause words. It uses no database reads. See "Kafka schema field completion — 2026-11-13" for field paths |

Projection implementation: a whole column keeps its original value and type.
A JSON path preserves numbers, returns JSON text for objects and arrays, and returns NULL for absent or non-JSON values.
JSON field names keep their case. Aliases change column labels only.
SELECT accepts doubled quotes as well as the existing backslash escapes.

## Kafka and Schema Registry — 2026-11-10

The user asks for Kafka support with the Schema Registry and approves the points below.
This request reopens the rule "MongoDB, Redis, and Kafka stay out of the first version" only for Kafka.
It also reopens the rule "Search: literal text, no regular expression" only for the Kafka filter.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Scope | Browse and read, `TAIL`, consumer groups and lag | Produce messages; topic administration | User choice |
| 2. Library | `rdkafka` 0.39 (librdkafka) | `rskafka`; own client with `kafka-protocol` | User choice. librdkafka has groups, SASL, and offsets by time |
| 3. Formats | Avro, JSON Schema, and Protobuf | Avro only | User choice |
| 4. Console | Own commands, one for each line; `#` at the start of a line is a comment | Only the explorer | User choice. Same model as Redis |
| 5. Connection type | "Kafka", with `kafka://` and `kafkas://`, port 9092 | Two types | Same pattern as `redis://` and `rediss://` |
| 6. Bootstrap | One host. The metadata of the cluster gives the other brokers | A list of hosts | `NetworkTarget` has one address. YAGNI |
| 7. TLS | SQLN rule: local without TLS, remote verifies, `?sslmode=disable` turns it off; `kafkas://` always uses TLS | — | The password only travels without TLS by explicit choice |
| 8. Authentication | Optional user. With a user: SASL `PLAIN`, `SCRAM-SHA-256`, or `SCRAM-SHA-512` (default) | GSSAPI and OAuth | GSSAPI needs Cyrus SASL in C |
| 9. No consumer group | A read never subscribes and never commits (`enable.auto.commit=false`, `enable.auto.offset.store=false`), only `assign`. librdkafka needs a `group.id` for a consumer, so a read uses the local id `sqln-read`; without `subscribe` and without a commit the cluster never creates that group. `GROUP <id>` asks only for committed offsets (OffsetFetch) and never joins | No `group.id`; a random group; the legacy librdkafka API through FFI | Spike: librdkafka refuses `assign` without `group.id`. The user chose the local id. A test compares the group list before and after each read |
| 10. Read only | Each command only reads, so the read-only option changes nothing | — | The scope has no write |
| 11. Commands | `TOPICS`; `PARTITIONS <topic>`; `CONSUME <topic> [PARTITION n] [FROM earliest\|latest\|<offset>\|@<time>] [WHERE <filter>] [LIMIT n] [SCAN n]`; `TAIL <topic> [PARTITION n] [FROM …] [WHERE <filter>]`; `GROUPS`; `GROUP <id>`. Clauses come in any order, each one time | — | Covers the scope |
| 12. `CONSUME` | Reads to `LIMIT` matches (default: the row cap) or to the high offset of each partition at the start. `SCAN n` limits the read messages. **Stop** ends the read and keeps the rows. A filter, a scan limit, or a stop adds `Scanned N messages; M matched.` in **Messages** | — | A finite result, as a query |
| 13. `TAIL` | A continuous command of the 2026-11-09 contract: it grows until **Stop**; at the cap, the oldest rows leave. The default start is `latest` | — | Reuses the live flow |
| 14. Filter: place | In the client, after the decoding: console (`CONSUME`, `TAIL`) and the filter field of the topic tab, with the same syntax | Server filter | Kafka has no server filter |
| 15. Filter: syntax | `key = 'text'`; `key\|value\|headers CONTAINS 'text'` (literal, without case); `key\|value MATCHES 'regex'` (crate `regex`, linear time, size limit; `(?i)` ignores case); `value.a.b.0 <op> <literal>` with `=`, `!=`, `<`, `<=`, `>`, `>=` and a text, a number, `true`, `false`, or `null`; `AND`, `OR`, `NOT`, parentheses; `AND` before `OR` | — | User choice: the four kinds |
| 16. Filter: cases | A missing field or another type is false. A value without JSON makes a field condition false; `CONTAINS` and `MATCHES` read the text only when it is UTF-8. A syntax error or a bad regex refuses before the read, with its position | — | No surprise and no silent failure |
| 17. Columns | `partition`, `offset`, `timestamp` (UTC), `key`, `value`, `headers` (JSON), `schema` | — | Grid search and copy work |
| 18. Decoding | For key and value: the byte 0 and a configured registry decode with the schema to JSON; a failure keeps the bytes and adds a notice; else UTF-8 text; else bytes. A tombstone is `NULL` | — | Never loses data in silence |
| 19. Explorer | Two fixed schemas: `topics` and `groups`. `Enter` on a topic opens a tab with the last 500 messages that match the filter (empty: all), newest first, with **Refresh** and without pages. The read goes back in windows of offsets for each partition until 500 matches or the start. `Enter` on a group opens its lag | Pages of a global offset | An offset across partitions has no stable page |
| 20. Lag | `GROUP <id>`: `topic`, `partition`, `committed`, `end`, `lag`, `member`. `GROUPS`: `group`, `state`, `protocol`, `members` | — | Read only |
| 21. Schema Registry | Optional fields of a Kafka connection: URL, user, and password. `config.json` gets `"schemaRegistry": {"url", "username"}` and `"sasl"`; `version` stays 1. The password follows the **Credential** choice of the form: **Vault** writes it to the system vault, **Session** keeps it in memory | Always the vault | The user authorizes each secret storage |
| 22. Schema cache | By id, for the life of the connector | Expiry | A schema id never changes |
| 23. Completion | Fixed list of commands, then the topics or groups of the catalog, then the clause words. Replaced for field paths by "Kafka schema field completion — 2026-11-13" | — | Topics are few, unlike Redis keys |
| 24. AI and MCP | No AI completion and no MCP read for Kafka. Replaced by "Kafka AI completion, MCP reads, and topic filter — 2026-11-13" | — | Same rule as Redis |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Build | `rdkafka` with `cmake-build`, `ssl-vendored`, `libz-static`, `zstd`, and `tokio`. A clean build needs `cmake` and `perl` | System librdkafka | One binary, without a system library |
| TLS of librdkafka | OpenSSL inside librdkafka only. `ssl.ca.pem` gets the system roots of `connector::tls::system_roots` in PEM; a CA file goes to `ssl.ca.location`; a client pair goes to `ssl.certificate.location` and `ssl.key.location` | A second set of roots | The other connectors keep `ring`; the roots stay the same, except the bundled `webpki` roots, which have no certificate form |
| TLS of the registry | `reqwest` with `connector::tls::client_config` | — | One TLS rule |
| Calls that wait | Metadata, offsets, group list, and committed offsets run in `spawn_blocking` with a 10-second limit | The executor thread | The interface thread never waits |
| One read, one consumer | Each `CONSUME`, `TAIL`, and topic-tab round creates its own consumer and closes it on a blocking thread | One consumer for the session | A later read never receives a message of an earlier read |
| Stop | The read watches the stop token of the sink. The `Canceller` does nothing | — | The read ends by itself |
| Logs of librdkafka | Dropped | The `log` crate | They can hold names and values |
| Batch password | `--execute` asks for the password on the terminal for a Kafka user without a saved password | The Redis rule: ask only with a saved reference | A SASL user always needs a password |
| Refused password | A consumer receives the broker errors as events of its queue. A failed call reads these events, so an authentication error asks for a password | Only the result of the call | The call result says only "transport failure" |
| Registry password | The connector reads the vault one time, on a blocking thread, when the registry has a user and the session has no password. The vault account is the registry URL and the user | The vault in the app | The batch mode and the app use the same path |
| Shared word reader | `connector::words` holds the `redis-cli` word reader and `command_word`, for Redis and for the Kafka completion and tab text | A copy for Kafka | DRY |
| Topic tab | `TableTab::filter_only` shows only the filter field; `kind_word` names the tab `topic` or `group`; the page text is `SQLN.TOPIC <topic> <count> [WHERE …]`, which is not a console command | A new tab kind | Reuses the table tab |

### Known limitations of Kafka

| Subject | Limitation |
|---|---|
| Bootstrap | One host. If that broker is down, the connection fails |
| TLS roots | Without a CA file, librdkafka uses only the system roots |
| Authentication | No GSSAPI and no OAuth. The tests do not cover mutual TLS or SCRAM |
| Topic tab | The order is the producer timestamp. A partition with timestamps out of offset order can give an approximate list |
| `GROUP` | It asks the committed offsets of each partition of each topic, and one end offset for each committed partition. A large cluster makes many calls |
| Internal topics | Names that start with `__` stay out of the explorer. `TOPICS` lists them with `internal` set to `true` |
| Registry | A Protobuf message index outside the file, or an Avro logical type without a JSON form, keeps the bytes with a notice |
