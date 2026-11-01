# Page button verification

The buttons live only in table tabs.
The decisions in `docs/DECISIONS.md` remain in force.
The sketch in `docs/UI.md` shows the buttons and the load bar.

## Results

| Check | Result |
|---|---|
| `cargo test --tests`, with real PostgreSQL 18 and MySQL 8.4 | 942 passed, 7 ignored, no failures |
| `cargo clippy --all-targets -- -D warnings` | No warnings |
| `cargo test --test table`, in 10 consecutive rounds | 122 passed per round |
| Injected defects in pagination | 17 detected of 17; no build failures |
| Release binary in a PTY, with real SQLite | Nine checks in each of the three layouts below |
| `cargo test --release --test latency -- --nocapture --test-threads=1` | Five passed |
| Pagination with 50,000 loaded rows | Worst key plus frame: 0.95 ms; goal: under 50 ms |

The seven ignored tests include five latency tests of the debug build.
The five latency tests pass in the release build.

The server test checks loaded pages, load by **Next**, going back, the last page, and **Load more**.
The full check finds an old expectation in the schema test.
The test now looks for the schema in the visible title, not in the button that can leave the bar.
That fix does not change the product.

## Binary in a PTY

Layouts: ANSI at 120×40, `astrodark` at 80×24, and `NO_COLOR` at 100×30.
The test sends keys and SGR mouse events to the binary.
A terminal emulator interprets the received frames.

The nine checks cover the first page, **Next** by mouse, **Previous** by mouse, and **Next** over loaded data.
They also cover last-page load, the end limit, going back by keyboard, **Load more** without movement, and a single page without buttons.

## Injected defects

A temporary copy of the code receives one defect per round.
The copy without defects passes the 122 tests before the injections.
The tests detect each change below:

1. Query again when moving forward over loaded data.
2. Not place the page at the top of the grid.
3. Keep the page when **Previous** runs.
4. Keep the page when **Next** runs.
5. Remove the target from the page request.
6. Remove the target when authorization completes.
7. Move the cursor after an interrupted load.
8. Not move the cursor after a completed load.
9. Move the cursor with **Load more**.
10. Show buttons for a single page.
11. Hide the buttons before the load of the second page.
12. Show **Next** as unavailable for loaded data.
13. Show **Previous** as available on the first page.
14. Take space from the load state to keep long buttons.
15. Keep the buttons during the row fetch.
16. Make the click on **Next** go back one page.
17. Ignore the last partial page.

## Limits

- A PTY does not replace a test in a real iTerm.
- This step does not check Nerd Font sources or server TLS.
- The LSP does not start because `rust-analyzer` is missing from the local toolchain.
  Install the component with `rustup component add rust-analyzer`, or adjust the command in `pi-lsp.json`.
