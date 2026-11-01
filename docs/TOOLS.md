# SQLN — How other tools solve the same points

This research was done on 2026-10-30.
It comes from public documentation and does not replace the tests in [RESULTS](RESULTS.md).
Some sources are old, and the current behavior may differ.

## Console results

| Tool | Solution |
|---|---|
| Harlequin | Cap of 500 rows, applied at the database; does not truncate silently |
| lazysql | Cap of 1,000 rows in the SQL editor; the value 0 removes the cap |
| DataGrip | 500 rows per page, with a button for the next page |
| DBeaver | Fetches the next part at the end; with `LIMIT`, it runs queries with larger limits |
| pgAdmin | Optional server-side cursor, which needs a transaction and does not show the total |
| psql | `FETCH_COUNT` fetches in groups; the query may fail after it shows rows |

## Table view

| Tool | Solution |
|---|---|
| lazysql | Fetches one page plus one row; shows `300+ rows` |
| pgAdmin | In 2020, it sorted by primary key to stabilize pages |
| Rainfrog | Preview of 100 rows |
| DataGrip | Search and local filter act only on the current page |

## Cancellation

| Tool | Solution |
|---|---|
| PostgreSQL | The protocol opens a new connection, does not answer, and may have no effect |
| `mysql` client | `KILL QUERY`, then `KILL CONNECTION`, through a second connection |
| DataGrip | Cancels; after 10 s it offers to disable the source, and the query keeps running at the database |

## Connection and session

| Tool | Solution |
|---|---|
| DataGrip | Connects on the first action; offers keep-alive and shared, dual, or dedicated session modes |
| DataGrip | Schema selector in the console toolbar |
| DBeaver | Keep-alive and an optional separate connection per editor |
| psql | The prompt shows `?` when the transaction state is undetermined |

The sources disagree about the default session mode of DataGrip 2026.2.

## Types and files

| Tool | Solution |
|---|---|
| DBeaver | Native database format, according to the driver |
| DataGrip | Client-side formatting, with a configurable time zone |
| Rainfrog | SQLx with specific types; does not accept geometric types |
| DataGrip | Separates `dataSources.xml` from `dataSources.local.xml`; consoles stay outside the project |
| DBeaver | `.dbeaver/data-sources.json` and credentials in a separate encrypted file |

## Trust

| Tool | Solution |
|---|---|
| SSH | Confirms a new key, refuses a changed key, and keeps the record outside the project |
| direnv | Blocks a new `.envrc` until `direnv allow` |
| VS Code | Opens unknown folders in restricted mode |

## Similar stack

Rainfrog is the closest code reference.
It uses `sqlx`, `ratatui`, `crossterm`, `keyring`, `tokio`, and `ratatui-textarea`.

## Sources

- [DataGrip: Sessions](https://www.jetbrains.com/help/datagrip/managing-connection-sessions.html)
- [DataGrip: Query consoles](https://www.jetbrains.com/help/datagrip/query-consoles.html)
- [DataGrip: Run queries](https://www.jetbrains.com/help/datagrip/run-a-query.html)
- [DataGrip: Rows](https://www.jetbrains.com/help/datagrip/rows.html)
- [DataGrip: Data extractors](https://www.jetbrains.com/help/datagrip/data-extractors.html)
- [DataGrip: CSV Formats](https://www.jetbrains.com/help/datagrip/settings-tools-database-csv-formats.html)
- [PostgreSQL: Protocol Flow](https://www.postgresql.org/docs/current/protocol-flow.html)
- [PostgreSQL: Connections](https://www.postgresql.org/docs/current/runtime-config-connection.html)
- [PostgreSQL: psql](https://www.postgresql.org/docs/current/app-psql.html)
- [MySQL Bug #28141](https://bugs.mysql.com/bug.php?id=28141)
- [DBeaver: Separate connections](https://dbeaver.com/docs/dbeaver/Separate-Connections/)
- [DBeaver: Data formats](https://dbeaver.com/docs/dbeaver/Managing-Data-Formats/)
- [pgAdmin: Query Tool](https://github.com/pgadmin-org/pgadmin4/blob/master/docs/en_US/query_tool.rst)
- [Harlequin 2.9.0](https://newreleases.io/project/github/tconbeer/harlequin/release/v2.9.0)
- [Harlequin 2.11.0](https://newreleases.io/project/github/tconbeer/harlequin/release/v2.11.0)
- [lazysql](https://github.com/jorgerojas26/lazysql)
- [Rainfrog](https://github.com/achristmascarl/rainfrog)
- [ssh_config](https://man.openbsd.org/ssh_config)
- [direnv](https://direnv.net/man/direnv.1.html)
- [VS Code: Workspace Trust](https://code.visualstudio.com/docs/editing/workspaces/workspace-trust)
