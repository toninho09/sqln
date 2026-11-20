//! Resident memory of large results, measured by the operating system.
//!
//! Producer threads build rows, as the connectors do, and the main thread keeps them in
//! grids, as the interface does. The report shows the resident memory with the grids
//! full, after they close, and the peak. A trial of another global allocator sets it in
//! this file and in `src/main.rs`. It reads `/proc/self/status`, so it measures only on
//! Linux.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Test code: a panic is a failure.

use std::sync::mpsc::sync_channel;
use std::thread;

use sqln::grid::{Row, RowWriter, Value};
use sqln::result::ResultTab;

/// Rows of each grid, in batches of the size that a connector sends.
const ROWS: usize = 1_000_000;
const BATCH: usize = 200;
/// Grids filled at the same time, each one by its own producer thread.
const GRIDS: usize = 4;
/// Rounds of fill and close. Memory that the allocator keeps grows over the rounds.
const ROUNDS: usize = 3;

fn status(field: &str) -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find(|line| line.starts_with(field))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn mib(kib: Option<u64>) -> String {
    kib.map_or_else(
        || "unsupported".to_owned(),
        |kib| format!("{:.1} MiB", kib as f64 / 1024.0),
    )
}

fn report(name: &str) {
    println!("{name:<40} {:>14}", mib(status("VmRSS:")));
}

fn batch(start: usize, writer: &mut RowWriter) -> Vec<Row> {
    (start..start + BATCH)
        .map(|row| {
            writer.push(Value::Number(&row.to_string()));
            writer.push(Value::Text(&format!("a text value of row {row}")));
            if row.is_multiple_of(10) {
                writer.push(Value::Null);
            } else {
                writer.push(Value::Text("a \"quoted\" text, with a comma"));
            }
            writer.finish()
        })
        .collect()
}

/// Fills one grid per producer thread. The rows cross threads, as connector rows do.
fn fill() -> Vec<ResultTab> {
    let names = || vec!["id".to_owned(), "text".to_owned(), "note".to_owned()];
    let mut tabs: Vec<ResultTab> = (0..GRIDS)
        .map(|id| ResultTab::new(id + 1, names()))
        .collect();
    let (sender, receiver) = sync_channel::<(usize, Vec<Row>)>(64);
    let producers: Vec<_> = (0..GRIDS)
        .map(|grid| {
            let sender = sender.clone();
            thread::spawn(move || {
                let mut writer = RowWriter::default();
                for start in (0..ROWS).step_by(BATCH) {
                    sender.send((grid, batch(start, &mut writer))).unwrap();
                }
            })
        })
        .collect();
    drop(sender);
    for (grid, rows) in receiver {
        let bytes = rows.iter().map(Row::bytes).sum();
        tabs[grid].table.append(rows, bytes);
    }
    for producer in producers {
        producer.join().unwrap();
    }
    tabs
}

fn main() {
    println!("{GRIDS} grids of {ROWS} rows of 3 columns\n");
    report("start");
    for round in 1..=ROUNDS {
        let tabs = fill();
        report(&format!("round {round}: grids full"));
        drop(tabs);
        report(&format!("round {round}: grids closed"));
    }
    println!("{:<40} {:>14}", "peak", mib(status("VmHWM:")));
}
