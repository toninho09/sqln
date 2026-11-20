# Decisions — AI, MCP, and agent

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## Explicit AI SQL completion

The user approves a command-driven completion flow, separate from MCP.
This adds optional `preferences.aiCompletion` settings. Existing configuration remains valid.

| Point | Decision |
|---|---|
| Entry | AI row above local autocomplete items. Up selects it from the first local item; the configured suggestion acceptance key starts the request. No separate default shortcut. |
| Scope | Consoles associated with PostgreSQL, MySQL, and SQLite. No Redis, unbound consoles, chat, agent tools, or generated SQL execution. Kafka consoles join with "Kafka AI completion, MCP reads, and topic filter — 2026-11-13". |
| Providers | One OpenAI-compatible Chat Completions provider, with OpenRouter as the default endpoint. Optional local servers run outside SQLN. The removed DeepSeek and protocol settings are legacy values that old files may still carry. |
| Provider URL | Accept an API base or a full completion URL. Add the route only once; a legacy full FIM URL maps to the chat route on the same base. Model discovery uses `/models`. An empty endpoint uses OpenRouter. A real API and terminal test confirms these routes. |
| Context | Cache the complete accessible structural schema. Respect connection schema filters. Refresh rebuilds a previously loaded snapshot; no polling for external changes. Never read records for completion. |
| Large context | Reduce automatically, with a visible reduced-context label. Keep the cache complete. |
| Selection | Replace only the selected interval. Without selection, propose insertion at the cursor. |
| Review | Show a waiting animation, then Accept and Reject. Use an editor preview for insertion and a diff for replacement. Reject starts selected; acceptance is one undo step. |
| Configuration | Global defaults and a project override. The project can disable AI. Enabled inherits independently; provider configuration inherits as one block. |
| Credentials | System vault or the contents of a key file. Read the file for each request to support key rotation. No plaintext API-key persistence or environment-variable source in this delivery. |
| Consent | Once per project path, connection, endpoint, and credential reference. Local private persistence only. |
| Failures | Keep user text. Cancel stale requests; never retry or change provider automatically. |

Implementation limits and configuration are in [AI completion](../AI_COMPLETION.md).

## Inline review of AI and MCP changes — 2026-11-05

The user asks for a review flow in the editor, as in a commercial IDE. Before this entry, the AI preview and the MCP proposals showed outside the editor, in floating windows and dialogs.
This entry replaces the **Review** row of the AI completion table, decision 8 of the MCP notifications entry for proposals, and the edit review dialog. Access requests keep their dialog and their queue.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | AI insertion without a selection | Ghost text at the cursor. Tab accepts, Esc rejects. The arrows keep it: Up and Down move the view, Left and Right do nothing (user correction of 2026-11-06). Other keys reject, and the key goes to the editor | The floating preview; arrows that move the cursor and reject | User choice |
| 2 | AI replacement and MCP proposal | An inline diff in the editor: removed lines with `-` and the error color, added lines with `+` and the success color, changed words marked | Side by side; the dialog | Side by side does not fit 80×24 |
| 3 | Unit of a decision | Each change (hunk), and all changes | The whole proposal only | User choice |
| 4 | Editor during a review | Read only. Another key shows the reason and the review keys | Free editing that rejects or moves the proposal | User text and the proposal stay consistent |
| 5 | Review keys | `a` and `r` for the current change, `A` and `R` for all, Up/Down, `j`/`k`, and `]`/`[` for the next and previous change. Each key is an `Action` with custom keys | Ctrl combinations | `Ctrl+Y` is Redo, and `Ctrl+Enter` needs keyboard enhancement |
| 6 | Esc | AI: reject. MCP: **Later**, and the proposal stays pending in the card and the MCP row | Reject for both | User choice |
| 7 | Enter and initial focus | No button has the focus. Enter decides nothing | Reject with the focus | An explicit key or a click decides |
| 8 | Opening of an MCP review | **Review** (card, MCP row, palette) selects the console tab and shows its first change | Automatic display; the dialog | Typing never goes to a review that the user did not open |
| 9 | Tab mark | `± ` before the name of a console with a pending proposal | No mark | The user sees where a proposal waits |
| 10 | Application and undo | An accepted change goes into the text at once, as one undo step. **Accept all** is one undo step | Application at the end of the review | User choice |
| 11 | Reply to the agent | A new final status `partial`, with the counts of accepted and rejected changes | `applied` when one change is accepted | The agent knows what the user kept |
| 12 | Queue after the last decision | The editor stays. The card shows the next proposal with its place | Select the tab of the next proposal | A jump to another tab moves the user |
| 13 | Agent tab SQL that the user changed | The same inline diff | The dialog | One flow |
| 14 | Controls | Buttons on the current change, and a bar of one row at the bottom of the editor. A narrow bar drops labels; the keys stay | Buttons on each change | At 80×24, the text keeps its room |

## Persistent AI ghost and buttons at each change — 2026-11-06

The user asks for buttons near the change and a suggestion that stays until a decision, because the generation has a cost.
This entry replaces decisions 1 and 14 of the inline review entry for AI ghost text, and the AI part of decision 4: only a replacement review keeps the editor read only.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Buttons of a ghost | A dedicated row after the ghost text; a ghost has no bar | The bar on the last row | The user asked for controls near the change |
| 2 | Life of a ghost | It stays until the user accepts or rejects it. Keys, clicks, commands, cursor movement, tab changes, and schema refreshes keep it | Any key or click rejects | The generation has a cost; only the user discards |
| 3 | Silent discards | An edit of the anchor line or any undo discards the ghost, in silence | Re-anchor on any edit; a confirmation | The continuation can be wrong, and no dialog interrupts |
| 4 | Edits on other lines | They move the anchor with the text | Keep the old row | The ghost stays correct |
| 5 | Acceptance | It applies the ghost at its anchor, not at the cursor, as one undo step | Apply at the cursor | The anchor is where the model answered |
| 6 | Answer in flight | It shows at its requested anchor; an edit on another line moves the anchor. An edit of the requested line stops the request at once, in silence | Stop the request when the cursor moved; wait for the answer and show nothing | The cost argument applies, and the answer never lands on changed text |
| 7 | Buttons of a diff | Each pending change has its Accept and Reject buttons. The bar keeps the place, the origin, Accept all, Reject all, and Close only | Buttons on the current change only; the bar decides one change | User choice; the keys still follow the current change |
| 8 | Tab mark | `± ` also shows a pending AI ghost | Agent proposals only | The user sees where a suggestion waits |
| 9 | Run | It executes the text without the ghost, and the ghost stays | Discard on run | Only acceptance changes the text |

## OpenRouter reference review — 2026-11-06

The user asks to follow the current OpenRouter chat completion reference.
The review confirms the endpoint, the bearer auth, the message form, the response parse, and the top-level `reasoning_effort` field of the app. Two points change:

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Output field of the body | The request body carries `max_completion_tokens`, the current field of the reference. The output range starts at 16 | Keep `max_tokens`, which the reference marks deprecated | The old field stays accepted but deprecated, and some providers refuse an output below 16 |
| 2 | Explicit no-reasoning value | The Reasoning effort choice gains `None`, which sends `reasoning_effort: "none"`. Disabled keeps the omission | Keep only the omission | The reference lists `none` in the field enum; it is the explicit request that the model does not think, and the OpenAI reference accepts it too |

This entry amends point 3 of "Provider reasoning effort — 2026-11-06": the value list now holds Disabled, None, Low, Medium, and High. The reference also lists `minimal`, `xhigh`, and `max`; SQLN does not send them, because strict OpenAI-compatible endpoints reject them.

## Provider reasoning effort — 2026-11-06

The user asks for a setting that chooses the reasoning effort of the autocomplete model and can disable the thinking.
The user chooses the OpenAI `reasoning_effort` field of the chat body, the omission of the field as the disabled state, and the values without `minimal`, after a review of the OpenRouter parameter rules.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Wire field | The request body carries the top-level `reasoning_effort` of the Chat Completions standard; the OpenRouter base accepts it as the OpenAI alias | The `reasoning` object of OpenRouter, or both fields | The provider is one OpenAI-compatible endpoint; strict servers reject unknown objects, and duplicated fields risk a 400 |
| 2 | Disabled | The choice Disabled omits the field, so the endpoint uses its default; this is not a guarantee that the model stops thinking | Send `reasoning.enabled: false`, or send `minimal` as off | Omission is safe for any endpoint. The OpenRouter switch that turns thinking off is its own field, and `minimal` is a value of the GPT-5 family only |
| 3 | Values | Disabled, Low, Medium, High | Include Minimal | Low, Medium, and High are the documented values of the OpenRouter effort set and valid OpenAI values for GPT-5 and the o-series. Minimal is outside the documented OpenRouter set for models that are not GPT, and the direct OpenAI API rejects it for the o-series. Adding it later is one enum variant and one text |

Saved settings carry the field only when it is not Disabled. A file without the field, or with an unknown value, reads as Disabled.

## AI settings in two sections — 2026-11-06

The user asks to split the AI part of Settings into two sections: **MCP** and **Autocomplete**.
This entry suspends row 12 of the MCP settings entry: the sections take the names MCP and Autocomplete, and the IA section ends.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Split | Two sections in the sidebar: MCP holds the four MCP fields and the client help; Autocomplete holds the Configure… row that opens the dialog | One section with two pages | The sidebar is the navigation of Settings; one row per place keeps the rule of the other sections |
| 2 | Names | MCP and Autocomplete | Keep IA | User request; each name names its subject |

## One OpenAI-compatible provider — 2026-11-06

The user asks to remove the FIM completion mode and to keep only the OpenRouter standard, then widens the request to one OpenAI-compatible provider.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Protocols | The completion always uses the OpenAI Chat Completions standard | Keep the FIM protocol | User request: one provider standard. The feature is a development preview, so no release blocks the change |
| 2 | Provider kinds | One kind: OpenAI compatible. The Deepseek and Openrouter kinds become legacy values of saved settings, accepted and treated like Compatible | Keep the kind selector | One experience, as with the selection removal of the same day |
| 3 | Empty endpoint | The default endpoint is `https://openrouter.ai/api/v1` | Refuse an empty endpoint | The default of the previous OpenRouter kind stays out of the box; other endpoints set the URL |
| 4 | Legacy full FIM URLs | A URL that ends in `/completions` maps to the chat route on the same base | Reject the URL | Old files stay valid without a migration; the credential identity keeps the configured URL |
| 5 | Legacy `protocol` field | Saved files with the field load; SQLN ignores the value and new saves omit it | Break the parse of old files | Follows the file rule: optional keys keep older files valid |

This entry suspends the DeepSeek parts of the provider decisions in the summary table above.

## AI completion at the cursor only — 2026-11-06

The user asks to remove the completion of a selection, so one completion experience stays.
This entry suspends the replacement decisions of the inline review entry for AI: points 2, 4, and 8 of that entry no longer apply to AI, and the exclusivity decision 8 of the parallel entry now covers agent reviews only.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Scope | The AI completion works at the cursor only, for now. No request replaces a selection | Keep the selection path inactive behind a setting | One experience for now; the removal is a user request, and the git history keeps the code |
| 2 | Request under a selection | It refuses with a status message, and the selection stays | Clear the selection and complete at the cursor | User choice; the selection is never discarded in silence |
| 3 | Lists | The AI row hides while a selection stays; the local list of `Ctrl+Space` still shows in insert mode | Keep the AI row and refuse on accept | The row that cannot run does not show |
| 4 | Agent proposals | They keep their inline diff, which ends the AI requests of the console; hidden ghosts return after the review | Remove the diff review itself | The agent review is another feature and shares the model |
| 5 | Diff of the answer | The answer always shows as ghost text at its anchor | A diff review when the answer spans lines | One completion experience |

## Several AI completions at once — 2026-11-06

The user asks for more than one AI completion in the same console, on different lines.
This entry extends the persistent ghost entry: its decisions 3, 5, 8, and 9 now cover each ghost, and a new setting bounds the answers that wait.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Parallelism | Requests run in parallel, with one setting for the most answers that wait at once per console: ghosts that show plus requests that run | A queue of one request at a time | The user asked for parallel work; one number keeps the model simple |
| 2 | Limit | Configurable in Settings > Autocomplete > Limits, range 1 to 9, default 3 | No limit; a fixed number | Provider limits and screen room vary |
| 3 | Same line | A request on a line that has a ghost replaces that ghost, even at the limit | Block the request | The newest answer for a line wins |
| 4 | Decide keys | `Tab` and `Esc` act on the line of the cursor, always, even with one ghost | The only ghost decides from anywhere | One rule for every case, user choice |
| 5 | Edit of an anchor line | It closes only the ghost of that line; the others stay | Close every ghost | Each ghost follows its own line |
| 6 | Undo | Any undo takes every ghost of the console | Track the ghosts that the undo affects | The affected set is fragile to track |
| 7 | Esc with several runs | It ends the run of the line of the cursor; the bar button **Cancel** ends them all | End every run | The user asked for the line rule; the button names all |
| 8 | Selection diff | It stays exclusive: opening it takes the ghosts and their runs, and the editor refuses new requests until it closes | Ghosts frozen behind the diff; diffs blocked by ghosts | The diff is read only; one mode at a time |
| 9 | Tab mark | `±N` counts the ghosts that wait | `±` without a count | The user sees how many answers wait |

## AI completion with a user request — 2026-11-06

The user asks to send a request with an AI completion, to say what the completion must do.
This entry extends the cursor-only entry: the request rides the same flow, and the plain row keeps its direct start.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Entry | A second AI row, **Complete with AI with a request…**, sits above the plain row in the suggestion list. The palette offers the same command. No separate default shortcut | Only the palette command; always ask on the plain row | The row is visible while typing; the plain row stays one key away |
| 2 | Input | A modal of one line opens on accept. `Enter` sends, `Esc` cancels, and an empty input sends a plain completion | An inline row in the editor; a footer line | The modal follows the rename pattern and keeps the editor untouched |
| 3 | Body | The request travels as `request` in the user content, next to `context` and `metadata`, only when present. The system instruction tells the model to follow it | A second user message; a comment in the SQL | One body shape; strict endpoints reject extra roles of unclear order |
| 4 | Limit | 1024 characters at most, and the text counts against the context window and the memory budget | No bound | The request shares the limits of the context |
| 5 | Review | The answer shows as ghost text at the anchor, with the same accept, reject, and edit rules as a plain completion | A different review for requests | One completion experience |
| 6 | Privacy | The request never echoes in the status bar, pending messages, or logs | Show the request while it waits | User text stays out of diagnostics |

## Prompt-cache-friendly completion request — 2026-11-06

The user asks to raise the prompt-cache hits of the completion requests.
This entry amends the large-context rule of the Explicit AI SQL completion table: reduction keeps one canonical order and a provider-derived budget.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Content order | The user content serializes `metadata` first and `context` last | Keep `context` first, as before | Providers cache identical leading bytes; the stable schema section then hits the prompt cache across requests |
| 2 | Object order | One canonical schema and name order for every request | Order by SQL references, foreign-key neighbors, then current schema | Any per-request reorder changes the metadata bytes and ends the cache; the schema snapshot order already gives the canonical order |
| 3 | Relevance | A `focus` field in the context lists the objects that the SQL references and their direct foreign-key targets, in the canonical order, only when present | Drop the relevance signal | The model still sees what the SQL names, without changing the cached section |
| 4 | Metadata budget | The budget derives from provider limits alone: the window minus the output, minus a fixed SQL reserve of 16 KiB capped at half of the remainder | The remainder after the SQL of each request | The section stays identical for one snapshot and provider configuration, whatever SQL the request carries |

## MCP context, Kafka bounds, and IPC 3 — 2026-11-20

A second review finds that the agent works without some facts: production and read-only marks, the notices of the database, the limit of the grid, and the end of a Kafka read.
A Kafka read without a bound can examine the whole topic, and the Kafka guide repeats in each `get_context`. The user approves the points below.
This entry replaces the **One guide** delivery of `get_context` in the Kafka entry of 2026-11-13, and the MCP fields row of that entry for the kind of a topic.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Connection facts | `get_context` gives `production`, `read_only`, and `default_schema`. `data_edit` leaves | Keep the fixed `data_edit` | The agent knows which runs ask or refuse |
| 2 | Statement end | `statements` gives `limit` and `notices` (10 notices, 2 KiB) | Messages only | The agent must not read part of the rows as all rows |
| 3 | Kafka read end | Each Kafka result gives `read`: scanned, matched, and end. A scan without match adds `next_action` | Notice text only | Zero rows after `SCAN` is not an empty topic |
| 4 | Default scan | SQLN adds `SCAN 100000` to an agent read without `LIMIT`, `SCAN`, or a start, at each access level | Refuse the read; keep only advice | User choice. The read always ends |
| 5 | Guide delivery | Once per session, only with a Kafka connection; `include_syntax` sends it again | Each reply; a separate tool | User choice. Fewer tokens |
| 6 | Topic kind | `describe` accepts the kind `topic` without a schema. The older form stays | Only the older form | The agent does not guess `schema: "topics"` |
| 7 | IPC 3 | Requests carry an id. Instances of another version appear with `compatible: false` and give `version_mismatch` | Hide them | User choice. One version change for the contract |

## MCP review fixes — 2026-11-20

A review of the MCP finds three faults. The MCP result holds each reply twice, so a reply that SQLN accepts can exceed the response limit, and a task that ran then looks like a failure without its `job_id`.
The read policy accepts DuckDB and Databricks reads of files and URLs through a quoted table name, and these sources have no native read-only context.
A call without `instance_id` moves to a restarted instance without notice. The user approves the three fixes.
This entry replaces decision 3 of the **MCP tools, waiting, and full access** entry.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Large reply | The bridge removes result rows, from the last result page, then the artifact page. The reply keeps the task state and says what to read again | An error without the task; a smaller page for every reply | A task that ran must not look like a failure. The agent could repeat a write |
| 2 | Files in the read policy | A table name in single quotes refuses. In DuckDB and Databricks, a quoted table name with a path character refuses | Refuse every quoted name; a native setting | DuckDB cannot enable external access again in the same database |
| 3 | Instance selection | A call without `instance_id` uses the instance of the previous call, or else the only open instance. A closed instance gives `instance_gone` | Discover again at each call | The bridge never moves to another instance without the agent's choice |

## MCP agent facts — 2026-11-20

A third review finds facts that the agent guesses. The `describe` text teaches the older Kafka form, the catalog ignores `schema`, and a topic with a schema refuses with a misleading reason.
Results give no column types, SQL for the user gives no next step, unloaded catalog columns look like no columns, and `get_context` lists only 200 connections. The user approves the points below.
The IPC version stays 3: the version of this branch is not released.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Column types | The result sink takes the database type of each column. PostgreSQL, MySQL, SQLite, DuckDB, and Databricks send it; Redis and Kafka do not. Result pages give `column_types` | Only advise `describe` | The agent knows dates, decimals, and JSON without one more read |
| 2 | SQL for the user | The `placed` reply gives `next_action`: how to read the run of the user | A task that waits for the run of the user | User choice. No new task kind |
| 3 | Catalog of a schema | `describe` with `schema` and no `object` gives that schema only. An unknown schema gives `schema_not_found` with the names | Refuse `schema` | A smaller reply without an artifact |
| 4 | Topic schema | The kind `topic` reads the fields with any schema | Refuse other schemas | A topic has no schema of its own |
| 5 | Loaded columns | Each catalog relation gives `columns_loaded` | Leave `columns: null` alone | Null must not read as no columns |
| 6 | Connection pages | `get_context` takes `connection_offset` and gives `connections_next_offset`. The Kafka guide looks at all connections | Keep 200 connections | Every connection is reachable |
| 7 | Dialect names | Each dialect file names its MCP dialect. The names keep their values | Rename them to snake case | Agents and `syntax.Kafka` keep working |

## MCP settings in the IA section — 2026-11-05

The user asks for an **IA** section in Settings, to set the MCP behavior per project.
This entry replaces the rule "Configuration, script, and workspace formats do not change" of the Local MCP assistant table for the four keys below, and row 2 of the MCP tools entry: a saved default access can give full access.
The keys are optional, so older files stay valid without a migration.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Content | SQLN preferences for the agent, in `config.json` | Write the MCP client files of the project | User choice |
| 2 | Help | A text below the fields with the Claude Code command and a reference to `docs/MCP.md`. SQLN does not configure the client | Configure the client from the dialog | User choice |
| 3 | **MCP** (`mcp`) | Off: the instance has no socket, and `list_instances` does not find it. On by default | No field | A `PROD` project can turn off the agent |
| 4 | Change of **MCP** | At once: a save opens or closes the socket. Off revokes the open sessions and cancels their tasks | At the next start | The user sees the effect of the save |
| 5 | **Max access** (`mcpMaxAccess`) | The highest option of the access request and of the session dialog: Allow, Allow all, or Full access. Full access by default | No limit | The project limits the agent |
| 6 | Lower maximum | Sessions and remembered clients above it go down to it at the save, with a notice in the status bar and the session Messages | Keep them until the end | User choice |
| 7 | **Default access** (`mcpDefaultAccess`) | Ask (default), Allow, Allow all, or Full access. A value other than Ask gives that level without a question, card, or bell, with a notice, as the remembered access does. It never goes above **Max access** | Only the initial focus of the request | User choice |
| 8 | Levels of the default | All levels, limited by **Max access**. The run protections stay | Up to Allow all | User choice |
| 9 | **Remember clients** (`mcpRememberClients`) | Off: each new session asks again. On by default. The memory stays out of the file | No field | User choice |
| 10 | Order | A remembered level comes before the default access | Default first | The remembered level is a choice of the user |
| 11 | Scope | Global and Project, as the other sections | Global only | User choice |
| 12 | Name | **IA** | **AI**, **Agent**, **MCP** | User choice |

## Agent tab lifetime and place — 2026-11-05

The user asks for the agent tab to open when the agent connects, to stay open while the agent is connected, and to stand at the right of the tab row.
This entry replaces the rule that the first query creates the agent tab.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Opening | At the access approval, or at once for a remembered client. A rejection opens no tab | At `request_access` | Before approval, the agent reads nothing |
| 2 | Tab without a connection | One session tab without a connection. The first task or placed SQL binds it. Another connection opens another agent tab | One tab for each session; a tab on the active connection | Tasks keep running in parallel on different connections |
| 3 | Close while connected | The last agent tab of a session with access asks: **Cancel** (initial focus) or **Revoke and close**. Other agent tabs of the session close as usual | Refuse the close; close and reopen on the next notice | The user keeps control, and the tab ends with the access |
| 4 | After the session | The tab stays with `MCP off ·` and closes without a question | Close it automatically | Its Messages and results stay readable |
| 5 | Place | Agent consoles come after the other tabs, at the right end of the row before **Quit** when the row has room. Agent diagram and comparison tabs keep the order of opening | Mixed with the consoles | The agent tab stays apart from the user's consoles |
| 6 | Lost tab | A session with access that loses its last tab, for example when its connection is removed, gets a new session tab | No tab until the next task | The session keeps one tab |

## Session controls in the agent tab — 2026-11-05

The user reports that the agent tab does not show its session: the access, the tasks, and many notices are in the status bar, the cards, and the palette only.
This entry replaces decision 11 of the MCP notifications entry for the items below. The global commands, the cards, and the MCP row stay.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Access in the tab | An **Access: level** button in the bar of each agent tab opens the session dialog: change the access or revoke it (`agentSession`) | The picker of all sessions only | The user sees the access of the agent in its tab |
| 2 | Tasks in the tab | A **Tasks N** button opens a list of the session's tasks. A selected task opens **Keep running** (initial focus) and **Stop** (`agentTasks`) | A Tasks tab next to Messages; one row for each task above the results | One bar row. At 80×24 the results keep their room |
| 3 | Scope | The button and the list cover the whole session, on all its connections. The console **Stop** still stops the tasks of that console only | The tasks of the tab's connection only | The access and the tasks belong to the session |
| 4 | Log | Every notice of an agent action also goes to the session's Messages: runs and edits of user consoles, proposals and their decision, discarded proposals, opened tabs, access changes, and revocation | The status bar only | The Messages keep a complete record of the agent |
| 5 | Log target | The session's agent tab for that connection. Without it, the last agent tab of the session. Without an agent tab, the status bar only | Open an agent tab for the note | No tab opens for a note |
| 6 | Narrow bar | Tasks goes before Access. A narrow bar drops Access first; the palette and a custom key keep it | Shorter labels | A running task can need a stop |

## Agent SQL in the agent tab — 2026-11-05

The user reports that SQL from the agent opens a new console for each request, and that a lowered access cannot go up again. This entry replaces decisions 3, 4, and 13 of the MCP notifications entry.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Place of the SQL | An editor in the agent tab | SQL blocks in Messages; a new console next to the agent tab | The user changes the SQL before the run, and tabs do not multiply |
| 2 | Editor visibility | Only while the tab holds SQL | Always | At 80×24, Messages and results keep their room |
| 3 | New SQL, SQL not changed by the user | Replaces it, as one undo step | Appends it | The editor shows the current SQL. Undo and the query history keep the old SQL |
| 4 | New SQL, SQL changed by the user | An edit proposal with a diff | Replace it | User text never disappears |
| 5 | Agent run of that SQL | `run_console` with the rules of its access level | Never | No special case |
| 6 | User run in the agent tab | A user run in Auto mode, with the current run confirmations. Commit and Rollback appear for a transaction that the user opened | A manual mode | An open transaction blocks the agent tasks of the tab |
| 7 | Notice of new SQL | Status bar, agent Messages, and the `SQL ·` tab mark | The access card and the bell | The card is for decisions that only the user makes |
| 8 | Save | **New console** copies the SQL to a user console | The agent tab saves as a script | The agent tab stays temporary |
| 9 | Old behavior | Removed from the MCP | `new_console: true` | Fewer options for the agent |
| 10 | Access changes after approval | The session dialog offers every other access option, to lower or raise the access | **Switch to proposals** only | The user could not raise the access again |

## Visible MCP access requests — 2026-11-05

The user reports that the MCP row hides access requests: a user can miss them. This entry adds to decision 7 of the MCP notifications entry.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Request notice | A card with the warning color, above the MCP row, with **Review** and **Later**. It takes no focus | Open the access dialog automatically; color the MCP row only | The request is visible, and typing never goes to a dialog |
| 2 | Card place | Bottom right of the main area | Top right | At 80×24, the top covers the editor text and its cursor |
| 3 | Bell | One terminal bell for each new request. A remembered client does not ring | No bell | The user sees a request in another window |
| 4 | Other cards | Edit proposals (**Review**, **Later**) and tasks that run for more than 3 seconds (**Stop**, **Hide**) also use the card. Placed SQL and failed tasks do not | A card for every notice | The card is for items that wait for the user or can need a stop |
| 5 | Several items | One card at a time: access, then proposals, then tasks, with the place in the queue | Stacked cards | At 80×24, stacked cards cover the results |
| 6 | MCP row | Stays, with the counts and the items that the user put off | Remove it | A hidden item stays visible |

## MCP tools, waiting, and full access — 2026-11-05

The user reports a poor agent flow: too many tools, a task identifier to poll after each request, and a database that the user cannot open to the agent.
The new facts are the user's request for full access and the number of calls per task.
This entry replaces the **Database** row of the Local MCP assistant table, the **Target** rule that every call names its instance, and the reason of decision 1 of the MCP notifications entry for remembered access.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Full access | A **Full access** option executes any SQL without review. Read-only connections, the `PROD` question, and the question for a write without `WHERE` still apply; the task waits for the answer | Writes without exceptions; a question for each write; a read-only database | The agent becomes useful, and the existing protections stay |
| 2 | Grant of full access | A fourth button in the access request, without the initial focus, for the session | A saved field per connection | No configuration format change |
| 3 | Instance selection | `instance_id` is optional when one instance is open. Several instances give `ambiguous_instance` | An ID in every call | Fewer steps. The bridge never selects one of several instances |
| 4 | Waiting | The bridge waits for the end of a task, 50 seconds by default, and `request_access` waits for the decision, 120 seconds by default. A late task continues with `job` | The agent polls each task | One call returns the rows |
| 5 | Remembered access | SQLN remembers the option of a client name until it exits. **Forget** and **Revoke** remove it. A rejection is not remembered | A new question for each session | The user accepts that the client name is not verified, for less friction |
| 6 | Tools | 11 tools. `generate_sql` leaves the MCP; the interface keeps its templates | 25 tools | The agent writes SQL from the catalog and the DDL |
| 7 | Agent transaction | SQLN rolls back a transaction that agent SQL leaves open, and the task fails with the reason | Keep the transaction for the user | Nobody ends a transaction of an agent console |

## MCP notifications and decisions — 2026-11-05

The user asks for clearer MCP notices and decisions, an option that approves everything in the access request,
an agent tab that shows only its messages, and commands that stop agent work. This entry replaces the **Edits** row of the Local MCP assistant table for sessions that use **Allow all**.

| # | Point | Approved decision | Rejected | Reason |
|---|---|---|---|---|
| 1 | Scope of **Allow all** | Access and direct edits in all consoles, for this session only | Remember the client for later sessions | The client name is not verified |
| 2 | Protection of **Allow all** | A dialog button without the initial focus, with a warning in the text | A second confirmation | The warning, undo, and the `MCP n all` label are sufficient |
| 3 | Return to proposals | **Switch to proposals** in the session list, without revocation | Revocation only | Less friction |
| 4 | Agent tab content | Messages and result grids, without the editor | Messages only | The user sees the data that the agent read |
| 5 | **Stop all agent tasks** | Cancels tasks. Access stays | Also pause new tasks | Revocation already blocks new tasks |
| 6 | MCP row during execution | Shows **Stop** while an agent task runs | Status bar label only | Safety visibility |
| 7 | Access decision in the MCP row | The row has **Review** only | **Allow** and **Reject** in the row | An accidental click must not give access |
| 8 | Decision queue | After a decision, open the next pending item, with `N of M` | Return to the editor | Fewer steps |
| 9 | Notices of agent actions | Status bar and the agent Messages | Messages only | The user sees changes to their consoles |
| 10 | Default key of `agentStop` | None. The MCP row and the palette offer it | A default key | No free key is necessary now |
| 11 | MCP center | Keep separate commands | One panel for sessions, proposals, and tasks | Not necessary now |
| 12 | **Reject all** | Not offered | A button when N > 1 | Not necessary now |
| 13 | SQL that the user must run, such as `CREATE TABLE` | `open_user_console` opens a new user console with the SQL, without execution or a focus change. Agent consoles refuse edits | The agent asks the user to open a console | The write flow ends in a console that the user reviews and runs. The database stays read-only for the agent |

## Local MCP assistant

The user approves a stdio assistant for SQLN instances that are already open.
The implementation plan includes advanced read analysis in the first delivery.

| Point | Approved decision |
|---|---|
| Target | A local stdio bridge discovers open instances. Each call identifies its instance. Global-only configuration works. |
| Access | The user approves each MCP session in each SQLN instance. Client labels do not prove identity. |
| Database | Read only. Reuse existing connections, credentials, trust checks, pools, limits, and cancellation. |
| Agent console | A separate, identified console receives notifications. Background operations do not change focus. |
| User console | The agent can edit and execute its current SQL. A checked revision and explicit script/range identify the request. Results replace that console's results. |
| New SQL | Free SQL submitted through a tool executes only in that session's agent console. |
| Transactions | Refuse execution in a console with an open, failed, or unknown transaction. Offer the agent console without rerouting. |
| Edits | Show a proposal by default. Only the user accepts it. Direct editing is a user-enabled, per-console option until close or process exit. |
| Disconnect | Cancel only that session's operations. Preserve available text/results and other sessions' work. |
| Advanced analysis | Include catalog, DDL, SQL templates, plans, ER diagrams, and structure/data comparison. Generated scripts never execute automatically. |
| Outside scope | No HTTP, remote access, connection/configuration management, imports/exports, database writes, user transaction controls, or ACP yet. |

Implementation rules and limits are in [MCP](../MCP.md). Configuration, script, and workspace formats do not change.
