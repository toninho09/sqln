# Decisions — Product

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## Decision record by area — 2026-11-19

The record had 107 sections in one file of 2,700 lines.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Layout | `docs/DECISIONS.md` keeps the rules and an index; each area has a file in `docs/decisions/` | One file; one file per decision | One file was hard to search; a file per decision scatters related choices |
| Content | Sections move without a change of text, in their original order | Rewrite or merge old entries | The history stays the same, and `git log -L` still finds it |
| Links | `docs/DECISIONS.md` keeps its path | Move the whole record | The documents and `AGENTS.md` link to that path |

## Decision close — 2026-10-31

**Defined:** the user accepts all 12 recommendations below.
The approval closes the decisions, but does not declare the changes as implemented.
Section 1 of the plan separates the current code from the missing adjustments.

| Point | Approved decision | Reference |
|---|---|---|
| 1. Rename consoles | `Ctrl+W N` and action in the list; refuse an empty or repeated name in the connection; keep identity and files | Plan, section 12 |
| 2. Change connection | `Ctrl+W a`, confirmation, previous save, text preservation, and result clearing; do not run or convert SQL | Plan, section 12 |
| 3. CTEs | Suggest identifiable names and columns of `WITH`, with aliases and scope; do not query the database to find them | Plan, section 6 |
| 4. Undo | Up to 100 entries and 64 MiB per console; remove the oldest and warn about a single entry above the budget | Plan, section 5 |
| 5. Hints and help | Show the effective `keymap` keys and only available actions, including in the prefix | Plan, section 5 |
| 6. Settings | `Ctrl+W ,`, scopes, inherited values, and the five fields of the sketch; warn before losing selection or history on keyboard change | Plan, section 11 |
| 7. Files | Approve the existing saving and conflicts; use **Close and discard text** for the removed file | Plan, section 12 |
| 8. Lists | Approve the console and scripts-without-connection lists, delete confirmation, and attach without execution | Plan, section 12 |
| 9. Configuration and state | Approve reload, lock on error, and scopes; warn without blocking when `workspace.json` is unreadable | Plan, sections 11 and 12 |
| 10. Tables | Approve opening, labels, sorting, and pages; mix tabs in the opening order and do not create a console when closing the last one with a table open | Plan, section 10 |
| 11. Credentials | Initial session-only use, explicit storage, confirmed recovery and migration; trust with focus on Cancel | Plan, section 13 |
| 12. Vault | `keyring-core`, Keychain, and Secret Service; unknown failure is an error, never an absence; validate on real services | Plan, section 13; architecture, section 9 |

Real terminal, mouse, network disk, certificates, and network failures stay as pending validations.

## Product

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Delivery | Complete first version, with local versioning | Public partial versions | Project decision |
| Phase order | Complete interface before the table view | Table first, with a provisional opening | The table opens through the navigator and uses the tabs; the provisional opening would be removed later |
| TSV | DataGrip format, minus the data losses | Exact DataGrip copy | The copy must not lose data in silence |
| Shortcuts on Mac | Without depending on `Ctrl`+arrows, `Home`, `End`, `PageUp`, and `PageDown` | Classic IDE shortcuts | Part does not work, and many keyboards lack those keys |
| Prefix | `Ctrl+W` in both modes | Close tab with `Ctrl+W` | Common path, without depending on the terminal |
| Quit | `Ctrl+W q` | Only close the last tab | The exit with several consoles needs a command; `q` follows Vim |
| Add connection | `a` in the navigator | Item in the tree | The empty navigator already offers a path, and the key follows `n`, `e`, and `r` |
| Simultaneous questions | Queue, with one open question | Replace the open question | Two consoles can ask for an answer at the same moment |
| Automatic name | Smallest available `console-N` in the connection, among open and saved consoles | Single application counter | The console sets belong to the connections |
| Password in the form | Masked field; session as the initial option and vault by explicit choice | Save automatically | The user authorizes the secret storage |
| Password in the URI | Mask on screen, with the same size | Show the typed text | The plan forbids showing passwords; the cursor stays correct |
| Commit by shortcut | Without confirmation | Ask for confirmation | Same behavior as the buttons |
| Metadata | One `.json` beside each `.sql` | Single index or comment in the script | Reduces Git conflicts and keeps the script clean |
| Telemetry | None | Optional | The application only touches the network to talk to the databases |
| List opening | While typing, besides `Ctrl+Space` | Only `Ctrl+Space` | JetBrains is the reference; `Enter` keeps breaking the line |
| Keyword case | Follows the typed text | Always uppercase | Respects the script style |
| Search | Literal text, case-insensitive | Regular expression | IDE pattern; reduces the scope |
| `COPY` with `STDOUT` | Refuse, like `STDIN` | Treat as a driver error | The protocol does not fit the grid, and the reason stays clear |
| Error position | Underline the token without moving the cursor | Move the cursor | The user can keep editing during the execution |
| Detail window | Wrap the lines on demand | Ratatui `Paragraph` or cut text | A 40 MB value cost 2.3 s per frame; now it costs 0.4 ms, with the full text |
| Replace all | `Ctrl+A` in the replace field | `Alt+A` | On macOS, `Option`+letter depends on the terminal |
| External `.sql` file | After the first version | In the first version | Reduces the scope |
| License | `MIT OR Apache-2.0` | GPL or proprietary | Rust ecosystem standard |
| Example catalog | Tests only, without `--no-catalog` | Active by default in the binary | Without a catalog, section 6 asks for keywords; invented tables mislead the user |
| Publishing | Remove `publish = false` at release | Keep, with binaries only | `cargo install sqln` depends on crates.io |
| Catalog responses | Number on each read; the old response leaves | Accept any response | An old read could erase from the navigator a table just created |
| Stop test | Wait for the real start of the statement | Wait fixed 300 ms | Under load, the connection took more than that, and the product acted correctly |
