# Decisions — Build and workspace

Part of the [decision record](../DECISIONS.md). The rules at the top of that file apply here.

## Release profile and global allocator — 2026-11-20

`[profile.release]` had no settings, so releases had no cross-crate optimization and 16 codegen units. The user chose fat LTO, and a new allocator only if a measure proves it.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Release | `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"` | `lto = "thin"` | The user chose the faster binary over a faster release build |
| Panics | Unwinding stays | `panic = "abort"` | The main loop catches a panic to save the text of the consoles |
| Allocator | The system allocator stays | `mimalloc` 0.1.52 with its default options | `benches/rss.rs`, 4 grids of 1,000,000 rows filled by producer threads in 3 rounds: the system allocator peaked at 406 MiB and kept 10 to 52 MiB after the grids closed; `mimalloc` peaked at 424 MiB and kept 330 to 424 MiB. With `MIMALLOC_PURGE_DELAY=0` it peaked at 324 MiB and kept 16 to 19 MiB, but that option needs an `unsafe` call at start; it needs a decision of the user |
| Measure | `benches/rss.rs` reads `VmRSS` and `VmHWM` on Linux; `make bench` runs it | The counting allocator of `benches/memory.rs` | Only the resident memory shows what an allocator keeps |

## sccache in CI — 2026-11-19

The user chose a compiler cache over changes to the bundled DuckDB and the vendored OpenSSL. This complements "Build time and duplicate crates": the features and what a user needs on the system stay the same.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| CI | `mozilla-actions/sccache-action` v0.0.11 with sccache v0.19.0 and the GitHub Actions cache on every job; `CARGO_INCREMENTAL=0` | System OpenSSL or a prebuilt DuckDB | A cache changes nothing for users |
| C code | The `cc` crate follows `RUSTC_WRAPPER`; CMake gets `CMAKE_C_COMPILER_LAUNCHER` and `CMAKE_CXX_COMPILER_LAUNCHER` | `CC="sccache cc"` | A compiler with a wrapper inside `CC` breaks build scripts that read `CC` as one path |
| `rust-cache` | Kept for now | Drop it at once | The first runs measure both; if sccache covers the build, `rust-cache` keeps only the registry, and if it brings no gain, sccache goes |
| Local | Documented in the README as an optional `~/.cargo/config.toml` setting | A committed `.cargo/config.toml` | A wrapper that is not installed breaks every build |
| Measure | Pending: job times with a cold and a warm cache, and `sccache --show-stats` in the job summary | — | The cache works only on GitHub runners |

## Main loop woken by events — 2026-11-19

The main loop woke every 20 ms without work. In 10 s without input, a debug build used 0.47 s of CPU and about 600 context switches; after the change it used no measurable CPU and about 45 context switches. The answer of a query, the clock of a long run, a resize to 80×24, and `SIGTERM` during a run behave as before.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Wait | One channel for keys, signals, and wakes, with a timeout from `App::next_wakeup` | `crossterm::poll` with a longer timeout | A poll cannot end when background work delivers a result |
| Input | A thread reads the terminal after the start | Read on the main thread | Only another thread can wait on the terminal while the loop waits on the channel |
| Execution events | A relay task with a bounded channel wakes the loop | A waking sender in each crate that sends events | The senders live in core, session, drivers, and runtime; one relay covers all of them and keeps the pressure on the database |
| Background results | `spawn_blocking`, `spawn_ai`, the AI schema read, and the agent endpoint wake the loop | A waking sender per application channel | Almost every application channel gets its result from those executors |
| Safety | A timer that passed and still waits polls at 20 ms; a file save polls at 20 ms; the longest sleep is 250 ms | No limit | A missed wake or timer delays the screen and never stops it |
| `Wake` | `sqln-core`, replaceable receiver, no-op without one | One receiver for life | Tests, benches, and the batch mode run without the loop |

## No second run of failed tests — 2026-11-19

`scripts/test-summary.sh` ran a failed test binary a second time and counted only a second failure. That hid unstable tests: runs of the `sqln` binary under full CPU load failed in about half of the runs.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Retry | A failure counts at once | Retry alone; retry and report | A retry hides a race that a user can also meet |
| AI explanation tests | Wait for the end of the run, not only for its text, before a command of the finished run | Longer deadlines | The error text shows before the event that ends the run |
| Agent cancellation test | The long read names its column inside the CTE and the test checks that the job runs | Keep `n(x)` | The read policy refuses a CTE column list, so the old test never ran the read |
| Check | 17 runs of the suite without a failure, 12 under full CPU load; 9 runs with the databases | — | — |

## Build time and duplicate crates — 2026-11-19

Measured on 12 CPUs, dev profile, Rust 1.99 (which links with `rust-lld` by default on x86_64 Linux).

| Measure | Time |
|---|---|
| Clean `cargo build` | 259 s |
| `openssl-sys` build script (vendored OpenSSL of `rdkafka`) | 203 s, in parallel with the next one |
| `libduckdb-sys` build script (bundled DuckDB) | 203 s |
| `sqlparser` | 72 s |
| One-line change in `src/app/draw.rs`, `cargo build` | 1.4 s |
| One-line change in `crates/postgres/src/postgres.rs`, `cargo build` / `cargo test --workspace --no-run` | 1.8 s / 4.6 s |
| One-line change in `crates/exec/src/connection.rs`, `cargo build` / `cargo test --workspace --no-run` | 2.5 s / 5.2 s |

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Dev profile | Keep `opt-level = 1` and `debug = 0` | `split-debuginfo`, more codegen units, `mold` | The incremental builds already take a few seconds; the linker is already `rust-lld` |
| Duplicates | `signal-hook` follows the 0.3 line of `crossterm`. The other duplicate versions come from the drivers and libraries | Pin the versions of transitive crates | Only a direct dependency is ours to align. `base64` 0.23 stays, because `arrow-cast` uses it too |
| Clean build | No change | `rdkafka` with system OpenSSL (`ssl` instead of `ssl-vendored`); a prebuilt DuckDB library instead of `bundled` | Each one changes what a user needs on the system to build or run SQLN. It needs a decision of the user |

## Driver crates, connector contract, and runtime — 2026-11-14

The user asked for less dependency between the crates, so that a change recompiles less code. The user chose the full scope: a contract crate, a runtime crate, and one crate for each driver. This entry replaces the rejected alternative "One crate for each driver" of "Cargo workspace" below, and its layer order.

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Contract | `sqln-session` holds the connector contract, the connection targets, `pool`, `trace`, `tls`, `words`, and the parts of the Kafka and Redis drivers that need no driver library | Keep them in `sqln-connector` | `store`, `exec`, and `ui` use only these parts, so a driver change does not recompile them |
| Drivers | One crate for each driver: `sqln-postgres`, `sqln-mysql`, `sqln-sqlite`, `sqln-duckdb`, `sqln-redis`, `sqln-kafka`. Each one depends on `sqln-session` and on no other driver | One crate for all drivers | User choice. A change in one driver recompiles that driver, `sqln-connector`, and `sqln` |
| Factory | `sqln-connector` is a facade with the old `connector` paths. `connector::open(&Definition)` replaces `Definition::connector()` | Keep the factory in `store` | Only `sqln` opens connectors, so only `sqln` depends on the drivers |
| Vault | `sqln-vault` is its own crate | Keep it in a driver crate or in `store` | `store` and the Kafka driver both use it |
| Runtime | `sqln-runtime` holds `Manager`, the console tasks, and the jobs. `sqln-exec` keeps the event, end, option, and request types | Keep `Manager` in `sqln-exec` | `sqln-ui` uses only the types, so a runtime change does not recompile `sqln-ui` |
| Driver features | Each driver is an optional Cargo feature of `sqln-connector` and `sqln`, all on by default. Without a driver, `connector::open` returns `Unavailable`: the connection stays visible and editable, and connecting explains the missing feature | Hide the engine or refuse the saved connection | User choice. Faster builds and smaller binaries without losing saved configuration |
| Tests | A test that needs a driver lists the driver crate or `sqln-connector` as a dev-dependency | Move each such test to `sqln` | Only the test build depends on the driver |

Measured on 8 CPUs, dev profile, one-line change, after this split:

| Changed file | `cargo build` | `cargo test --workspace --no-run` |
|---|---|---|
| `postgres.rs` (driver) | 5.4–5.8 s (was 9.7 s for `connector`) | 17–18 s (was 30–34 s) |
| `exec/runner.rs` (runtime) | 4.3–6.2 s | 13 s |
| `compare` (exec models) | 7.9–10.4 s | 21 s |
| `ui` | 5.1–5.8 s (was 7.7 s) | 19 s (was 24–25 s) |
| `app` | 4.5–6.6 s (was 4.9 s) | 13–14 s (was 10–12 s) |

A crate without unit tests has `test = false`, so the new crates add only the unit test binaries of the crates that have unit tests.

## Cargo workspace — 2026-11-14

The user asked to split the project into separate crates, for a shorter compile time and for layers that the compiler checks. This entry replaces the rule "Start with a single Rust crate" of the [plan](../INITIAL_PLAN.md).

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Goal | Compile time and layers that the compiler checks | Optional connectors; reuse as a library | User choice |
| Crates | `sqln-core`, `sqln-sql`, `sqln-connector`, `sqln-store`, `sqln-exec`, `sqln-ui`, and `sqln`, in this layer order | One crate for each driver | The drivers share the session contract, and the user did not ask for optional connectors (YAGNI) |
| Paths | The `sqln` crate re-exports every module, and each lower crate re-exports its dependencies with `pub(crate) use` | Change every `crate::` and `sqln::` path | The tests and the code keep their paths |
| Lower layers | A connector sends `SinkEvent` and `TraceEvent`. `ExecEvent` implements `From` for both | Move `ExecEvent` to the connector crate | `ExecEvent` holds job types of higher layers. The sink boxes one future per batch of rows, not per row |
| Saved types | Connection definitions, execution limits, AI settings, and export and import choices move to `store` | Move `store::config` up to `sqln-exec` | The saved formats stay in one crate. No saved format changes |
| Screen states | The tab and form states of compare, diagram, data edit, export, and import move to `sqln-ui` as top-level modules | Keep them in the job modules | They use interface types. Their job models stay in `sqln-exec` |
| Version | `[workspace.package]` holds one version for all crates | One version per crate | `text` reads `CARGO_PKG_VERSION`, and the crates ship together |
| Publication | Each internal crate has `publish = false` | Publish the crates | The project does not publish to crates.io |
| Test helpers | A helper that the unit tests of another crate need uses a `test-support` feature of its crate | Make the helper always public | The helper stays out of the release binary |
| Integration tests | Each test is in the lowest crate that has all its dependencies, with one test binary per crate. Shared helpers are in `sqln-testkit` | Keep all tests in `tests/` of `sqln`; one binary per test file | User choice. A change in `app` does not link the tests of the lower crates |
| Test binaries | One integration test binary per crate. Only `vault_probe` stays alone. CI and the Makefile select a file with its module name | One binary per group of files | User choice. Each binary adds link time. Temporary folders include the test module, so tests in one binary do not share a folder |

Measured result on 8 CPUs, dev profile, one-line change, before and after the split:

| Changed file | `cargo build` | `cargo test --no-run` |
|---|---|---|
| `app` | 13.3 s → 4.9 s | 44.3 s → 45.2 s |
| `ui` | 8.8 s → 7.7 s | 45.0 s → 48.7 s |
| `connector` | 13.0 s → 9.7 s | 50.5 s → 45.8 s |
| `sql` | 13.9 s → 12.7 s | 52.5 s → 47.5 s |
| `grid` | 9.4 s → 13.7 s | — |

The link of the 22 test binaries takes most of `cargo test --no-run`, and the split does not change it. A change in a low crate recompiles each crate above it.

`cargo test --workspace --no-run` after the move of the integration tests to their crates (28 integration test binaries), and after the merge into one binary per crate (8 integration test binaries, and no unit test binary for `main.rs` and `sqln-testkit`):

| Changed file | Before the move | After the move | After the merge |
|---|---|---|---|
| `app` | 45.2 s | 39 s | 10–12 s |
| `ui` | 48.7 s | 49–56 s | 24–25 s |
| `connector` | 45.8 s | 57–60 s | 30–34 s |
| `sql` | 47.5 s | 58–59 s | 30–33 s |

The run of the test suite (`scripts/test-summary.sh`, without the build) goes from 15.5 s to 18–19 s, because more tests share one binary.

## Official version of EdTUI — 2026-11-01

The user removed the `vendor/edtui` fork, and the project goes back to the published `edtui` 0.11.7.
This decision reopens the item "4. Undo" of "Decision close" and the row "Undo limit" of "Credentials and editor".

| Point | Decision | Rejected | Reason |
|---|---|---|---|
| Undo limit | No memory cap. Each stack keeps up to 100 entries | Keep the fork; send the cap to the original project; publish the fork under another name; own history | `cargo install` ignores `[patch]`, and the fork blocked publishing. Sending it to the original project would make the release depend on the maintainer. The own history does not cap the memory in the Vim mode |
| Big entry warning | Removed | Keep the warning | Without a cap, every entry stays in the history |
| Memory risk | Technical debt, described in `ARCHITECTURE.md`, section "Known editor limit" | Block the release | User choice. Common scripts stay far below the worst case |

## Libraries

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| Drivers | `tokio-postgres`, `mysql_async`, and `rusqlite` | SQLx | `timeout` failed, cancellation and reset were missing, and the pool hid a refused connection |
| Pool | Own implementation | Library pool | Own rules of queue, reservation, and cleaning |
| Main loop | 20 ms wait on the key, with a bounded channel | Thread only for the keys | The bounded channel controls the database read, without another thread |
| Task per console | One Tokio task keeps the session | Session map in the manager | The transaction stays with its owner, and the cancellation stays with the read |
| Tokens per key | Reuse the highlight tokens in the autocomplete | Parse the text twice | With 50,000 lines, it went from 55 ms to 33 ms |
| Editor | EdTUI with a conventional layer | `ratatui-textarea` | No syntax highlight and no Vim mode |
| SQL analysis | Own tokenizer | `sqlparser-rs` or tree-sitter | Needs to tolerate incomplete text; approved prototype |
| Syntax highlight | Own tokenizer with `Highlight` | `syntect` through EdTUI | EdTUI generates RGB and resets the state on each line; the plan requires ANSI and `NO_COLOR` |
| Conventional mode editing | Own function over `state.lines` | EdTUI actions | `insert_str` counts bytes, and `Paste` treats a leading LF as a whole line |
| Undo | EdTUI stack, with groups per word | One entry per key | The stack keeps 100 entries; `DeleteChar(0)` creates the undo point |
| Redo | Counter over the EdTUI stack | Own stack | EdTUI does not clear `redo` after a new edit |
| Click in the editor | Own position calculation | EdTUI mapping | EdTUI counts one column per character and stops at the last character |
| Probe on PostgreSQL | Clock through the simple protocol | `SAVEPOINT` | `SAVEPOINT` generates log errors |
| State on MySQL | `SERVER_STATUS_IN_TRANS` of the response, and `DO 0` after an error | `SET TRANSACTION`; `performance_schema` | The probe erased the user `SET TRANSACTION` and cleared `SHOW WARNINGS`; `performance_schema` needs privileges |
| Types on PostgreSQL | `prepare` before the simple protocol | Heuristic over the text | `bytea` and numbers need the type; costs one round trip to the server |
| TLS provider | `ring`, except OpenSSL inside librdkafka for Kafka (2026-11-10) | `aws-lc-rs` | Avoids a second provider and the `aws-lc` build |

## Implementation — 2026-10-31

These choices follow the plan rules on the points the rules left open.

| Subject | Choice | Rejected | Reason |
|---|---|---|---|
| New password | Check with a test connection before writing | Write and find out on the next execution | A refused password never reaches a storage |
| First refusal | Do not repeat the execution | Repeat on its own after the new password | Simpler to explain; the action requested during the suspension continues, because no SQL left |
| Last console | Hidden console, outside the tab strip | Allow zero consoles | The code assumes an active console; the user sees no difference |
| End of the last table | End the application. Replaced on 2026-11-03: the home screen appears | Show the hidden console | "Close the last tab" is already an exit path |
| Keyboard switch | Change the mode in the editor itself | Recreate the editor and warn | Nothing is lost, so no warning is needed |
| Panic report | Place and call stack, without the message | Include the message | The message can quote SQL or a value |
| Log | Event types, without database error text | Record the messages | A database error can quote a value |
| Signals | `SIGHUP`, `SIGINT`, `SIGQUIT`, and `SIGTERM` | Only `SIGTERM` | Closing the terminal sends `SIGHUP` |
| Plain-text file | Refuse a link and an open permission | Fix the permission on its own | The same care of `ssh` with a private key |
