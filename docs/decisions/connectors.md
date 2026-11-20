# Decisions — Connectors and sessions

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## Lock waits — 2026-11-20

The user asked for a panel that shows which session blocks which, and chose an own tab.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Place | `Scope::Locks` in a structure tab, with `L` in the navigator and `Ctrl+W L` in any panel | Columns in the sessions tab | The tab shows the lock and the object, and keeps pages, search, copy, and export |
| Row | One row for each pair of a waiting session and a blocking session | One row for each session | A session can wait for several others |
| Columns | The blocking session, its user, and its query take the places of the sessions tab | Waiting session first | The signals of `q` and `K` read those places, so one path serves both tabs, and the useful action frees the wait |
| PostgreSQL | `pg_blocking_pids` with the lock of `pg_locks` that is not granted | Only `pg_locks` joins | The function follows the lock queue and the parallel workers |
| MySQL and MariaDB | `sys.innodb_lock_waits` | `performance_schema.data_lock_waits`; `information_schema.INNODB_LOCK_WAITS` | One view answers on MySQL 5.7, MySQL 8, and MariaDB 10.6 or newer; each table exists only in some of them |
| Reader | The session that reads the list is not a blocker | Show it with a mark | A session must not end itself, as in the sessions tab |
| Refresh | Manual with `R`, and automatic after an action | By interval | Same rule as the sessions tab |

## Error kinds of the connector contract — 2026-11-19

The user approved a plan of internal improvements. This entry covers the typed errors of the connector contract.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Kind | `ExecError` and `ConnectError` get `kind: ErrorKind`. `ExecError::new` keeps `Other`, so a site without a known kind needs no change | A typed error per driver | The layers above the drivers need one shared vocabulary |
| Source | Each driver reads the kind from codes: SQLSTATE class, MySQL error number, SQLite and DuckDB result code, Redis error kind, Databricks error code or HTTP status | Matching the message text | Text changes with the server version and the language |
| Runtime | A `Connection` error closes the session after the statement, as a closed connection does | Rely on `reset` alone | The session state is unknown, and a driver may not detect the closed socket |
| Password | `ConnectError::password_refused` stays a separate field | Derive it from `Auth` | `pg_hba.conf` and similar rules are `Auth`, but a new password does not change them |
| Interface | No visible change yet. A later step may use the kind for the next action of an error message, after a decision of the user | Change the messages now | Visible behavior needs a decision of the user |
| `async-trait` | Keep it | Native `async fn` in traits | The contract is used as `dyn Connector`, `dyn Session`, and `dyn Canceller`. Native async traits are not object safe, so the change would need a boxed future per call anyway |
| Canceller | `Canceller::cancel` keeps `Result<(), String>` | `ExecError` | The runner only waits for the request to end. No caller reads the error |

## Databricks SQL connector — 2026-11-15

The user asked for SQL queries on Databricks. The user chose the scope below. This entry adds `sqln-databricks` to the driver list of "Driver crates, connector contract, and runtime".

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Protocol | The SQL Statement Execution API 2.0 over HTTPS, with `reqwest` | ODBC (needs the Simba driver), Thrift | No Rust driver exists. The workspace already uses `reqwest` |
| Authentication | A personal access token only. It is the password of the connection, in the vault or in the session | OAuth | User choice |
| Warehouse | The `warehouse` field (`warehouse=<id>` in the connection text) is required. The id has only ASCII letters and digits, because it goes into a request path | A full HTTP path | One value, and no path text from the user |
| Catalogs | The explorer shows the schemas of all catalogs. Each schema name is `catalog.schema`, also for the start catalog. The **Catalog** field is the catalog of a session without a schema | Plain names for the start catalog, as DuckDB | User choice to navigate between catalogs. The structure queries need the catalog of each schema |
| Metastore | Catalog reads use `system.information_schema` (Unity Catalog). `hive_metastore` does not show in the explorer, but its tables still run in a console | `SHOW` commands for each catalog and schema | User choice. One query for each read |
| Scope | Read and run only: no transactions, no grid edit, no parameters, no plan, no diagram, no compare | Full feature set | User choice |
| Session state | The API keeps no session. Each request carries the catalog and the schema of the session. `SET` and `USE` in a console do not reach the next statement | A session of the warehouse | The API has no session |
| Results | `EXTERNAL_LINKS` with `ARROW_STREAM`, one part in memory at a time, inside the memory limit. The download of a link never sends the token. An expired link gets a new link one time | `INLINE` with its 25 MiB limit | User choice |
| Cancel | `POST /statements/{id}/cancel`. A cancel that arrives before the id waits for it. The first request waits 5 seconds at most | `wait_timeout: 0s` | Fewer requests for a short statement |
| Read only | The connector refuses a write with the shared check, as DuckDB does | Nothing | The API has no read-only session |
| Tests | A fake HTTP server in `sqln-testkit` (`http` feature). The AI tests use it too | A real workspace | User choice. No container exists for Databricks |
| Form | **Databricks** comes after MySQL in the **Type** choice. It has **Catalog**, **Warehouse**, and **Token**, and no **User** | Last in the choice | The SQL servers stay together. `Left` from PostgreSQL still reaches Kafka, DuckDB, Redis, and SQLite; `Right` needs one more step to SQLite |

## SQLite in memory — 2026-11-06

The user asks for `:memory:` SQLite databases. The initial plan rejected them in the first version; the explicit request is the new fact, and this entry replaces that rejection.

| # | Point | Approved rule | Rejected | Reason |
|---|---|---|---|---|
| 1 | Entry | The file field of the form and the command line accept exactly `:memory:`, without case. `file:` URIs and `mode=memory` stay refused | Any URI form | One text form keeps the validation simple |
| 2 | Pool | The pool holds exactly one place, and no idle timeout closes it, because a new session starts empty | A shared cache between sessions | The shared cache has locking rules of its own |
| 3 | Session | The session returns clean instead of discarded, so temporary tables persist for the session. A disconnect and the end of the application lose the database, which the form and the README say | Refuse the disconnect | The user asked for a scratch database, not a file |
| 4 | Source type | `SqliteTarget` holds an enum of the file and the memory source, and `:memory:` never reaches the driver as a path | A magic path value | The type carries the rule |
| 5 | Configuration | `config.json` stores `":memory:"` as the file of the connection. No saved file can hold it today, so no migration is needed | A new field | The text round-trips through the form |
| 6 | Queue | Compare, structure, and console reads of the connection share the one session in arrival order | A second session | Data consistency wins over parallel reads |

## Search path of a console schema in PostgreSQL — 2026-11-04

The user reported `type geography does not exist (42704)` on a query that works in DataGrip. A console with a chosen schema sent `SET search_path TO "<schema>"`, so `public` left the path, and with it the types and functions of PostGIS. A console gets a schema from the selector, from a saved script, or from the compare.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Console path | The chosen schema first, then the default path of the session (`"$user", public`, or the path of the role and the database) | `"<schema>", public`; only the schema | User choice. Extensions in `public` or in another schema of the default path stay visible. A name without a schema still looks in the chosen schema first, and `CREATE` without a schema still uses it |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Statement | `set_config('search_path', concat_ws(', ', <schema>, NULLIF(current_setting('search_path'), '')), false)` | A fixed text of the default path | The default path can change by role or database. An empty default path gives only the schema |
| Compare | `RunOptions::only_schema` keeps `SET search_path TO "<schema>"` for the reads of a compare | The same path as the console | The compare needs the names of other schemas with their schema, such as `public.geography`, for its sync script |
| Repeats | The cleanup of the session restores the default path, and a reserved session does not receive the context again | Remove the schema from the default path first | The path does not grow. A repeated name in the path does no harm |

## Visible schemas of a connection — 2026-11-04

The user asked to mark, in the configuration of a connection, the schemas of PostgreSQL or the databases of Redis that the explorer shows. The user chose points 1 to 4 and approved points 5 to 13 with the plan.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Form | A field with a list that SQLN reads from the server, with a search and a mark on each row. The field keeps the names, and the user can also write them. An empty field shows all | A text field only | User choice |
| 2. Effect | The whole connection: tree, tree filter, schema selector of the console, compare dialog, and completion see only the marked schemas. A statement can still use any schema | Only the tree | User choice. Redis also skips the `SCAN` of a hidden database |
| 3. Current schema | Always visible: `current_schema()` of PostgreSQL, `DATABASE()` of MySQL, the database of a Redis connection | Visible only when marked | User choice. The console runs there, and completion reads it |
| 4. Open connection | The field stays in the form of a connected connection. **Save** reads the catalog again and keeps the sessions | Change only after a disconnect | User choice |
| 5. Databases | PostgreSQL, MySQL, and Redis. SQLite has no field | Only PostgreSQL and Redis | In MySQL a database is the schema |
| 6. Marked empty Redis database | It appears. Without a list, the old rule stays: an empty database appears only when it is the current one | Only a database with keys | The user marked it |
| 7. Redis list | The databases with keys, the current database, and the marked databases. Another number is written in the field | `0` to `N - 1` from `CONFIG GET databases` | A managed service often refuses `CONFIG` |
| 8. Patterns | Exact names only, with case | `tenant_*` | Not necessary now |
| 9. Tree note | `(filtered)` after the name of a connection with a list | No note | The user sees why a schema is not there. `Loading` and `Failed` come first |
| 10. Place | First field of **Options**. The label is **Schemas** for PostgreSQL and **Databases** for MySQL and Redis. The `...` button and `Ctrl+O` open the list | Own section | The form of an open connection keeps two sections |
| 11. List keys | Write to filter, `Up` and `Down`, `Space` marks, `Enter` writes the field, `Esc` cancels. A click marks a row | Other keys | Same keys as the other lists |
| 12. Field text | Names with commas between them, such as `public, sales`. Spaces, empty names, and repeats go away. A Redis name must be a number | A list of lines | One line, as the other fields |
| 13. `config.json` | `"schemas": ["public", "sales"]`. A missing or empty list shows all. `version` stays 1 | A new version | Same rule as `idleTimeoutSeconds` |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Read of the names | The path of **Test**: one short session with the definition of the form, also with the connection open. `ExecEvent::ConnectionTested` carries the names | The pool when the connection is open | One path. The password requests of **Test** stay as they are, and the list reads the server of the form |
| Filter | `Session::read_catalog_of` reads the catalog and calls `Catalog::keep_schemas`. Redis replaces it and reads only the databases of the list | A `WHERE` in each catalog query | One rule for each database. The catalog of PostgreSQL and MySQL is one query |
| Names | `Session::read_schema_names` reads the catalog. Redis reads only `INFO keyspace` | One query for each database | Only Redis has a cost in the full read: one `SCAN` for each database |
| Window | `draw_list_window` of the palette, over the form | A list below the field | The list can hold many names |

Known limits:

- A name with a comma does not fit in the field.
- PostgreSQL and MySQL still read the relations of each schema. The filter reduces the memory and the screen, not the query.
- A new schema does not appear until the user marks it.
- The list opens one more session, outside the pool limit, also with the connection open.
- The list needs a valid form. A Redis field with a name that is not a number stops it.
- An older SQLN refuses a file with `schemas`.

## Idle limit and connection form with a sidebar — 2026-11-03

The user asked for an idle limit in the connection form, with 300 seconds as the default. Before this change, SQLN never closed a session for inactivity. Only the server or the network closed it.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. What closes | Only the sessions that wait in the pool. A console with an open transaction keeps its session | Also the session of a transaction, with a rollback by the server | A close would lose work without a commit and without a question |
| 2. Start of the count | Each session counts from its return to the pool | The connection counts from its last use, and all sessions close together | The pool keeps the session in use and closes the others |
| 3. Value 0 | No limit, as in the SQL limit | Refuse 0 | The same rule as the SQL limit |
| 4. Saved connections without the field | 300 seconds | No limit | It is the default of the request |
| 5. Sources | All, also SQLite and Redis | Only the network sources | One rule |
| 6. Message | None. The explorer mark changes, and the next use opens a session | A message in the status line | The close needs no action of the user |

The new field did not fit: the largest form (a network source with the choice of a scope) had 21 fields and filled 80 x 24 exactly. The user then reopened the row "Connection form" of "nvim-style interface" (four arrangements, no scroll), and approved a sidebar with sections, as in Settings, the import, and the export.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Sections | The five groups: General, Address, Credentials, Security, and Options | Split Options into Limits and Appearance | The groups already exist, and Options fits |
| 2. Section without fields | Out of the sidebar | Shown as disabled | SQLite has no Credentials and no Security |
| 3. Error in another section | The form opens the section of the field and puts the focus on it | Only the note, as in Settings | The required fields, such as Host and User, live in other sections |
| 4. Buttons | Fixed at the bottom of each section | In one section | As in Settings |
| 5. Form of a connected connection | Also the sidebar: General and Options | One page | User choice |
| 6. Keys and mouse | As in Settings: `Tab` walks the sidebar, the fields, and the buttons; `Up` and `Down` on the sidebar change the section; a click opens a section | Own keys | One model for the dialogs |
| 7. First section | General, with the focus on Type, or on Name in an edit | The last section used | The focus of before stays |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Timer | One task of `Manager` for each pool, with a `WeakPool`. It sleeps until the next idle session reaches the limit | A check at the next use | A check at the next use does not free the server |
| Event | `ExecEvent::IdleClosed`, which redraws the explorer and opens a display-only form for changes, as `Disconnected` does without its message | `Disconnected` | That event writes "Disconnected." |
| Error field | `ConnectionForm::definition` returns `Invalid { field, message }` | A second check that finds the field | One check |
| Height | All sections keep the height of the largest section of the form | The height of each section | The window does not move between sections |
| Width | 76 columns: the sidebar, the label, and the longest choice | 72 | The value needs 26 columns |
| Sidebar code | The form uses the functions of `ui/settings.rs` | Own copies | One drawing for the dialogs |

### Known limitations of the idle limit

- The cleanup of SQLite already closes the connection at each return to the pool, so SQLite never has an idle session, and the limit has no effect there.
- A change of the limit needs a disconnection, as each change of the definition does, because the pool keeps its limit.

## Query plan and server sessions — 2026-11-02

The user compared SQLN with DataGrip and asked for a plan of the two execution gaps. This request reopens the rule "Own view for `EXPLAIN`" of the first version.

Query plan:

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Screen | Result tab of the console, with a tree in the grid | Own tab in the main strip; own widget with expand and collapse | Reuses search, copy, the row window, and export |
| 2. Commands | `Ctrl+W E` for the estimate, and `Ctrl+W A` for a real run | Estimate only | The real time is the most useful data |
| 3. Real run of a statement that can write | Ask, and then run | Refuse; run in a transaction and undo | `is_plain_read` counts each unknown function as a write, so a refusal blocks valid reads. A transaction adds another run path |
| 4. Selection with several statements | Refuse, with a reason | One plan per statement | Simpler, and a `SET` in the selection would give an error |
| 5. History | A plan stays out | Save the text with `EXPLAIN` | The history keeps the queries of the user |

Server sessions:

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 6. Place | Tab in the main strip, with `S` in the navigator and in the palette | Modal window | Reuses the structure tab |
| 7. Databases | PostgreSQL and MySQL. SQLite refuses with a reason. Redis and Valkey stay out | Redis and Valkey with `CLIENT LIST` | `CLIENT LIST` works in a console, and a tab needs an own reader |
| 8. Reach | Each session that the database user can see | Current database only | The `database` column separates them |
| 9. Actions | `q` cancels the query, and `K` ends the session | End only | A cancel keeps the connection of the other person |
| 10. Question | Always | Production only | One wrong key ends the work of another person |
| 11. Read-only connection | Refuse both actions | Allow | The database does not refuse them, so SQLN keeps the promise |
| 12. Session of the list | Hide it | Show it with a mark | A session must not end itself |
| 13. Refresh | Manual with `R`, and automatic after an action | Also by interval | An interval makes a continuous read on the server |

Implementation decisions:

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Run path | `RunTarget` carries the statements or a plan through `run`, `RunAnyway`, and `RunConfirmed` | Own dialog actions for a plan | One path keeps the checks of a run: config, busy, and unknown transaction |
| Tree | Built when the result ends | Build inside the connector | The connector stays the same, and a read that fails keeps the text |
| Sessions tab | `Scope::Sessions` in `Structure` | New tab type | The tab keeps deadline, stop, pool queue, budget, password, and pages |
| Signal | `PageKind::Signal` through the task of the tab | A temporary connection | The tab already has the path, and the database reports the error there |
| MySQL list | `information_schema.PROCESSLIST` | `SHOW PROCESSLIST`; `performance_schema.processlist` | `SHOW` refuses `LIMIT`, and `performance_schema` needs a grant |
| Reload key | `R` serves each tab with `filters = false` | Only the sessions tab | A structure tab had only the mouse |

### Known limitations of the plan and the sessions

- MariaDB has no `FORMAT=TREE`, so the error of the server appears. A manual `EXPLAIN` still works.
- `FORMAT=TREE` needs MySQL 8.0.16 or newer.
- On MySQL 8.4, a plan of a write on one table gives only `<not executable by iterator executor>`, as one node.
- On MySQL 8.4, `EXPLAIN ANALYZE` of an `UPDATE` with a `JOIN` kept no change in a test. PostgreSQL keeps the change. The question says that a change can stay.
- A column of a plan can pass the width of the grid. `r` shows the row, and `Enter` shows the value.
- `information_schema.PROCESSLIST` is deprecated on MySQL 8.4. `tests/servers.rs` proves that it answers.
- After `K`, a session can stay in the list for an instant. `R` reads it again.
- A `KILL` can use the session that it ends, if the pool gives that session. The tab shows a closed connection, and the pool opens another.
- Without `pg_read_all_stats` or `PROCESS`, the database hides the query text of other users.
- Without `pg_signal_backend` or `CONNECTION_ADMIN`, the database refuses the action, and the tab shows the error.

## MySQL client key — 2026-11-02

`mysql_async` 0.37 reads client keys in PKCS#1, PKCS#8, and SEC1 form. The user removed the SQLN check that accepted only PKCS#1.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Key form | The driver decides | Keep the SQLN check | The check refused keys that the driver reads |
| Bad key | The connection fails with the error of the driver | A message of SQLN | The form of a key has no SQLN limit to explain |

## Redis and Valkey — 2026-10-31

**Defined:** the user asks for Redis and Valkey support and approves the 11 recommendations below.
This request reopens the rule "MongoDB, Redis, and Kafka stay out of the first version" only for Redis and Valkey.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| 1. Console input | One command per line, as in `redis-cli`; `#` at the start of the line is a comment | `;` ends the command | The `redis-cli` text works without change |
| 2. Explorer | Databases and up to 1,000 keys per database, with `SCAN`; `Enter` opens the value in a tab | Only databases with a paginated list; only databases | Allows browsing without writing commands |
| 3. Read only | Ask `COMMAND INFO` and refuse commands without the `readonly` mark; without a server response, refuse | Fixed list; remove the option | Covers Valkey and modules, and the server has no read-only session |
| 4. Transactions | No manual mode; `MULTI` and `EXEC` work in a script | Tie the manual mode to `MULTI` and `EXEC` | Inside `MULTI`, a read answers `QUEUED`, without data |
| 5. Continuous commands | Refuse `SUBSCRIBE`, `PSUBSCRIBE`, `SSUBSCRIBE`, and `MONITOR` (replaced by "Continuous commands — 2026-11-09") | Continuous flow in the grid | The console has no continuous flow |
| 6. Destructive commands | Without confirmation | Ask for confirmation | `DROP` also does not ask for confirmation |
| 7. TLS of `redis://` | SQLN rule: local without TLS, remote verifies; `?sslmode=disable` turns it off; `rediss://` always uses TLS | `redis://` never uses TLS | The password only travels without TLS by explicit choice |
| 8. Topology | Single server only | Cluster; Cluster and Sentinel | Cluster and Sentinel change the pool |
| 9. Result in the grid | Value in one cell, list in rows, map in the `field` and `value` columns, nested value as text | Everything as text | Uses the grid; needs RESP3 |
| 10. Autocomplete | Command names, with a fixed list | Also key names | The key list can be partial |
| 11. Connection type | One "Redis/Valkey" type, with `redis://`, `rediss://`, `valkey://`, and `valkeys://` | Two types | The protocol is the same |

These choices follow the rules above on the points the rules left open.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Driver | `redis` 1.8, with `MultiplexedConnection` on its own stream | Driver TLS | The driver TLS does not use the `connector::tls` roots; the own stream keeps one TLS rule for all sources |
| Database in the form | Number, validated in the form, in the URI, and in `config.json` | Free text | Redis has no database names |
| User | Optional only for Redis | Require `default` | A server without ACL has no user |
| Identity commands | Refuse `AUTH`, `HELLO`, and `RESET` | Allow | The session returns to the pool, and another console would receive the new login |
| Replication | Refuse `SYNC` and `PSYNC` with the continuous commands (replaced by "Continuous commands — 2026-11-09") | Allow | Those commands also do not end |
| Tests | `tests/redis.rs`, separate from `tests/servers.rs` | Add to `tests/servers.rs` | That file repeats SQL on each server |

### Known limitations of Redis and Valkey

| Subject | Limitation |
|---|---|
| Read only | The protection stays only in the client. `PING`, `INFO`, `ECHO`, `TIME`, `SELECT`, `CONFIG`, `CLIENT`, `OBJECT`, `MEMORY`, and `XINFO` stay refused, because the server does not mark those names as `readonly`. The database selector keeps working |
| Stop | Only for a command that waits, such as `BLPOP`. In another command, SQLN closes the connection, and the server ends the command |
| Slow command | A long `KEYS *` holds the server, so the stop request may wait 5 seconds without a response |
| Cap and budget | The driver reads the whole response before the row cap and the grid budget. A huge `LRANGE k 0 -1` uses memory before the cut |
| Explorer | Shows up to 1,000 keys in the `SCAN` arrival order, sorted by name. Empty databases do not appear, except the current database and the databases of the list of the connection (2026-11-04). A key that is not UTF-8 does not appear and marks the list as partial. A command does not update the tree; use **Refresh** |
| Value pages | `HSCAN`, `SSCAN`, and `XRANGE` have no offset, so each page repeats the previous read, with the SQL `OFFSET` cost. A hash or set that changes can repeat or skip items |
| Module types | A module type, such as JSON, does not open in a tab. Use the console |
| `MULTI` | Valid only inside the same execution. Running `MULTI` alone discards the queue at the end of the execution |
| Topology | No Cluster and no Sentinel. A `MOVED` response appears as an error |
| Version | RESP3 needs Redis 6 or newer. An older server fails to connect |
| Autocomplete | The command list is fixed, without module commands and without keys |
| Command line | A value does not take two lines. Use `\n` in double quotes |

### Pending items of Redis and Valkey

- Validate in a real terminal: form, explorer, value tab, and vault.
- Test a Unix socket with a real server. Docker on macOS does not share that socket, and the code has only a build test.
- Test the Stop of a command that does not wait. A long Lua script would hold the other tests on the same server.
- Test a server with a restricted ACL, without `CLIENT ID` or without `COMMAND INFO`.
- Test managed services, which can rename commands.

## Session and connection

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Session state | Database or schema selector per console | Fixed connection per console | Five consoles would occupy the pool |
| Connect | On first use | Explicit action or at startup | Asks for the password only when needed |
| Connection loss | Replace idle ones and fail the reserved ones | Repeat the command | Repeating can duplicate changes |
| Failed cleaning | Drop the connection | Recover | Simpler and safer |
| Connection name in the bar | Shorten the name | Shorten the state | The user needs to always see the transaction and the execution |
| Pool queue | Without a limit | With a limit | Each console has one execution |
| Late cancellation | Wait for the request before reusing the session | Reuse at the end of the statement | A late request could hit the next statement |
| End of the request on PostgreSQL | Read until the server closes the request connection | Driver `cancel_query` | It returns at write time, and the `UPDATE` after a `SELECT` at the cap was cancelled in a load test |
| `sslmode=require` | Verify the certificate | Encrypt without verifying, as in `libpq` | The plan forbids ignoring the verification |
| `sslmode=prefer` and `allow` | Refuse the text | Accept | Those modes can use a connection without TLS in silence |
| Default CA | System and `webpki-roots` | Only one of the sources | Company CAs stay in the system; `mysql_async` already brings the second one |
| Trust | Confirm per destination | Confirm per project | Follows the SSH model and does not depend on detecting clones |
| Read only | Field per connection, applied by the database | Confirm `DELETE` without `WHERE` | The database applies the rule |
| Table filter | A single statement | Accept any text | The drivers accept several statements |
| Cleaning on MySQL | `Conn::reset` and back to the database of the connection; without a database, close the session | Only `Conn::reset` | `COM_RESET_CONNECTION` keeps the database, and a `USE` would pass to the next console |
| Cleaning on PostgreSQL | The steps of `DISCARD ALL` without `DEALLOCATE ALL`, and `DEALLOCATE` of each `PREPARE` of the user | `DISCARD ALL` | `DISCARD ALL` removes the type lookup that `tokio-postgres` keeps in its cache, and the next enum read failed with `26000` |
| `DISCARD ALL` and `DEALLOCATE` of the user on PostgreSQL | Run them; then the columns are text, and the cleaning closes the session | Refuse them; handle `26000` in the `prepare` | In a transaction, the failed lookup already stops the transaction |
| `CLIENT REPLY` on Redis | Refuse | Accept | The session waits for each answer, and the execution had no end before Stop |
| Cleaning on Redis | `DISCARD`, `CLIENT SETNAME ""`, `TRACKING`, `NO-EVICT`, and `NO-TOUCH` `OFF`, `UNWATCH`, and `SELECT`, in one round trip | Refuse those `CLIENT` commands | The name and the modes passed to the next console |
| Selector with an open transaction | Refuse the change | Change with `SET` or `USE` | A reserved session does not receive the context, and a transaction with an error refuses `SET` |
| Disconnect with open work | Refuse and name the console | Ask and stop the console | Fewer states; the user resolves the transaction with the existing controls |
| Disconnect with a catalog read | Wait for the session for up to 5 seconds | Report the end right after closing the idle ones | The message would be false, and the form stayed stuck in the rename mode |
| Removed connection | The console stays open, without a connection | Close the consoles | The text is not lost, as in the scripts without connection |

## DuckDB — 2026-11-07

The user asked for DuckDB support. The delivery is the engine and the essential interface: the connector, the connection form, `config.json`, the command line, and the icon. Schema compare, the ER diagram, the DDL window, `EXPLAIN`, grid editing, and the foreign-key reads stay out.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Driver | The `duckdb` crate `1.20000.1` with `default-features = false, features = ["bundled"]` | Default features | The default of `libduckdb-sys` downloads a prebuilt library at build time; the bundled source builds offline and reproducibly, at the cost of a long C++ build |
| Model | One file or one database in memory, as SQLite | A network server | DuckDB is embedded; the model and the thread-per-session bridge of `connector/sqlite.rs` fit it exactly |
| Sessions | The connector opens the database once in an owner thread, and each session clones that connection with `try_clone` | `Connection::open` for each session | Two `open` calls on one file make two isolated instances: a write of one session stays invisible to the other, and the writes of the pool are lost |
| Owner start | The lock of the first open spans the wait, so concurrent first connections never open the file twice | A check after the open | Two opens of one file race for its lock, and one of them fails |
| Dead owner | A file clears the dead owner and starts a new one once; a database in memory keeps the failure | A restart for every source | The thread is the only holder of the database: a database in memory dies with it, and a restart would hand out an empty database without a word |
| In memory | `single_session` and no idle close, as an in-memory SQLite database | A pool of sessions | A new connection starts with an empty database |
| Result read | Arrow through `stream_arrow`, with `array_value_to_string` for each cell | `Row::get::<String>` | The driver refuses `String` for a list, a struct, or a map, and an owning read materializes every row before the row cap applies |
| Change count | `execute` when the prepared statement reports no result column, and also when the statement changes rows without `RETURNING` | Only the column count, as the SQLite connector does | DuckDB answers every row change, also under `WITH`, with one `Count` column that is already in the prepared statement, and it would reach the grid as a user result |
| Current schema | The catalog reads `SELECT current_schema()` of the session, as PostgreSQL does | A fixed `main` | A console changes the schema with `SET schema`, and the catalog is read on the session that holds it |
| Transaction state | The session keeps it from the statements it runs | `Connection::is_autocommit` | The driver reports that function as a constant `true` |
| Aborted transaction | `Failed`, and only `ROLLBACK` ends it | `Open` | DuckDB refuses every statement after an error inside a transaction, as PostgreSQL does |
| Read-only | `access_mode = READ_ONLY` when the file opens, from the definition | A check of each statement only | The engine enforces it, and a file cannot open read-write and read-only at once |
| Read-only of the settings | Refuse a data-changing statement in the connector | Open the file read-only | A DuckDB connection cannot change its access mode after it opens, so the global preference, the agent, and a reservation need a check outside the engine |
| External files | Refuse a `csv`, `tsv`, `parquet`, `json`, `jsonl`, or `ndjson` file, and name `read_csv`, `read_parquet`, or `read_json` | Open it, as the driver does | `Connection::open` of one of these starts a database in memory without a word, so a table of the session disappears at the end |
| Command line | The `duckdb:` scheme, with `duckdb:` and `duckdb::memory:` in memory | Detect a file extension | A bare path stays a SQLite file, which the current behavior requires |
| Form order | DuckDB at the end of the type choices | Next to SQLite | The tests of the form reach a kind by counting arrows |
| Attached databases | A schema of an attached database keeps the `catalog.schema` name, and the catalog, the column read, the structure, the completion, and the templates filter by catalog | Hide the schemas of the attached databases | `duckdb_schemas`, `duckdb_tables`, and `duckdb_views` list every catalog of the session, and a schema name alone cannot reach a table of one of them: the open wrote `FROM "public"."player"` and the engine answered that the schema does not exist. The anchor is the catalog of the connection, read on a clone that ran no statement, because `SET schema` changes the current database of a session and a pooled read can run on any session. A local schema whose own name has a dot does not open, because every read splits the name at the first dot |
