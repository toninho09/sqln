# AI SQL completion

AI completion is optional and off by default. It supports consoles associated with PostgreSQL, MySQL, SQLite, and DuckDB.
It does not execute generated SQL. It does not use MCP sessions or agent access.

## Configure

The fields live in **Settings > Autocomplete**, on one page. Choose Global or Project in Settings to pick the scope of the fields.

1. Set **Enabled** to On.
2. Set **Provider** to Custom.
3. Enter the model identifier and the API URL. Leave the URL empty to use the OpenRouter base. A base URL or full completion URL is valid.
4. Set **Key source** to System vault, Key file, or None.
5. Enter a new vault key, or the path of a key file.
6. Select **Test** to check the endpoint, if you want.
7. Select **Save** in Settings. The save writes a new vault key to the system vault.

**Max simultaneous** holds the most AI answers that wait at once in one console, counting ghosts that show and requests that run. The range is 1 to 9 and the default is 3.
**Context tokens**, **Output tokens**, and **Timeout seconds** bound the request.
**Reasoning effort** chooses None, Low, Medium, or High for models that think. The request carries `reasoning_effort` with that value. **Disabled** sends no field, so the endpoint uses its default; **None** sends `reasoning_effort: "none"`, the explicit request that the model does not think.
On the OpenRouter base, models without a reasoning control drop the field without an error, and the router maps the effort to the form of each backend. On a direct OpenAI endpoint, a model that rejects the field answers with an error. Set **Reasoning effort** back to **Disabled** to restore the compatible request.
On OpenRouter, the router also knows the `reasoning.enabled: false` switch and the effort values `minimal`, `xhigh`, and `max`. SQLN sends only the values that the OpenAI reference accepts, so other OpenAI-compatible endpoints stay valid.

The request body carries the output limit as `max_completion_tokens`, the current field of the reference. The output range starts at 16, because some providers refuse smaller values.
The **Test** button requests `/models`. It sends no SQL or database metadata.
A test failure does not prevent saving. Some compatible endpoints do not offer `/models`.
Opening or saving Settings never tests the endpoint automatically.
A save of a scope without an AI block writes none: untouched fields stay out of the file.
When the vault write of a new key fails, the save offers retry or a key that stays in the session only.

The project inherits the global provider block until it defines a custom block.
A custom block contains the provider, model, endpoint, credential reference, and limits together.
Changing a project model does not change the global provider.
Set **Provider** to Inherited to restore the global block.
Set **Enabled** to Off in the project to disable AI even when the global configuration enables it.
Disabling AI cancels outstanding requests and removes proposals.
MCP settings remain independent.

Vault keys belong to their scope and endpoint. A custom project provider needs a key for that scope.
A failed vault operation offers retry or a key that stays only in the current session. SQLN never stores an API key in `config.json` or its plaintext database-credential file.
SQLN never stores an API key in `config.json` or its plaintext database-credential file.

## Complete SQL

1. Position the cursor.
2. Open autocomplete with `Ctrl+Space`.
3. Press `Up` from the first local suggestion to select **Complete with AI…**.
4. Press the configured acceptance key: Tab, Enter, or both.
5. Approve the connection and endpoint when SQLN requests consent.
6. Review the ghost text in the editor.
7. Press `Tab` to accept the ghost at the line of the cursor, or `Esc` to reject it.

To say what the completion must do, select **Complete with AI with a request…**, the row above the plain AI row:

1. Press `Up` twice from the first local suggestion, or once in an AI-only list.
2. Press the configured acceptance key.
3. Write the request in the dialog. `Enter` sends it, `Esc` cancels, and an empty input sends a plain completion.
4. The answer shows as ghost text, with the same keys and rules as a plain completion.

A request holds one line and at most 1024 characters. Its text counts against the context window and the memory budget.
The request travels in the request body as `request`, next to `context` and `metadata`, and the system instruction tells the model to follow it.
The request never echoes in the status bar, pending messages, or logs.

The AI rows also appear above nonempty local suggestion lists during typing.
They are not initially selected. Typing never opens an AI-only list.
Explicit `Ctrl+Space` can show an AI-only list, including in comments; its first selection is the plain row.
It also works in Vim normal mode when AI is enabled.
A selection hides the AI rows: the completion works at the cursor only, and the status bar reports the reason.
The command palette offers **Complete with AI** and **Complete with AI with a request**. The commands have no separate default shortcut.

The response shows as ghost text at the requested place, in the hint style.
Several answers can wait at once, each on its own line: requests run in parallel, up to the configured limit.
A dedicated row after each ghost text offers **Accept** and **Reject**; on a narrow screen the row shows only the keys.
`Tab` decides the ghost at the line of the cursor, at its anchor; `Esc` ends the request of that line, or rejects its ghost.
Each ghost stays until the user decides: keys, clicks, commands, cursor movement, tab changes, and schema refreshes keep it, and the editor stays editable.
An edit of the anchor line closes only the ghost of that line, in silence; an edit on another line moves the anchor with the text.
Any undo takes every ghost of the console.
A new request on a line replaces the answer of that line, and a request on a free line adds one, within the limit.
During a request, an edit of the requested line stops the request at once, and an edit on another line moves the anchor of the answer.
`Run` executes the text without the ghosts, and the ghosts stay.
The `±N` mark on the tab of the console counts the ghosts that wait.
The completion never replaces a selection: it works at the cursor only. A request under a selection refuses with a message, and the selection stays.
An agent proposal can still show an [inline diff](UI.md#inline-review); the AI completion never opens one.

An animation identifies schema loading, pool waiting, and model work.
While a request runs, the requested line shows an animated mark in the first column of the gutter, as the [inline review](UI.md#inline-review) section describes.
With several requests in one console, the bar shows their count, and its **Cancel** button ends them all; `Esc` ends only the request of the line of the cursor.
SQL execution has its own Stop command.
Only an edit of the requested line, the end of the console session, or a disabled provider discards a request, as the [inline review](UI.md#inline-review) section describes. An edit of the requested line stops the request at once; the application does not wait for an answer that cannot show.
A response does not take focus from another dialog.

## Providers

SQLN speaks one standard: the OpenAI-compatible Chat Completions API, as OpenRouter uses today.

| Provider | API base |
|---|---|
| OpenRouter | `https://openrouter.ai/api/v1` (default) |
| Any OpenAI-compatible server | User supplied |

The Endpoint field accepts an API base or a full completion URL, including a trailing slash.
SQLN adds the completion route only once. A legacy full FIM URL of an old file maps to the chat route on the same base.
Credential identities keep the configured URL.

Model identifiers come from the provider. SQLN does not choose a commercial model automatically.
For Ollama, use its OpenAI-compatible base, for example `http://127.0.0.1:11434/v1`.
Install and run a local server separately. SQLN does not install, download, or load models.

Remote endpoints require HTTPS. Plain HTTP is allowed only for loopback.
URLs cannot contain credentials, query strings, or fragments. SQLN does not follow HTTP redirects.
Requests do not stream, retry automatically, or switch providers after a failure.

## Schema and limits

The first completion reads the complete accessible structure of the configured schemas.
The connection's schema filter still applies. The current schema follows the existing catalog rule.
SQLN reads table and view columns, types, defaults, generated-column metadata, keys, constraints, indexes, and supported comments.
It includes routine signatures and type names from the catalog, not routine or trigger bodies.
SQLite table and index definitions supply information such as checks and generated expressions.
Table records, result grids, other consoles, and query history do not enter the context.
SQL text and structural defaults or comments can still contain sensitive information.

The snapshot stays in memory and counts against the shared memory budget, with a 32 MiB snapshot cap.
Completions reuse it. A catalog refresh invalidates and rebuilds a previously loaded snapshot.
Existing refreshes after DDL have the same effect. Disconnect drops the snapshot.
External database changes require a refresh; SQLN does not poll for them.
Reads use the existing pool, authorization, cancellation, and database timeout.
A user's reserved transaction session is never reused for metadata.

Default limits:

| Limit | Default |
|---|---|
| Model context window | 32,768 tokens |
| Generated output | 1,024 tokens; configurable up to 4,096 |
| HTTP connection | 10 seconds |
| Complete HTTP request | 60 seconds; configurable from 1 to 600 |
| HTTP response | 1 MiB |
| Key file | 64 KiB |

Context sizing conservatively counts UTF-8 bytes and protocol overhead. It is not the model's exact tokenizer.
Set the context window no higher than the selected model supports.
When the full schema does not fit, SQLN reduces context automatically.
The metadata budget derives from provider limits alone, so one snapshot and one provider configuration produce one identical metadata section for every request.
Objects keep one canonical schema and name order; a whole object comes first, then its column metadata alone, then only its names, as space runs out.
The `focus` field of the request lists, in the same order, the schema objects that the SQL references and their direct foreign-key targets.
**Reduced context** identifies this state and gives included and omitted object counts.
The request places the stable metadata before the changing SQL context, so repeated completions on one connection send identical leading bytes and raise the prompt-cache hits of the provider.
The cached snapshot remains complete. SQLN refuses a request if its mandatory SQL interval does not fit.
A truncated model output does not become an applicable proposal.

## Rotating key files

The file contains only the UTF-8 API key, optionally followed by CR/LF.
It is not JSON, an environment file, or a command to execute.
SQLN opens and reads the configured path immediately before every API request or test.
It does not cache the file content or copy it to the vault.
Symlinks to regular files are supported for rotating secret mounts.
A missing or invalid file fails the request; SQLN never falls back to the previous key.

Relative paths resolve from the configured scope base, as other SQLN paths do.
SQLN never writes an externally managed key file. Set its permissions through your secret-management system.

Example project preference using a rotating file:

```json
{
  "aiCompletion": {
    "enabled": true,
    "provider": {
      "kind": "openrouter",
      "model": "<organization/model>",
      "credential": {
        "source": "file",
        "path": "/run/secrets/sqln-ai-key"
      }
    }
  }
}
```

Place this object under `preferences` in `config.json`.
The `aiCompletion` object and its fields are optional. Existing configuration files require no migration.
Older SQLN binaries reject the new preference because they reject unknown configuration fields.

## Explicit live test

The default suite uses a local HTTP fixture. It never calls a paid API.
An opt-in test runs the real binary in a pseudoterminal against a real OpenAI-compatible endpoint.
It requests a proposal, accepts it, checks the saved SQL, and verifies that acceptance does not execute SQL.

1. Put the API key in a private file outside the repository.
2. Run `SQLN_TEST_AI_KEY_FILE=/absolute/path/to/key SQLN_TEST_AI_MODEL=<model> cargo test --test terminal ai_chat_completion_runs_through_the_real_terminal -- --ignored --exact`. Set `SQLN_TEST_AI_BASE_URL` to use another endpoint; the default endpoint is OpenRouter.
3. Remove the key file when it is no longer needed.

The test uses a temporary configuration and a synthetic SQLite database. It does not modify the supplied key file.
Never add a key to test source, command arguments, or project configuration.

## Consent and diagnostics

SQLN asks once per project path, connection, endpoint, and credential reference.
The confirmation starts on Cancel. Changing those identities requires another approval.
Rotating the contents of the same key file does not require another approval.
Approvals stay in the private global `ai-trust.json`, never in the project.

Diagnostic logs contain no SQL, schema, prompts, model responses, API keys, or key-file contents.
HTTP errors use fixed messages rather than the external response body.
Errors give a possible next action without removing user text.
