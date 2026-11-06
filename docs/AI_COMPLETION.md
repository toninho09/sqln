# AI SQL completion

AI completion is optional and off by default. It supports consoles associated with PostgreSQL, MySQL, and SQLite.
It does not execute generated SQL. It does not use MCP sessions or agent access.

## Configure

1. Open **Settings > IA > Autocomplete…**.
2. Choose Global or Project in Settings before opening Autocomplete.
3. Set **Enabled** to On.
4. Set **Provider settings** to Custom.
5. Select Deepseek, Openrouter, or Compatible.
6. Enter the model identifier and, for Compatible, the API URL. A base URL or full completion URL is valid.
7. Open **Credentials** with `F2`.
8. Select System vault or Key file. Compatible also permits None.
9. Enter a new vault key, or the path of a key file.
10. Select **Apply**.
11. Select **Save** in Settings.

`F1`, `F2`, and `F3` open Provider, Credentials, and Limits. Left and Right change choices.
The optional **Test connection** button requests `/models`. It sends no SQL or database metadata.
A test failure does not prevent saving. Some compatible endpoints do not offer `/models`.
Opening or saving Settings never tests the endpoint automatically.

The project inherits the global provider block until it defines a custom block.
A custom block contains the provider, model, endpoint, credential reference, and limits together.
Changing a project model does not change the global provider.
Set **Provider settings** to Inherited to restore the global block.
Set **Enabled** to Off in the project to disable AI even when the global configuration enables it.
Disabling AI cancels outstanding requests and removes proposals.
MCP settings remain independent.

Vault keys belong to their scope and endpoint. A custom project provider needs a key for that scope.
A failed vault operation offers retry or a key that stays only in the current session.
SQLN never stores an API key in `config.json` or its plaintext database-credential file.

## Complete SQL

1. Position the cursor, or select the SQL to replace.
2. Open autocomplete with `Ctrl+Space`.
3. Press `Up` from the first local suggestion to select **Complete with AI…**.
4. Press the configured acceptance key: Tab, Enter, or both.
5. Approve the connection and endpoint when SQLN requests consent.
6. Review the response.
7. Select **Accept** or **Reject**.

The AI row also appears above nonempty local suggestion lists during typing.
It is not initially selected. Typing never opens an AI-only list.
Explicit `Ctrl+Space` can show an AI-only list, including in comments and with a selection.
It also works in Vim normal and visual modes when AI is enabled.
The command palette offers **Complete with AI**. The command has no separate default shortcut.

Without a selection, the editor shows a temporary insertion preview.
With a selection, a comparison shows removed and added text, with changed words marked.
Review starts on Reject. Tab and Shift+Tab select a button; Enter activates it; Esc rejects.
PageUp and PageDown scroll the preview or comparison. The mouse wheel also scrolls.
Long lines wrap to the available Unicode display width. Resize keeps controls accessible.
Accept applies only the captured interval as one undo step. Reject preserves text and selection.

An animation identifies schema loading, pool waiting, and model work.
Use Esc or **Cancel** to stop a request. SQL execution has its own Stop command.
Editing, cursor or selection movement, tab changes, schema changes, and refresh discard stale requests and proposals.
A response does not take focus from another dialog.

## Providers

| Provider | API base | Protocol |
|---|---|---|
| DeepSeek | `https://api.deepseek.com/beta` | Native FIM, using prefix and suffix |
| OpenRouter | `https://openrouter.ai/api/v1` | Chat Completions |
| Compatible | User supplied | Chat Completions or FIM |

The Endpoint field accepts an API base or a full completion URL, including a trailing slash.
For DeepSeek, both `https://api.deepseek.com/beta` and `https://api.deepseek.com/beta/completions` work.
SQLN adds the completion route only once. DeepSeek model discovery uses `https://api.deepseek.com/models`, not `/beta/models`.
A full URL must match the selected FIM or Chat protocol. Credential identities keep the configured URL.

Model identifiers come from the provider. SQLN does not choose a commercial model automatically.
OpenRouter models do not necessarily expose native FIM. SQLN sends prefix, selection, and suffix as chat context instead.
For Ollama, use its OpenAI-compatible base, for example `http://127.0.0.1:11434/v1`, with Chat protocol.
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
It prioritizes referenced objects, direct foreign-key neighbors, the current schema, then other objects in stable order.
Names and column metadata come before larger comments and definitions.
**Reduced context** identifies this state and gives included and omitted object counts.
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

## Explicit live DeepSeek test

The default suite uses a local HTTP fixture. It never calls a paid API.
An opt-in test runs the real binary in a pseudoterminal with `deepseek-flash` and the full DeepSeek FIM URL.
It requests a proposal, accepts it, checks the saved SQL, and verifies that acceptance does not execute SQL.

1. Put the API key in a private file outside the repository.
2. Run `SQLN_TEST_DEEPSEEK_KEY_FILE=/absolute/path/to/key cargo test --test terminal deepseek_fim_completion_runs_through_the_real_terminal -- --ignored --exact`.
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
