# Decisions — Credentials

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## State of the credential delivery

The code implements the credential phase, with authorized plain text, migration, refused password, and separate deletion.
Keychain and Secret Service in a container are validated; the Secret Service still needs a repeat on a real Linux desktop.
The [vault validation](../INITIAL_PLAN.md#validation-of-the-real-vaults) records the native checks and pending desktop validation.
The tests with an in-memory vault and real databases do not replace that native validation.
The missing-vault criterion comes from the validation: only responses confirmed on Linux, never on macOS.

## Credentials and editor — 2026-10-31

**Defined:** the user approves these points after reading the code.
The approval closes the decisions, but does not declare the changes as implemented.

| Point | Approved decision | Rejected | Reason |
|---|---|---|---|
| Keychain | Validate on this machine, with the user present | Postpone | Plain text depends on that validation |
| Secret Service | Linux container now and a real desktop before the release | Real desktop only | The container covers missing service, lock, and common operations, but not a desktop session |
| Plain-text password | `credentials.json` in the global folder, with `0600`, per identity, for both scopes | `password` in the global `config.json` and a section for project connections | `config.json` stays without secrets, and both scopes use one format |
| End of the suspension | Each explicit action opens the password prompt; an accepted password or the connection edit ends the suspension | Only **Disconnect** or edit | Executions and catalog stop insisting, without an extra step for the user |
| Undo limit | Small fork of `edtui` through `[patch]`; at release, send to the original project or publish under another name | Copy the code into `src/`; own history | The change stays in `Stack`; the copy brings 10,169 third-party lines, and the own history is fragile |
