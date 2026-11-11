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

## MCP settings

Open **Settings** with `Ctrl+W ,` and select **MCP**. The fields have the global and the project scope, as the other settings.
A save applies at once.

| Field | Key | Default | Effect |
|---|---|---|---|
| **MCP** | `mcp` | On | Off closes the endpoint: `list_instances` does not find the instance. A save with Off revokes all sessions, cancels their tasks, and forgets the remembered clients. On opens a new endpoint with a new UUID. |
| **Default access** | `mcpDefaultAccess` | Ask | `ask` shows the access request. `propose`, `direct`, or `full` gives that level to a new session without a question, with a notice in the status bar and in the agent Messages. |
| **Max access** | `mcpMaxAccess` | Full access | The highest level of the access request, of the session dialog, of the default access, and of the remembered access. A save with a lower value lowers the sessions and the remembered clients above it, with a notice. |
| **Remember clients** | `mcpRememberClients` | On | Off: each new session asks again. A save with Off forgets the remembered clients. |

A remembered client gets its remembered level before the default access. Both stop at **Max access**.
The run protections stay for each level: read-only connections, the `PROD` question, and the question for a write without `WHERE`.
SQLN never writes the configuration of an MCP client. Configure the client as the next section shows.

## Select an instance and allow access

1. Open SQLN as usual.
2. Ask the assistant to call `request_access`. With one open SQLN instance, `instance_id` is optional.
   With several instances, the assistant calls `list_instances` and sends the exact `instance_id`.
3. Click **Review** in the access card or in the MCP row above the status line. Alternatively, open the palette with `Ctrl+W :` and select **Agent sessions: allow or revoke access**.
   A new request shows the card at the bottom right and rings the terminal bell once. **Later** hides the card, and the MCP row keeps the request.
   Edit proposals and agent tasks that run for more than 3 seconds also use this card, after access requests. See [UI](UI.md).
4. Read the client, the short session identifier, and the request age.
5. Select an option only if you recognize the request.

`request_access` waits for your decision, 120 seconds by default. It returns `status` and `access`.

| Option | `access` | Permits |
|---|---|---|
| **Allow** | `propose` | Reads, read-only queries, and edit proposals. Each edit needs your review. |
| **Allow all** | `direct` | Also direct edits in all consoles, without review. The database stays read-only. |
| **Full access** | `full` | Also database writes, without review. See [Full access](#full-access). |

The client name is reported by the client. It does not prove the client's identity.
**Reject** has the initial focus, and `Esc` selects it. A notification never captures editor input.
After a decision, the next pending request opens. The title shows its place, such as `(1 of 2)`.

SQLN remembers the option for that client name until SQLN exits, unless **Remember clients** is Off. A new session of the client, such as a restarted assistant, gets the same access without a question.
The status bar names the client when this occurs. A rejection is not remembered.
**Agent sessions** lists remembered clients without a live session. Select one and click **Forget** to make its next session ask again.
Revoke also forgets the client. The memory never enters saved configuration.

The MCP row shows pending access requests, pending edit proposals, and running agent tasks.
It shows the effective shortcut, a **Review** button, and a **Stop** button while a task runs.
**Review** opens the oldest access request first, then the oldest edit proposal.
Typing, temporary messages, and time do not clear this row. It stays until no item remains.
The status bar shows `MCP n` while `n` sessions have access, `MCP n all` when one uses **Allow all**, and `MCP n full` when one uses **Full access**.
`request_access` reports `pending`, `authorized`, `denied`, `revoked`, or `disconnected`.

Access includes existing connection labels, open SQL, loaded data, metadata, queries, and edit proposals.
Existing trust and password questions still apply before database use.
The bridge never receives a vault password or a connection URI with credentials.

This flow also works without a project. One global MCP configuration can discover several SQLN processes.
The bridge never selects one of several instances or silently changes targets.
Restarting SQLN gives it another UUID. Old references stop working.

Revoke a session through the same **Agent sessions** command.
Revocation or stdio disconnection cancels that session's queries, metadata reads, and comparisons.
It also invalidates its pending proposals. Other agents and user executions keep their work.
For a session with access, the same command offers the other access options, to lower or raise the access, and **Revoke**.
**Allow** makes edits need review again and makes the database read-only again. Access stays. **Keep access** has the initial focus.

To stop agent work without revoking access, use one of these commands:

| Command | Effect |
|---|---|
| **Stop** in the MCP row, or **Stop all agent tasks** in the palette (`agentStop`) | Cancels the tasks of all sessions. The agent receives `cancelled` with the reason. |
| The stop key or **Stop** button of a console | Cancels the agent tasks of that console. |
| **Tasks N** in an agent tab (`agentTasks`), then **Stop** | Cancels one task of that session. |
| **Revoke** | Cancels the session's tasks and removes its access. |

SQLN tells you in the status bar when an agent edits a console directly, runs a user console, opens a diagram or comparison tab, or a task fails.
The same notices, the proposals and your decisions about them, and access changes also go to the Messages of the session's agent tab.
SQLN uses the agent tab for the connection of the action, or else the last agent tab of the session. No tab opens for a notice: before the first agent tab, only the status bar shows it.
Each agent tab has an **Access: level** button (`agentSession`). It opens the same session dialog as **Agent sessions**, for the session of that tab.

## Tools

| Tool | Purpose |
|---|---|
| `list_instances` | Lists open SQLN instances. |
| `request_access` | Asks for access and waits for the decision. A second call reads the state. |
| `get_context` | Reads the access level, the active tab, the connections, the open tabs, and this session's pending proposals. |
| `read_console` | Reads console text and its revision. `messages: true` adds the console Messages. |
| `read_result` | Reads loaded rows. With `row` and `column`, it reads one large cell in blocks. |
| `query` | Executes SQL on a connection, in the session's agent console, and returns the results. |
| `run_console` | Executes the text or a range of a user console, and returns the results. |
| `edit_console` | Places SQL in the agent tab for you to review and run, or edits a console. |
| `describe` | Reads the catalog, the DDL of one object, or an ER diagram. |
| `compare` | Compares structure or data within one SQLN instance. |
| `job` | Continues to wait for a task or a proposal, reads the next artifact page, or cancels a task. |

All tools except `list_instances` accept an optional `instance_id`.
Only `request_access` is available before approval.

### Waiting

`request_access`, `query`, `run_console`, `edit_console`, `describe`, `compare`, and `job` wait for the end of their work.
`wait_seconds` sets the time. The default is 50 seconds, because many clients stop a tool call after 60 seconds. The maximum is 600.
After this time, the reply holds the current state and `next_action`. The task continues. Call `job` with its `job_id` to wait again.
A client cancellation of a waiting call cancels its task.

A task reply contains `job_id`, `kind`, `status`, `message`, `tab_ids`, `statements`, `results`, and `artifact`.
States include `validating`, `ready`, `waiting_confirmation`, `waiting_credentials`, `running`, `building_artifact`, `completed`, `failed`, and `cancelled`.

- `statements` holds the end of each executed statement: `status`, `rows` for a result, `changed_rows` for a write, and `error`.
- `results` holds the first page of each query or comparison result: 100 rows by default, set by `rows` (0 to 1,000).
  The pages share half of the response limit. A result without space has `rows_omitted: true`; read it with `read_result`.
- `artifact` holds the first page of a text artifact, such as DDL, a catalog, a diagram, or a sync script. `job` with `artifact_offset` reads the next page.

The bridge does not retain more than 64 tasks per session. It evicts completed tasks first.

### Consoles and queries

`query` uses the session's agent console for that connection.
The session's first agent tab opens when you allow access, or at once for a client with remembered access. It has no connection yet, and your focus stays.
The first task or placed SQL gives that tab its connection. A task on another connection opens another agent tab of the session.
Agent tabs stand at the right end of the tab row, after the other tabs.
While the session has access, its last agent tab closes only after a question: **Cancel** (initial focus) or **Revoke and close**.
After a revocation or a disconnection, the tab stays with `MCP off ·`, and it closes without that question.
The console name identifies its agent, such as `MCP · client [3f2a9c1e]`. It does not change your active tab or focus.
Without placed SQL, the agent tab shows its Messages and its result grids, and a **Stop** button. Paste and text keys do not change it.
It receives operation notifications with the task kind and a short task identifier.
A tab of a session without access shows `MCP off ·`.
`query` accepts named `parameters` and `explain` (`plan` or `analyze`). Its SQL and parameters appear in Messages.

`run_console` executes only the requested console's current editor text.
It requires the current editor `revision` and either the whole script or an explicit character range.
It uses the console's parameter values. Missing values refuse the request without moving focus.
The execution replaces that console's results, as a user execution does.
`explain` reads one statement. `analyze` executes it.

No agent execution shares an open, failed, or unknown user transaction.
An occupied console also refuses execution. The error recommends the agent's own console; nothing reroutes automatically.
The console's manual transaction mode does not apply to an agent execution.
An agent execution never leaves a transaction open. If its SQL starts a transaction and does not end it, SQLN rolls it back and the task fails with the reason.

### SQL for you to run

For SQL that you must run, such as a write without **Full access**, the agent calls `edit_console` without `console_id`, with `connection_id` and `sql`.
SQLN shows the SQL in an editor at the top of the agent tab for that connection. No tab opens, nothing executes, and your focus stays.
The status bar names the agent, the agent Messages say how to run the SQL, and the tab shows `SQL ·` until you open it.

- You can change the SQL and run it with the usual run keys or **Run**. It is your run: the run confirmations apply, and the read policy does not.
- The agent tab runs in Auto mode. If your SQL opens a transaction, **Commit** and **Rollback** appear. Agent tasks in that tab refuse to run until the transaction ends.
- New SQL from the agent replaces SQL that you did not change, as one undo step. If you changed the SQL, the new SQL is an edit proposal for your review.
- The agent can read the results of your run with `read_result`. It can run the SQL with `run_console`, under the rules of its access level.
- **New console** (`agentSqlToConsole`) copies the SQL to a new user console, which takes the focus. Use it to save the SQL as a script.
- **Clear** (`agentSqlClear`) empties the editor as one undo step. The tab then shows its results only.

Agent tabs do not become saved scripts or reopen with an agent after restarting SQLN. Manual Save explains this restriction.

### Read-only protection

Without **Full access**, the assistant policy parses SQL ASTs and rejects unsupported statements, writes, session changes, locks, and executable comments.
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
| Kafka | No agent read. The assistant refuses each Kafka command |

These restrictions belong to each agent execution. They do not change saved connection settings.
Pool cleanup removes the context before another execution reuses a session. Unknown cleanup discards the session.
Metadata reads, DDL reads, diagrams, and comparisons always use this protection.

This is not a sandbox for extensions or external effects of code installed in the database server.
Use database credentials with the minimum necessary privileges.

### Full access

With **Full access**, `query` and `run_console` execute any SQL, without the read policy and without the native read-only context.
These protections still apply:

- A read-only connection, or the read-only setting, stays read-only. The database refuses the write.
- A run on a connection with the `PROD` mark that can change data, or a write without `WHERE`, opens the run confirmation of a user run. The task waits in `waiting_confirmation`.
  **Run** continues it. **Cancel** or `Esc` cancels it with `The user refused the run in SQLN.`
- The transaction rules above.

## Edit SQL

`edit_console` with `console_id` takes an expected revision and ordered replacement intervals.
Positions count Unicode characters, not UTF-8 bytes or terminal columns. Lines and columns start at zero.
An end position is exclusive. The operation rejects invalid or overlapping ranges before changing anything.

The default mode, `propose`, leaves the editor unchanged and waits for your review.
The reply has `kind: "proposal"` and a final `status`: `applied`, `rejected`, `partial`, `revision_conflict`, or `budget_reached`.
`partial` means that the user accepted some changes and rejected others. `accepted_changes` and `rejected_changes` hold the counts after the user decides.
The notice after **Accept** names the console that changed, because it can be another tab.
A proposal that the console invalidates closes its open review, and the status bar gives the reason.
`console_revision` holds the current revision after the decision.
Click **Review** in the MCP row or the card, or open **Review agent changes** in the palette and select a proposal.
SQLN selects the console tab and shows the changes in its editor, as an [inline review](UI.md#inline-review).
The tab of a console with a pending proposal starts with `± `.
Decide each change, or all changes. `Esc` (**Later**) closes the review; the proposal and its decisions stay pending.
After the last decision, the editor stays, and the card shows the next proposal.

Each accepted change goes into the text at once, as one undo step, in conventional and Vim modes. **Accept all** is one undo step.
The operation adjusts the cursor and selection relative to the changes.
The existing script autosave and external-file conflict checks still apply.
A changed revision invalidates the proposal. The assistant must read the console and propose again.
SQLN shows the reason in the status bar when it discards a proposal, for a changed revision or a diff that exceeds the data budget.
Only one pending proposal can exist for each console. Only the user decides a proposal.

A session with **Allow all** or **Full access** edits every console directly. For direct editing in one console only:

1. Select the console in SQLN.
2. Open **Enable agent direct editing for this console** in the palette.
3. Confirm **Allow**.

The tab shows `Direct ·` while permission is enabled.
After each direct edit, the status bar names the agent, the console, and the undo key.
The assistant can then send `mode: "direct"`, with the same revision and range checks.
The assistant cannot enable this option, approve a proposal, or change another console's permission.
Disable it through the same palette action. Permission ends when the console or SQLN closes and never enters saved configuration.
Editing SQL, accepting a proposal, or opening a console never executes it.

## Metadata and analysis

`describe` without `object` reads the catalog. It reads the cache unless `refresh` is true or no catalog is loaded.
A small cached catalog returns directly. Other catalog reads return a JSON artifact.
With `object` (`name`, `kind`, `identity`) and `schema`, it reads the DDL.
`kind` accepts `table`, `view`, `function`, `procedure`, `sequence`, `trigger`, `enum`, and `domain` where supported.
For a PostgreSQL routine, `identity` identifies its arguments. For a trigger, it identifies the table.
With `er_diagram: true` and `schema`, it reads an ER diagram of the schema, or of the `object` table.
Source capabilities and existing version limits apply.

Diagram and comparison requests create visible background tabs, without selecting them.
A diagram artifact is JSON with `mermaid`, `entities`, `links`, and a `partial` marker.
A comparison returns its result grid and a synchronization script artifact.
Comparison rules, primary-key requirements, memory limits, and dialect restrictions match the interface.
Data comparison row reads have no SQL deadline, as in the existing interface. Cancellation still applies.
`DROP` and `DELETE` remain comments unless `removals` is requested for script generation.
No generated script runs automatically, including scripts with removals enabled.
The MCP does not offer the SQL templates of the interface. The agent writes SQL from the catalog and the DDL.

## Pagination and limits

- Result reads return already loaded values. They do not execute the original SQL again.
- A result reference contains a generation. Supply it on later pages and cell reads.
- A changed result returns `result_changed`, rather than continuing into another dataset.
- `get_context` lists up to 200 connections. Tabs accept `offset` and `limit`, and return `next_offset`.
- Result pages default to 100 rows and accept up to 1,000 rows.
- Responses have a 256 KiB ceiling. Large cells return `needs_cell_read`; read them with `read_result`, `row`, and `column`.
- Text and cell blocks contain at most 16 KiB before JSON encoding. Follow `next_offset`.
- Text offsets count UTF-8 bytes. Follow returned offsets to stay on character boundaries.
- Later `read_console` pages require the revision from the first page.
- `read_console.message_offset` selects the first message record.
- Values distinguish `null`, `text`, `number`, and `binary`. Numbers remain decimal text. Binary blocks use base64.
- Grid reads include pending user edits, without saving them. Selection, loaded limits, and the current viewport remain visible in metadata.
- Each session has at most four active tasks in each instance.
- IPC requests have an 8 MiB ceiling. Edit proposals also require a bounded console snapshot and available data budget.
- Retained proposal text, execution snapshots, and artifacts count against the data budget.
- New operations refuse while a blocking dialog is open. Existing credential requests and run confirmations continue only through their own user response.

An error contains a code, a reason, and a possible next action. A partial result is never reported as complete.

## Local security and lifecycle

The registry uses `XDG_RUNTIME_DIR/sqln-<uid>` when `XDG_RUNTIME_DIR` is absolute.
Otherwise it uses the user's system temporary directory, with the same private subdirectory.
Directory permissions are `0700`; descriptors and sockets use `0600`.
The implementation rejects unsafe owners, permissions, and symbolic links.
A lock identifies a live registration. A handshake checks the instance UUID and IPC version. This version uses IPC version 2.
Descriptors contain process and path metadata, not passwords, SQL, or result values.

A new local connection cannot use another connection's approval or task IDs.
Keep local client executables and the operating-system user account trusted.
Diagnostic logs exclude SQL, values, passwords, and external database errors.
Command history in the application retains the effective SQL under its existing rules.

MCP support does not change configuration, script, or workspace formats.
It does not add remote access, HTTP, ACP, file import/export, connection management, or user transaction controls.
Remembered access stays in memory only.
Comparisons do not span SQLN instances.
Older SQLN versions do not recognize custom bindings for the new agent actions. Remove those bindings before downgrading.

## Client validation

CLI connection checks passed with Claude Code 2.1.318 and OpenCode 1.19.5 for IPC version 1, using isolated global configuration directories.
The tool set of IPC version 2 has SDK tests only. Repeat the CLI checks before a release.
The SDK tests also initialize a real stdio subprocess, list its tools, and route requests to explicit local instances.
No interactive model conversation forms part of these checks. UI authorization and edit approval use input-event and TestBackend tests.
