# Local MCP assistant

SQLN provides MCP through `sqln mcp`. Keep the SQLN terminal application open.
The command starts a stdio bridge, not another terminal interface or an independent database client.
The bridge uses private Unix domain sockets on macOS and Linux. There is no HTTP listener.

## Configure a client

Install the current SQLN executable and put it on the client's `PATH`.
Use an absolute executable path if the client does not inherit your shell's `PATH`.

### Claude Code

Add one user-scoped server:

```sh
claude mcp add --transport stdio --scope user sqln -- sqln mcp
```

Equivalent MCP server entry:

```json
{
  "mcpServers": {
    "sqln": {
      "command": "sqln",
      "args": ["mcp"]
    }
  }
}
```

### OpenCode

Merge this entry into the global `~/.config/opencode/opencode.json` file:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "sqln": {
      "type": "local",
      "command": ["sqln", "mcp"],
      "enabled": true
    }
  }
}
```

Do not replace unrelated client settings. No connection definition belongs in either MCP configuration.

## Select an instance and allow access

1. Open SQLN as usual.
2. Ask the assistant to call `list_instances`.
3. Select the exact instance by its UUID, working directory, or project.
4. Ask it to call `request_access` with that `instance_id`.
5. Click **Review** in the pending MCP access notification, or open the palette with `Ctrl+W :`.
6. Select **Agent sessions: allow or revoke access** if you use the palette.
7. Select the pending session.
8. Select **Allow** only if you recognize the request.

The client name is reported by the client. It does not prove the client's identity.
The first dialog option rejects access. A notification never captures editor input.
A separate row above the status line shows the pending request count and the effective shortcut.
Typing, temporary messages, and time do not clear this notification. It stays until you decide or the clients disconnect.
`get_access` reports `pending`, `authorized`, `denied`, `revoked`, or `disconnected`.
A request without a prior access request reports `not_requested`.

Each bridge connection needs its own approval in each SQLN instance.
Access includes existing connection labels, open SQL, loaded data, metadata, read queries, and edit proposals.
Existing trust and password questions still apply before database use.
The bridge never receives a vault password or a connection URI with credentials.

This flow also works without a project. One global MCP configuration can discover several SQLN processes.
Every target call specifies `instance_id`. The bridge never selects the last window or silently changes targets.
Restarting SQLN gives it another UUID. Old references stop working.

Revoke a session through the same **Agent sessions** command.
Revocation or stdio disconnection cancels that session's queries, metadata reads, and comparisons.
It also invalidates its pending proposals. Other agents and user executions keep their work.

## Consoles and queries

`open_agent_console` creates a session-local console using an existing connection.
The console name identifies its agent. Its text and results do not change your active tab or focus.
The agent's own console receives operation notifications.
These consoles do not become saved scripts or reopen with an agent after restarting SQLN.
Copy their SQL to a user console to save it. Manual Save explains this restriction.

`run_read_query` accepts new SQL only for that session's own console.
Its SQL and parameters appear in Messages. It does not replace editor text.

`run_console` executes only the requested console's current editor text.
It requires the current editor `revision` and either the whole script or an explicit character range.
It uses the console's parameter values. Missing values refuse the request without moving focus.
The execution replaces that console's results, as a user execution does.

No agent execution shares an open, failed, or unknown user transaction.
An occupied console also refuses execution. The error recommends the agent's own console; nothing reroutes automatically.
The console's manual transaction mode does not apply to an agent execution.

Tasks return `job_id`. Poll `get_job`; use `cancel_job` to stop that session's task.
States include `validating`, `ready`, `waiting_credentials`, `running`, `building_artifact`, `completed`, `failed`, and `cancelled`.
A completed query includes result references with their generation. Use those references to avoid reading a later execution.
The bridge does not retain more than 64 tasks per session. It evicts completed tasks first.

### Read-only protection

The assistant policy parses SQL ASTs and rejects unsupported statements, writes, session changes, locks, and executable comments.
It checks nested queries and CTEs. It accepts only known read functions.
An unknown or qualified function call refuses execution rather than assuming it reads.
For a script, every statement passes validation before the first statement runs.

Native session protection also applies:

| Source | Protection |
|---|---|
| PostgreSQL | `default_transaction_read_only = on` |
| MySQL | `SET SESSION TRANSACTION READ ONLY` |
| SQLite | `PRAGMA query_only = ON` |
| Redis/Valkey | Known read command plus the server's `readonly` flag |

These restrictions belong to each agent execution. They do not change saved connection settings.
Pool cleanup removes the context before another execution reuses a session. Unknown cleanup discards the session.

This is not a sandbox for extensions or external effects of code installed in the database server.
Use database credentials with the minimum necessary privileges.

## Edit SQL

`edit_console` takes an expected revision and ordered replacement intervals.
Positions count Unicode characters, not UTF-8 bytes or terminal columns. Lines and columns start at zero.
An end position is exclusive. The operation rejects invalid or overlapping ranges before changing anything.

The default mode, `propose`, leaves the editor unchanged.
Open **Review agent changes** in the palette, select a proposal, and read the comparison.
Use the arrow keys or wheel to scroll. `Tab` or Left/Right selects Reject or Accept.
`Enter` confirms the selected option. Reject is selected first. `Esc` returns without accepting.
Click a button to accept or reject with the mouse.

Accepting changes applies one undo step in conventional and Vim modes.
The operation adjusts the cursor and selection relative to the changes.
The existing script autosave and external-file conflict checks still apply.
A changed revision invalidates the proposal. The assistant must read the console and propose again.
SQLN shows the reason in the status bar when it discards a proposal, for a changed revision or a diff that exceeds the data budget.
Only one pending proposal can exist for each console.

For direct editing:

1. Select the console in SQLN.
2. Open **Enable agent direct editing for this console** in the palette.
3. Confirm **Allow**.

The tab shows **Direct edits** while permission is enabled.
The assistant can then send `mode: "direct"`, with the same revision and range checks.
The assistant cannot enable this option, approve a proposal, or change another console's permission.
Disable it through the same palette action. Permission ends when the console or SQLN closes and never enters saved configuration.
Editing SQL, accepting a proposal, or generating a script never executes it.

## Tools

All tools except `list_instances` require `instance_id`.
Only discovery and the current session's access state are available before approval.

| Group | Tools |
|---|---|
| Access | `list_instances`, `request_access`, `get_access` |
| Application context | `get_context`, `list_connections`, `list_tabs` |
| Console text and messages | `read_console`, `read_messages` |
| Loaded data | `read_result`, `read_result_cell` |
| Execution | `open_agent_console`, `run_read_query`, `run_console`, `get_job`, `cancel_job` |
| Editing | `edit_console`, `get_proposal` |
| Metadata and SQL | `get_catalog`, `get_object_ddl`, `generate_sql`, `read_artifact` |
| Analysis | `explain`, `get_er_diagram`, `compare_schema`, `compare_data` |

`get_catalog` reads the cache unless a refresh is requested or no catalog is loaded.
An asynchronous catalog read returns a JSON artifact. A large catalog also uses a paged artifact.
`generate_sql` accepts `select_all`, `select_columns`, `insert`, `update`, and `delete` templates.
Generated write SQL is text only. The MCP cannot execute it.

`get_object_ddl` accepts `table`, `view`, `function`, `procedure`, `sequence`, `trigger`, `enum`, and `domain` where supported.
For a PostgreSQL routine, `identity` identifies its arguments. For a trigger, it identifies the table.
Source capabilities and existing version limits apply.

`explain` reads one statement from a console with a checked revision and optional range.
It does not analyze by default. `analyze: true` executes only a query accepted by the read policy.
The existing query plan support determines availability.

Diagram and comparison requests create visible background tabs, without selecting them.
A diagram artifact is JSON with `mermaid`, `entities`, `links`, and a `partial` marker.
A comparison exposes its result grid and a synchronization script artifact.
Comparison rules, primary-key requirements, memory limits, and dialect restrictions match the interface.
Data comparison row reads have no SQL deadline, as in the existing interface. Cancellation still applies.
`DROP` and `DELETE` remain comments unless `removals` is requested for script generation.
No generated script runs automatically, including scripts with removals enabled.

## Pagination and limits

- Result reads return already loaded values. They do not execute the original SQL again.
- A result reference contains a generation. Supply it on later pages and cell reads.
- A changed result returns `result_changed`, rather than continuing into another dataset.
- Connection and tab lists accept `offset` and `limit`, and return `next_offset`.
- Result pages default to 100 rows and accept up to 1,000 rows.
- Responses have a 256 KiB ceiling. Large cells return a reference for `read_result_cell`.
- Text and cell blocks contain at most 16 KiB before JSON encoding. Follow `next_offset`.
- Text offsets count UTF-8 bytes. Follow returned offsets to stay on character boundaries.
- Later `read_console` pages require the revision from the first page.
- `read_messages.offset` selects a record; `text_offset` continues a large message.
- Values distinguish `null`, `text`, `number`, and `binary`. Numbers remain decimal text. Binary blocks use base64.
- Grid reads include pending user edits, without saving them. Selection, loaded limits, and the current viewport remain visible in metadata.
- Each session has at most four active tasks in each instance.
- IPC requests have an 8 MiB ceiling. Edit proposals also require a bounded console snapshot and available data budget.
- Retained proposal text, execution snapshots, and artifacts count against the data budget.
- New operations refuse while a blocking dialog is open. Existing credential requests continue only through their own user response.

An error contains a code, a reason, and a possible next action. A partial result is never reported as complete.

## Local security and lifecycle

The registry uses `XDG_RUNTIME_DIR/sqln-<uid>` when `XDG_RUNTIME_DIR` is absolute.
Otherwise it uses the user's system temporary directory, with the same private subdirectory.
Directory permissions are `0700`; descriptors and sockets use `0600`.
The implementation rejects unsafe owners, permissions, and symbolic links.
A lock identifies a live registration. A handshake checks the instance UUID and IPC version.
Descriptors contain process and path metadata, not passwords, SQL, or result values.

A new local connection cannot use another connection's approval or task IDs.
Keep local client executables and the operating-system user account trusted.
Diagnostic logs exclude SQL, values, passwords, and external database errors.
Command history in the application retains the effective SQL under its existing rules.

MCP support does not change configuration, script, or workspace formats.
It does not add remote access, HTTP, ACP, file import/export, connection management, or user transaction controls.
Comparisons do not span SQLN instances.
Older SQLN versions do not recognize custom bindings for the new agent actions. Remove those bindings before downgrading.

## Client validation

CLI connection checks pass with Claude Code 2.1.318 and OpenCode 1.19.5, using isolated global configuration directories.
The SDK tests also initialize a real stdio subprocess, list its tools, and route requests to explicit local instances.
No interactive model conversation forms part of these checks. UI authorization and edit approval use input-event and TestBackend tests.
