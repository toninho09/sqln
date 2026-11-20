# SQLN — Decision record

This document records the choices and the rejected alternatives.
The complete rules are in the [plan](INITIAL_PLAN.md).
Do not reopen a decision without a new fact.
Later dated decisions replace earlier choices on the same subject. Earlier tables preserve the historical alternatives.
Implementation notes describe their recorded stage unless a current-state note says otherwise.

The decisions are grouped by area, one file each. Inside a file, the sections keep their original order: newer first, then the early records.
Add a new decision at the top of its area file and add a line at the top of the list below.

## Areas

| Area | File |
|---|---|
| Build and workspace | [decisions/build-and-workspace.md](decisions/build-and-workspace.md) |
| Connectors and sessions | [decisions/connectors.md](decisions/connectors.md) |
| Kafka | [decisions/kafka.md](decisions/kafka.md) |
| Execution | [decisions/execution.md](decisions/execution.md) |
| Results and grid | [decisions/results-and-grid.md](decisions/results-and-grid.md) |
| Editor | [decisions/editor.md](decisions/editor.md) |
| AI, MCP, and agent | [decisions/ai-mcp-agent.md](decisions/ai-mcp-agent.md) |
| Interface | [decisions/interface.md](decisions/interface.md) |
| Files and persistence | [decisions/files-and-persistence.md](decisions/files-and-persistence.md) |
| Credentials | [decisions/credentials.md](decisions/credentials.md) |
| Product | [decisions/product.md](decisions/product.md) |

## All decisions

In the order of the original record. Undated entries are the early records or carry no date.

| Date | Decision | Area |
|---|---|---|
| 2026-11-20 | [Release profile and global allocator](decisions/build-and-workspace.md#release-profile-and-global-allocator--2026-11-20) | Build and workspace |
| 2026-11-19 | [sccache in CI](decisions/build-and-workspace.md#sccache-in-ci--2026-11-19) | Build and workspace |
| 2026-11-19 | [Main loop woken by events](decisions/build-and-workspace.md#main-loop-woken-by-events--2026-11-19) | Build and workspace |
| 2026-11-19 | [No second run of failed tests](decisions/build-and-workspace.md#no-second-run-of-failed-tests--2026-11-19) | Build and workspace |
| 2026-11-19 | [Decision record by area](decisions/product.md#decision-record-by-area--2026-11-19) | Product |
| 2026-11-19 | [Build time and duplicate crates](decisions/build-and-workspace.md#build-time-and-duplicate-crates--2026-11-19) | Build and workspace |
| 2026-11-19 | [Error kinds of the connector contract](decisions/connectors.md#error-kinds-of-the-connector-contract--2026-11-19) | Connectors and sessions |
| undated | [Table statistics and AI explanations](decisions/results-and-grid.md#table-statistics-and-ai-explanations) | Results and grid |
| 2026-11-16 | [Connection order and folders](decisions/interface.md#connection-order-and-folders--2026-11-16) | Interface |
| 2026-11-15 | [Databricks SQL connector](decisions/connectors.md#databricks-sql-connector--2026-11-15) | Connectors and sessions |
| 2026-11-14 | [Tab close and modal order](decisions/interface.md#tab-close-and-modal-order--2026-11-14) | Interface |
| 2026-11-14 | [Driver crates, connector contract, and runtime](decisions/build-and-workspace.md#driver-crates-connector-contract-and-runtime--2026-11-14) | Build and workspace |
| 2026-11-14 | [Cargo workspace](decisions/build-and-workspace.md#cargo-workspace--2026-11-14) | Build and workspace |
| 2026-11-13 | [Kafka AI completion, MCP reads, and topic filter](decisions/kafka.md#kafka-ai-completion-mcp-reads-and-topic-filter--2026-11-13) | Kafka |
| 2026-11-13 | [Kafka Postgres functions](decisions/kafka.md#kafka-postgres-functions--2026-11-13) | Kafka |
| 2026-11-13 | [Kafka schema field completion](decisions/kafka.md#kafka-schema-field-completion--2026-11-13) | Kafka |
| 2026-11-13 | [Kafka read progress](decisions/kafka.md#kafka-read-progress--2026-11-13) | Kafka |
| undated | [Visible result tabs](decisions/results-and-grid.md#visible-result-tabs) | Results and grid |
| undated | [Execute a subquery](decisions/execution.md#execute-a-subquery) | Execution |
| undated | [Execution scope and first tasks](decisions/execution.md#execution-scope-and-first-tasks) | Execution |
| undated | [Cell inspection and editor controls](decisions/results-and-grid.md#cell-inspection-and-editor-controls) | Results and grid |
| undated | [Horizontal scrollbar in the console editor](decisions/editor.md#horizontal-scrollbar-in-the-console-editor) | Editor |
| undated | [JSON colors in cell windows](decisions/results-and-grid.md#json-colors-in-cell-windows) | Results and grid |
| undated | [Text selection in the cell window](decisions/results-and-grid.md#text-selection-in-the-cell-window) | Results and grid |
| 2026-11-10 | [Side of the explorer](decisions/interface.md#side-of-the-explorer--2026-11-10) | Interface |
| 2026-11-12 | [Kafka SQL expressions](decisions/kafka.md#kafka-sql-expressions--2026-11-12) | Kafka |
| 2026-11-11 | [Kafka consumer group](decisions/kafka.md#kafka-consumer-group--2026-11-11) | Kafka |
| 2026-11-11 | [Kafka array operators](decisions/kafka.md#kafka-array-operators--2026-11-11) | Kafka |
| undated | [Kafka SELECT queries](decisions/kafka.md#kafka-select-queries) | Kafka |
| 2026-11-10 | [Kafka and Schema Registry](decisions/kafka.md#kafka-and-schema-registry--2026-11-10) | Kafka |
| 2026-11-09 | [Continuous commands](decisions/execution.md#continuous-commands--2026-11-09) | Execution |
| undated | [Batch execution of scripts](decisions/execution.md#batch-execution-of-scripts) | Execution |
| undated | [Local SQL syntax warnings](decisions/editor.md#local-sql-syntax-warnings) | Editor |
| undated | [Analysis of loaded results](decisions/results-and-grid.md#analysis-of-loaded-results) | Results and grid |
| undated | [Execution without the terminal interface](decisions/execution.md#execution-without-the-terminal-interface) | Execution |
| 2026-11-08 | [Themes with colored bases](decisions/interface.md#themes-with-colored-bases--2026-11-08) | Interface |
| 2026-11-08 | [More built-in themes](decisions/interface.md#more-built-in-themes--2026-11-08) | Interface |
| undated | [SQL files and table import](decisions/files-and-persistence.md#sql-files-and-table-import) | Files and persistence |
| 2026-11-07 | [External `.sql` files as consoles](decisions/files-and-persistence.md#external-sql-files-as-consoles--2026-11-07) | Files and persistence |
| 2026-11-06 | [Grid inserts and deletes](decisions/results-and-grid.md#grid-inserts-and-deletes--2026-11-06) | Results and grid |
| 2026-11-06 | [SQLite in memory](decisions/connectors.md#sqlite-in-memory--2026-11-06) | Connectors and sessions |
| 2026-11-06 | [Auto-refresh of the tab on screen](decisions/interface.md#auto-refresh-of-the-tab-on-screen--2026-11-06) | Interface |
| undated | [Explicit AI SQL completion](decisions/ai-mcp-agent.md#explicit-ai-sql-completion) | AI, MCP, and agent |
| 2026-11-05 | [Inline review of AI and MCP changes](decisions/ai-mcp-agent.md#inline-review-of-ai-and-mcp-changes--2026-11-05) | AI, MCP, and agent |
| 2026-11-06 | [Persistent AI ghost and buttons at each change](decisions/ai-mcp-agent.md#persistent-ai-ghost-and-buttons-at-each-change--2026-11-06) | AI, MCP, and agent |
| 2026-11-06 | [OpenRouter reference review](decisions/ai-mcp-agent.md#openrouter-reference-review--2026-11-06) | AI, MCP, and agent |
| 2026-11-06 | [Provider reasoning effort](decisions/ai-mcp-agent.md#provider-reasoning-effort--2026-11-06) | AI, MCP, and agent |
| 2026-11-06 | [Autocomplete fields in Settings](decisions/interface.md#autocomplete-fields-in-settings--2026-11-06) | Interface |
| 2026-11-06 | [AI settings in two sections](decisions/ai-mcp-agent.md#ai-settings-in-two-sections--2026-11-06) | AI, MCP, and agent |
| 2026-11-06 | [One OpenAI-compatible provider](decisions/ai-mcp-agent.md#one-openai-compatible-provider--2026-11-06) | AI, MCP, and agent |
| 2026-11-06 | [AI completion at the cursor only](decisions/ai-mcp-agent.md#ai-completion-at-the-cursor-only--2026-11-06) | AI, MCP, and agent |
| 2026-11-06 | [Several AI completions at once](decisions/ai-mcp-agent.md#several-ai-completions-at-once--2026-11-06) | AI, MCP, and agent |
| 2026-11-06 | [AI completion with a user request](decisions/ai-mcp-agent.md#ai-completion-with-a-user-request--2026-11-06) | AI, MCP, and agent |
| 2026-11-06 | [Prompt-cache-friendly completion request](decisions/ai-mcp-agent.md#prompt-cache-friendly-completion-request--2026-11-06) | AI, MCP, and agent |
| 2026-11-05 | [MCP settings in the IA section](decisions/ai-mcp-agent.md#mcp-settings-in-the-ia-section--2026-11-05) | AI, MCP, and agent |
| 2026-11-05 | [Agent tab lifetime and place](decisions/ai-mcp-agent.md#agent-tab-lifetime-and-place--2026-11-05) | AI, MCP, and agent |
| 2026-11-05 | [Session controls in the agent tab](decisions/ai-mcp-agent.md#session-controls-in-the-agent-tab--2026-11-05) | AI, MCP, and agent |
| 2026-11-05 | [Agent SQL in the agent tab](decisions/ai-mcp-agent.md#agent-sql-in-the-agent-tab--2026-11-05) | AI, MCP, and agent |
| 2026-11-05 | [Visible MCP access requests](decisions/ai-mcp-agent.md#visible-mcp-access-requests--2026-11-05) | AI, MCP, and agent |
| 2026-11-05 | [MCP tools, waiting, and full access](decisions/ai-mcp-agent.md#mcp-tools-waiting-and-full-access--2026-11-05) | AI, MCP, and agent |
| 2026-11-05 | [MCP notifications and decisions](decisions/ai-mcp-agent.md#mcp-notifications-and-decisions--2026-11-05) | AI, MCP, and agent |
| undated | [Local MCP assistant](decisions/ai-mcp-agent.md#local-mcp-assistant) | AI, MCP, and agent |
| 2026-11-04 | [Query parameters](decisions/execution.md#query-parameters--2026-11-04) | Execution |
| 2026-11-04 | [Grid row selection](decisions/results-and-grid.md#grid-row-selection--2026-11-04) | Results and grid |
| 2026-11-04 | [Grid editing](decisions/results-and-grid.md#grid-editing--2026-11-04) | Results and grid |
| 2026-11-04 | [Command discovery and width-dependent hints](decisions/interface.md#command-discovery-and-width-dependent-hints--2026-11-04) | Interface |
| 2026-11-04 | [Objects, DDL, and key navigation](decisions/interface.md#objects-ddl-and-key-navigation--2026-11-04) | Interface |
| 2026-11-04 | [Quit button in the tab row](decisions/interface.md#quit-button-in-the-tab-row--2026-11-04) | Interface |
| 2026-11-04 | [Search path of a console schema in PostgreSQL](decisions/connectors.md#search-path-of-a-console-schema-in-postgresql--2026-11-04) | Connectors and sessions |
| 2026-11-04 | [Visible schemas of a connection](decisions/connectors.md#visible-schemas-of-a-connection--2026-11-04) | Connectors and sessions |
| 2026-11-03 | [Lists with a search in the compare dialog](decisions/interface.md#lists-with-a-search-in-the-compare-dialog--2026-11-03) | Interface |
| 2026-11-03 | [Compare in the hint row and the `Ctrl+W` menu](decisions/interface.md#compare-in-the-hint-row-and-the-ctrlw-menu--2026-11-03) | Interface |
| 2026-11-03 | [Schema compare and data compare](decisions/interface.md#schema-compare-and-data-compare--2026-11-03) | Interface |
| 2026-11-03 | [Messages command history](decisions/interface.md#messages-command-history--2026-11-03) | Interface |
| 2026-11-03 | [Home screen](decisions/interface.md#home-screen--2026-11-03) | Interface |
| 2026-11-03 | [Idle limit and connection form with a sidebar](decisions/connectors.md#idle-limit-and-connection-form-with-a-sidebar--2026-11-03) | Connectors and sessions |
| 2026-11-03 | [Horizontal wheel on the grid](decisions/results-and-grid.md#horizontal-wheel-on-the-grid--2026-11-03) | Results and grid |
| 2026-11-03 | [`LIMIT` with the row cap](decisions/execution.md#limit-with-the-row-cap--2026-11-03) | Execution |
| 2026-11-03 | [ER diagram in the hint row and the `Ctrl+W` menu](decisions/interface.md#er-diagram-in-the-hint-row-and-the-ctrlw-menu--2026-11-03) | Interface |
| 2026-11-03 | [ER diagram](decisions/interface.md#er-diagram--2026-11-03) | Interface |
| 2026-11-02 | [Consoles in the tree of the explorer](decisions/interface.md#consoles-in-the-tree-of-the-explorer--2026-11-02) | Interface |
| 2026-11-02 | [`Ctrl+W` menu by panel](decisions/interface.md#ctrlw-menu-by-panel--2026-11-02) | Interface |
| 2026-11-02 | [Explorer commands in the `Ctrl+W` menu](decisions/interface.md#explorer-commands-in-the-ctrlw-menu--2026-11-02) | Interface |
| 2026-11-02 | [Query plan and server sessions](decisions/connectors.md#query-plan-and-server-sessions--2026-11-02) | Connectors and sessions |
| 2026-11-02 | [MySQL client key](decisions/connectors.md#mysql-client-key--2026-11-02) | Connectors and sessions |
| 2026-11-02 | [Import from a file](decisions/files-and-persistence.md#import-from-a-file--2026-11-02) | Files and persistence |
| 2026-11-02 | [Undo history of the conventional mode](decisions/editor.md#undo-history-of-the-conventional-mode--2026-11-02) | Editor |
| 2026-11-01 | [Tree filter, database notices, bell, transaction age, SQL generation, and `JOIN`](decisions/results-and-grid.md#tree-filter-database-notices-bell-transaction-age-sql-generation-and-join--2026-11-01) | Results and grid |
| 2026-11-01 | [Official version of EdTUI](decisions/build-and-workspace.md#official-version-of-edtui--2026-11-01) | Build and workspace |
| 2026-11-01 | [Vertical row, JSON, comment, and row count](decisions/results-and-grid.md#vertical-row-json-comment-and-row-count--2026-11-01) | Results and grid |
| 2026-11-01 | [Command-line target, run protection, formatter, and history](decisions/execution.md#command-line-target-run-protection-formatter-and-history--2026-11-01) | Execution |
| 2026-10-31 | [Hint line](decisions/interface.md#hint-line--2026-10-31) | Interface |
| 2026-10-31 | [Structure tab](decisions/results-and-grid.md#structure-tab--2026-10-31) | Results and grid |
| 2026-10-31 | [File explorer](decisions/files-and-persistence.md#file-explorer--2026-10-31) | Files and persistence |
| 2026-10-31 | [Export to file](decisions/files-and-persistence.md#export-to-file--2026-10-31) | Files and persistence |
| 2026-10-31 | [Redis and Valkey](decisions/connectors.md#redis-and-valkey--2026-10-31) | Connectors and sessions |
| 2026-10-31 | [nvim-style interface](decisions/interface.md#nvim-style-interface--2026-10-31) | Interface |
| 2026-10-31 | [Decision close](decisions/product.md#decision-close--2026-10-31) | Product |
| undated | [State of the credential delivery](decisions/credentials.md#state-of-the-credential-delivery) | Credentials |
| 2026-10-31 | [Credentials and editor](decisions/credentials.md#credentials-and-editor--2026-10-31) | Credentials |
| undated | [Execution and results](decisions/execution.md#execution-and-results) | Execution |
| undated | [Files and saving](decisions/files-and-persistence.md#files-and-saving) | Files and persistence |
| undated | [Table view](decisions/results-and-grid.md#table-view) | Results and grid |
| undated | [Session and connection](decisions/connectors.md#session-and-connection) | Connectors and sessions |
| undated | [Libraries](decisions/build-and-workspace.md#libraries) | Build and workspace |
| undated | [Product](decisions/product.md#product) | Product |
| 2026-10-31 | [Implementation](decisions/build-and-workspace.md#implementation--2026-10-31) | Build and workspace |
| 2026-10-31 | [Themes](decisions/interface.md#themes--2026-10-31) | Interface |
| 2026-11-07 | [UI kit](decisions/interface.md#ui-kit--2026-11-07) | Interface |
| 2026-11-07 | [DuckDB](decisions/connectors.md#duckdb--2026-11-07) | Connectors and sessions |
| 2026-11-08 | [Custom themes](decisions/interface.md#custom-themes--2026-11-08) | Interface |
| 2026-11-13 | [Getting-started guide in the help](decisions/interface.md#getting-started-guide-in-the-help--2026-11-13) | Interface |
