use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{temp_db, temp_dir};
use sqln::clipboard::{base64, base64_bytes};
use sqln::connector::sqlite::{SqliteConnector, SqliteTarget};
use sqln::exec::{ConsoleId, ExecEvent, ExecutionEnd, Manager, RunOptions, Settings};
use sqln::export::{Delimiter, ExportFormat, ExportOptions, ExportWriter, NullText};
use sqln::grid::{Budget, Row, Value};
use sqln::import::feed::{Feed, LotEnd};
use sqln::import::statement::{MAX_STATEMENT_BYTES, Target, TargetColumn, is_binary_type};
use sqln::import::{
    BadRow, Encoding, FileReader, FileRow, ImportFormat, ImportJob, ImportMode, ImportProgress,
    Input, Item, KEY_OBJECTS, PREVIEW_ROWS, ReadOptions, detect, log_path, probe, read_columns,
};
use sqln::sql::Dialect;
use tokio::sync::mpsc::Receiver;

fn input(bytes: &[u8]) -> Input {
    Box::new(Cursor::new(bytes.to_vec()))
}

fn names(columns: &[&str]) -> Vec<String> {
    columns.iter().map(|name| (*name).to_owned()).collect()
}

fn format(format: ImportFormat) -> ReadOptions {
    ReadOptions {
        format,
        ..ReadOptions::default()
    }
}

/// Reads all rows with `width` columns. The header stays out of the rows.
fn items(bytes: &[u8], options: &ReadOptions, columns: &[&str]) -> Vec<Item> {
    let mut reader = FileReader::new(input(bytes), options, &names(columns)).unwrap();
    let mut items = Vec::new();
    while let Some(item) = reader.next_item().unwrap() {
        items.push(item);
    }
    items
}

fn cells(item: &Item) -> Vec<Option<&str>> {
    match item {
        Item::Row(row) => row.cells.iter().map(Option::as_deref).collect(),
        Item::Bad(row) => panic!("bad row: {row:?}"),
    }
}

fn bad(item: &Item) -> &BadRow {
    match item {
        Item::Bad(row) => row,
        Item::Row(row) => panic!("good row: {row:?}"),
    }
}

fn row(item: &Item) -> &FileRow {
    match item {
        Item::Row(row) => row,
        Item::Bad(row) => panic!("bad row: {row:?}"),
    }
}

fn read_error(bytes: &[u8], options: &ReadOptions, columns: &[&str]) -> String {
    let mut reader = FileReader::new(input(bytes), options, &names(columns)).unwrap();
    loop {
        match reader.next_item() {
            Ok(Some(_)) => {}
            Ok(None) => panic!("the file has no fault"),
            Err(reason) => return reason,
        }
    }
}

fn csv() -> ReadOptions {
    ReadOptions::default()
}

fn no_header() -> ReadOptions {
    ReadOptions {
        header: false,
        ..ReadOptions::default()
    }
}

#[test]
fn csv_reads_quotes_doubled_quotes_and_line_ends() {
    let file =
        b"a,b\n\"x,y\",\"say \"\"hi\"\"\"\n\"one\ntwo\",\"three\r\nfour\"\r\nplain,\"last\"\n";
    let rows = items(file, &csv(), &["a", "b"]);
    assert_eq!(rows.len(), 3);
    assert_eq!(cells(&rows[0]), [Some("x,y"), Some("say \"hi\"")]);
    assert_eq!(cells(&rows[1]), [Some("one\ntwo"), Some("three\r\nfour")]);
    assert_eq!(cells(&rows[2]), [Some("plain"), Some("last")]);
}

#[test]
fn a_quote_inside_plain_text_stays_in_the_value() {
    let rows = items(b"5\" disk,\"a\"b\n", &no_header(), &["a", "b"]);
    assert_eq!(cells(&rows[0]), [Some("5\" disk"), Some("ab")]);
}

#[test]
fn an_empty_field_is_null_and_empty_quotes_are_text() {
    let rows = items(b"a,b\n,\"\"\n", &csv(), &["a", "b"]);
    assert_eq!(cells(&rows[0]), [None, Some("")]);
}

#[test]
fn the_null_word_marks_only_a_field_without_quotes() {
    let options = ReadOptions {
        null_text: NullText::Word,
        ..ReadOptions::default()
    };
    let rows = items(b"a,b,c\nNULL,\"NULL\",\n", &options, &["a", "b", "c"]);
    assert_eq!(cells(&rows[0]), [None, Some("NULL"), Some("")]);
}

#[test]
fn a_bom_stays_out_of_the_first_column() {
    let file = b"\xef\xbb\xbfname,age\nAna,30\n";
    let columns = read_columns(input(file), &csv()).unwrap();
    assert_eq!(columns, ["name", "age"]);
    let rows = items(file, &csv(), &["name", "age"]);
    assert_eq!(cells(&rows[0]), [Some("Ana"), Some("30")]);
}

#[test]
fn windows_1252_reads_each_byte() {
    let options = ReadOptions {
        encoding: Encoding::Windows1252,
        ..no_header()
    };
    let rows = items(b"a\xe7\xe3o\x80\x85\x9f,\xff\n", &options, &["a", "b"]);
    assert_eq!(cells(&rows[0]), [Some("ação€…Ÿ"), Some("ÿ")]);
}

#[test]
fn wrong_utf8_is_a_problem_of_one_row() {
    let rows = items(b"a\xe7b\nok\n", &no_header(), &["a"]);
    assert!(bad(&rows[0]).reason.contains("Windows-1252"));
    assert_eq!(cells(&rows[1]), [Some("ok")]);
}

#[test]
fn a_wrong_number_of_fields_names_its_line() {
    let file = b"a,b\n1,2\n\"x\ny\",3\n4\n5,6\n";
    let options = ReadOptions {
        keep_source: true,
        ..ReadOptions::default()
    };
    let rows = items(file, &options, &["a", "b"]);
    assert_eq!(row(&rows[0]).line, 2);
    assert_eq!(row(&rows[1]).line, 3);
    assert_eq!(row(&rows[1]).source, "\"x\ny\",3");
    let failed = bad(&rows[2]);
    assert_eq!(failed.line, 5);
    assert_eq!(failed.source, "4");
    assert!(failed.reason.contains("1 fields"), "{}", failed.reason);
    assert_eq!(row(&rows[3]).line, 6);
}

#[test]
fn the_source_of_a_row_stays_empty_without_the_log() {
    let rows = items(b"1\n", &no_header(), &["a"]);
    assert_eq!(row(&rows[0]).source, "");
}

#[test]
fn quotes_without_an_end_stop_the_file() {
    let reason = read_error(b"a,b\n1,2\n3,\"open\nmore\n", &csv(), &["a", "b"]);
    assert_eq!(reason, "The quotes that start at line 3 have no end.");
}

#[test]
fn columns_come_from_the_header_or_from_a_count() {
    assert_eq!(
        read_columns(input(b"id,,\"full name\"\n1,2,3\n"), &csv()).unwrap(),
        ["id", "column_2", "full name"]
    );
    assert_eq!(
        read_columns(input(b"1,2\n"), &no_header()).unwrap(),
        ["column_1", "column_2"]
    );
    assert!(read_columns(input(b""), &csv()).unwrap().is_empty());
}

#[test]
fn a_blank_line_is_skipped_except_with_one_column() {
    let rows = items(b"a,b\n\n1,2\n\n", &csv(), &["a", "b"]);
    assert_eq!(rows.len(), 1);
    let rows = items(b"a\n1\n\n2\n", &csv(), &["a"]);
    assert_eq!(rows.len(), 3);
    assert_eq!(cells(&rows[1]), [None]);
}

#[test]
fn a_blank_line_before_the_header_is_not_the_header() {
    let rows = items(b"\na\n1\n", &csv(), &["a"]);
    assert_eq!(rows.len(), 1);
    assert_eq!(cells(&rows[0]), [Some("1")]);
}

#[test]
fn each_delimiter_splits_a_row() {
    for (delimiter, file) in [
        (Delimiter::Semicolon, &b"1;2,3\n"[..]),
        (Delimiter::Pipe, &b"1|2,3\n"[..]),
    ] {
        let options = ReadOptions {
            delimiter,
            ..no_header()
        };
        let rows = items(file, &options, &["a", "b"]);
        assert_eq!(cells(&rows[0]), [Some("1"), Some("2,3")]);
    }
    let options = ReadOptions {
        header: false,
        ..format(ImportFormat::Tsv)
    };
    let rows = items(b"1\t\"a\tb\"\n", &options, &["a", "b"]);
    assert_eq!(cells(&rows[0]), [Some("1"), Some("a\tb")]);
}

#[test]
fn the_file_suggests_the_format_and_the_delimiter() {
    let of = |name: &str, line: &[u8]| {
        let options = detect(Path::new(name), line, ReadOptions::default());
        (options.format, options.delimiter)
    };
    assert_eq!(of("a.csv", b"a,b"), (ImportFormat::Csv, Delimiter::Comma));
    assert_eq!(
        of("a.csv", b"a;b;\"c;d;e;f\",g"),
        (ImportFormat::Csv, Delimiter::Semicolon)
    );
    assert_eq!(of("a.TXT", b"a|b"), (ImportFormat::Csv, Delimiter::Pipe));
    assert_eq!(of("a.csv", b"a\tb"), (ImportFormat::Tsv, Delimiter::Comma));
    assert_eq!(of("a.tsv", b"a,b"), (ImportFormat::Tsv, Delimiter::Comma));
    assert_eq!(of("a.json", b"[{"), (ImportFormat::Json, Delimiter::Comma));
    assert_eq!(
        of("a.jsonl", b"{"),
        (ImportFormat::JsonLines, Delimiter::Comma)
    );
    assert_eq!(
        of("a.ndjson", b"{"),
        (ImportFormat::JsonLines, Delimiter::Comma)
    );
    assert_eq!(of("a.csv", b"plain"), (ImportFormat::Csv, Delimiter::Comma));
}

#[test]
fn json_keeps_the_order_of_the_keys_and_reads_each_kind_of_value() {
    let file = br#"[
      {"z": 1.50, "a": "t\"x\n", "n": null, "b": true, "c": false, "o": {"k": [1, 2]}},
      {"a": "only"}
    ]"#;
    let options = format(ImportFormat::Json);
    let columns = read_columns(input(file), &options).unwrap();
    assert_eq!(columns, ["z", "a", "n", "b", "c", "o"]);
    let rows = items(file, &options, &["z", "a", "n", "b", "c", "o"]);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        cells(&rows[0]),
        [
            Some("1.50"),
            Some("t\"x\n"),
            None,
            Some("1"),
            Some("0"),
            Some(r#"{"k": [1, 2]}"#)
        ]
    );
    assert_eq!(
        cells(&rows[1]),
        [None, Some("only"), None, None, None, None]
    );
}

#[test]
fn a_boolean_follows_the_dialect() {
    let options = ReadOptions {
        dialect: Dialect::Postgres,
        ..format(ImportFormat::JsonLines)
    };
    let rows = items(br#"{"a": true, "b": false}"#, &options, &["a", "b"]);
    assert_eq!(cells(&rows[0]), [Some("true"), Some("false")]);
}

#[test]
fn json_keeps_all_digits_of_a_large_number() {
    let file = br#"[{"a": 123456789012345678901234567890.123456789}]"#;
    let rows = items(file, &format(ImportFormat::Json), &["a"]);
    assert_eq!(
        cells(&rows[0]),
        [Some("123456789012345678901234567890.123456789")]
    );
}

#[test]
fn brackets_inside_a_string_do_not_end_a_value() {
    let file = br#"[{"a": "]}\"{["}, {"a": "x"}]"#;
    let rows = items(file, &format(ImportFormat::Json), &["a"]);
    assert_eq!(cells(&rows[0]), [Some("]}\"{[")]);
    assert_eq!(cells(&rows[1]), [Some("x")]);
}

#[test]
fn a_value_that_is_not_an_object_is_a_problem_of_one_row() {
    let file = b"[1, \"x\", true, [1], {\"a\": 1}]";
    let rows = items(file, &format(ImportFormat::Json), &["a"]);
    assert_eq!(rows.len(), 5);
    for row in &rows[..4] {
        assert_eq!(bad(row).reason, "This value is not a JSON object.");
    }
    assert_eq!(cells(&rows[4]), [Some("1")]);
}

#[test]
fn a_wrong_inner_syntax_is_a_problem_of_one_row() {
    let file = b"[{\"a\": tru}, {\"a\": 1}]";
    let rows = items(file, &format(ImportFormat::Json), &["a"]);
    assert_eq!(rows.len(), 2);
    assert!(!bad(&rows[0]).reason.is_empty());
    assert_eq!(cells(&rows[1]), [Some("1")]);
}

#[test]
fn a_json_file_without_an_array_stops() {
    let options = format(ImportFormat::Json);
    assert_eq!(
        read_error(b"{\"a\": 1}", &options, &["a"]),
        "A JSON file must hold an array of objects."
    );
    assert_eq!(
        read_error(b"[{\"a\": 1}", &options, &["a"]),
        "The JSON file ends before its array ends."
    );
    assert_eq!(
        read_error(b"[{\"a\": \"x", &options, &["a"]),
        "The JSON file ends before its array ends."
    );
    assert_eq!(
        read_error(b"[{\"a\": 1}\n\n{\"a\": 2}]", &options, &["a"]),
        "Line 3 does not continue the JSON array."
    );
    assert_eq!(
        read_error(b"[{\"a\": 1},\n]", &options, &["a"]),
        "Line 2 does not continue the JSON array."
    );
    assert_eq!(
        read_error(b"[]\n[]", &options, &[]),
        "Line 2 has text after the end of the JSON array."
    );
}

#[test]
fn an_empty_json_file_has_no_rows() {
    let options = format(ImportFormat::Json);
    assert!(items(b"", &options, &["a"]).is_empty());
    assert!(items(b"[]\n", &options, &["a"]).is_empty());
    assert!(read_columns(input(b"[]"), &options).unwrap().is_empty());
}

#[test]
fn json_names_the_line_of_each_object() {
    let file = b"[\n  {\"a\": 1},\n\n  {\"a\": 2}\n]\n";
    let rows = items(file, &format(ImportFormat::Json), &["a"]);
    assert_eq!(row(&rows[0]).line, 2);
    assert_eq!(row(&rows[1]).line, 4);
}

#[test]
fn json_lines_skips_blank_lines_and_continues_after_a_bad_line() {
    let file = b"{\"a\": 1}\n\n{broken\n\xff\n[1]\n{\"a\": 2}\n";
    let options = ReadOptions {
        keep_source: true,
        ..format(ImportFormat::JsonLines)
    };
    let rows = items(file, &options, &["a"]);
    assert_eq!(rows.len(), 5);
    assert_eq!(row(&rows[0]).line, 1);
    assert_eq!(bad(&rows[1]).line, 3);
    assert_eq!(bad(&rows[1]).source, "{broken");
    assert!(bad(&rows[2]).reason.contains("UTF-8"));
    assert_eq!(bad(&rows[3]).reason, "This value is not a JSON object.");
    assert_eq!(row(&rows[4]).line, 6);
    assert_eq!(cells(&rows[4]), [Some("2")]);
}

#[test]
fn the_keys_come_from_the_first_objects_only() {
    let mut file = String::new();
    for _ in 0..KEY_OBJECTS {
        file.push_str("{\"a\": 1}\n");
    }
    file.push_str("{\"a\": 2, \"late\": 3}\n");
    let options = format(ImportFormat::JsonLines);
    let columns = read_columns(input(file.as_bytes()), &options).unwrap();
    assert_eq!(columns, ["a"]);
    let mut reader = FileReader::new(input(file.as_bytes()), &options, &columns).unwrap();
    let mut last = None;
    while let Some(item) = reader.next_item().unwrap() {
        last = Some(item);
    }
    assert_eq!(cells(&last.unwrap()), [Some("2")]);
    assert_eq!(reader.ignored_keys(), 1);
}

fn target(dialect: Dialect) -> Target {
    Target {
        table: "t".to_owned(),
        columns: vec![
            TargetColumn {
                name: "a".to_owned(),
                binary: false,
                source: 0,
            },
            TargetColumn {
                name: "b".to_owned(),
                binary: true,
                source: 1,
            },
        ],
        dialect,
        base64: false,
    }
}

fn values(target: &Target, cells: &[Option<&str>]) -> Result<String, String> {
    let cells: Vec<Option<String>> = cells.iter().map(|cell| cell.map(str::to_owned)).collect();
    target.values(&cells)
}

#[test]
fn the_head_quotes_the_names_of_the_dialect() {
    assert_eq!(
        target(Dialect::Postgres).head(),
        "INSERT INTO t (\"a\", \"b\") VALUES "
    );
    assert_eq!(
        target(Dialect::MySql).head(),
        "INSERT INTO t (`a`, `b`) VALUES "
    );
}

#[test]
fn a_text_literal_keeps_its_meaning_in_each_dialect() {
    let hostile = Some(r"x'); DROP TABLE t; --\");
    assert_eq!(
        values(&target(Dialect::Sqlite), &[hostile, None]).unwrap(),
        r"('x''); DROP TABLE t; --\', NULL)"
    );
    assert_eq!(
        values(&target(Dialect::Postgres), &[hostile, None]).unwrap(),
        r"(E'x''); DROP TABLE t; --\\', NULL)"
    );
    assert_eq!(
        values(&target(Dialect::MySql), &[Some(r"a\b"), None]).unwrap(),
        "(_utf8mb4 0x615C62, NULL)"
    );
    assert_eq!(
        values(&target(Dialect::MySql), &[Some("it's"), None]).unwrap(),
        "('it''s', NULL)"
    );
}

#[test]
fn a_nul_byte_is_a_row_error() {
    let target = target(Dialect::Sqlite);
    let reason = values(&target, &[Some("a\0b"), None]).unwrap_err();
    assert_eq!(reason, "The value of a holds a NUL byte.");
}

#[test]
fn hexadecimal_text_in_a_binary_column_becomes_binary() {
    let of = |dialect, value| values(&target(dialect), &[None, Some(value)]).unwrap();
    assert_eq!(of(Dialect::Sqlite, "0xABcd"), "(NULL, X'ABcd')");
    assert_eq!(of(Dialect::MySql, "0xAB"), "(NULL, X'AB')");
    assert_eq!(of(Dialect::Postgres, "0xAB"), r"(NULL, E'\\xAB')");
}

#[test]
fn other_text_in_a_binary_column_becomes_its_bytes() {
    let of = |dialect, value| values(&target(dialect), &[None, Some(value)]).unwrap();
    assert_eq!(of(Dialect::Sqlite, "0xABC"), "(NULL, X'3078414243')");
    assert_eq!(of(Dialect::Sqlite, "ab"), "(NULL, X'6162')");
    assert_eq!(of(Dialect::Sqlite, ""), "(NULL, X'')");
    assert_eq!(of(Dialect::Postgres, r"a\b"), r"(NULL, E'\\x615C62')");
}

#[test]
fn base64_in_a_binary_column_becomes_binary() {
    let target = Target {
        base64: true,
        ..target(Dialect::Sqlite)
    };
    assert_eq!(
        values(&target, &[None, Some("AQL/")]).unwrap(),
        "(NULL, X'0102FF')"
    );
    assert_eq!(
        values(&target, &[None, Some("no base64!")]).unwrap_err(),
        "The value of b is not valid Base64."
    );
}

#[test]
fn base64_bytes_reads_what_base64_writes() {
    for bytes in [
        &b""[..],
        b"a",
        b"ab",
        b"abc",
        b"abcd",
        &[0, 255, 16, 128, 7],
    ] {
        assert_eq!(base64_bytes(&base64(bytes)).as_deref(), Some(bytes));
    }
    assert_eq!(base64_bytes("abc"), None);
    assert_eq!(base64_bytes("a==="), None);
    assert_eq!(base64_bytes("ab-_"), None);
}

#[test]
fn only_binary_types_are_binary() {
    for name in ["bytea", "BLOB", "longblob", "varbinary(16)", "binary(4)"] {
        assert!(is_binary_type(name), "{name}");
    }
    for name in ["text", "integer", "character varying(10)", "json"] {
        assert!(!is_binary_type(name), "{name}");
    }
}

/// Writes a file in the folder of a test.
fn write_file(name: &str, bytes: &[u8]) -> PathBuf {
    let path = temp_dir(name).join("data.csv");
    std::fs::write(&path, bytes).unwrap();
    path
}

fn job(
    path: &Path,
    columns: &[&str],
    target_columns: &[(&str, bool)],
    mode: ImportMode,
    batch: usize,
) -> ImportJob {
    let columns = names(columns);
    let target = Target {
        table: "\"main\".\"t\"".to_owned(),
        columns: target_columns
            .iter()
            .map(|(name, binary)| TargetColumn {
                name: (*name).to_owned(),
                binary: *binary,
                source: columns.iter().position(|column| column == name).unwrap(),
            })
            .collect(),
        dialect: Dialect::Sqlite,
        base64: false,
    };
    ImportJob {
        path: path.to_owned(),
        read: ReadOptions::default(),
        columns,
        target,
        mode,
        batch,
        error_limit: 1000,
        log: log_path(path, "2026-11-02-1200"),
        progress: Arc::new(ImportProgress::default()),
    }
}

fn plain_job(path: &Path, mode: ImportMode, batch: usize) -> ImportJob {
    job(
        path,
        &["a", "b"],
        &[("a", false), ("b", false)],
        mode,
        batch,
    )
}

#[test]
fn the_path_of_the_log_stays_beside_the_source() {
    assert_eq!(
        log_path(Path::new("/tmp/data.csv"), "2026-11-02-1200"),
        Path::new("/tmp/data-errors-2026-11-02-1200.log")
    );
}

#[test]
fn a_batch_ends_at_its_rows_and_the_file_ends_the_last_batch() {
    let path = write_file("feed-rows", b"a,b\n1,2\n3,4\n5,6\n");
    let mut feed = Feed::open(plain_job(&path, ImportMode::ByBatch, 2)).unwrap();
    let lot = feed.next_lot();
    assert_eq!(lot.rows.len(), 2);
    assert_eq!(lot.end, LotEnd::More);
    assert_eq!(
        feed.statement(&lot.rows),
        "INSERT INTO \"main\".\"t\" (\"a\", \"b\") VALUES ('1', '2'), ('3', '4')"
    );
    let lot = feed.next_lot();
    assert_eq!(lot.rows.len(), 1);
    assert_eq!(lot.end, LotEnd::Done);
}

#[test]
fn a_batch_ends_at_its_size_and_a_large_row_goes_alone() {
    let large = "x".repeat(MAX_STATEMENT_BYTES);
    let half = "y".repeat(MAX_STATEMENT_BYTES / 2 + 100);
    let file = format!("a,b\n1,2\n{large},2\n3,4\n{half},1\n{half},2\n");
    let path = write_file("feed-size", file.as_bytes());
    let mut feed = Feed::open(plain_job(&path, ImportMode::ByBatch, 100)).unwrap();
    let sizes: Vec<usize> = std::iter::from_fn(|| {
        let lot = feed.next_lot();
        (!lot.rows.is_empty()).then_some(lot.rows.len())
    })
    .collect();
    assert_eq!(sizes, [1, 1, 2, 1]);
}

#[test]
fn a_clean_read_of_a_batch_moves_the_percent() {
    let path = write_file("feed-percent", b"a,b\n1,2\n3,4\n");
    let plain = plain_job(&path, ImportMode::ByBatch, 1);
    let progress = plain.progress.clone();
    let mut feed = Feed::open(plain).unwrap();
    feed.next_lot();
    assert!((1..100).contains(&progress.percent()));
    feed.next_lot();
    feed.next_lot();
    assert_eq!(progress.percent(), 100);
}

#[test]
fn a_bad_row_ends_the_batch_unless_the_mode_logs_it() {
    let file = b"a,b\n1,2\n3\n5,6\n";
    let path = write_file("feed-bad", file);
    let mut feed = Feed::open(plain_job(&path, ImportMode::ByBatch, 10)).unwrap();
    let lot = feed.next_lot();
    assert_eq!(lot.rows.len(), 1);
    let LotEnd::Stop(reason) = lot.end else {
        panic!("{:?}", lot.end);
    };
    assert_eq!(
        reason,
        "Line 3: This row has 1 fields. The file has 2 columns."
    );

    let logged = plain_job(&path, ImportMode::LogErrors, 10);
    let (log, progress) = (logged.log.clone(), logged.progress.clone());
    let mut feed = Feed::open(logged).unwrap();
    let lot = feed.next_lot();
    assert_eq!(lot.rows.len(), 2);
    assert_eq!(lot.end, LotEnd::Done);
    feed.close().unwrap();
    assert_eq!(progress.errors(), 1);
    assert_eq!(
        std::fs::read_to_string(log).unwrap(),
        "Line 3: This row has 1 fields. The file has 2 columns.\n3\n\n"
    );
}

#[test]
fn a_clean_file_leaves_no_log() {
    let path = write_file("feed-clean", b"a,b\n1,2\n");
    let logged = plain_job(&path, ImportMode::LogErrors, 10);
    let log = logged.log.clone();
    let mut feed = Feed::open(logged).unwrap();
    feed.next_lot();
    feed.close().unwrap();
    assert!(!log.exists());
}

#[test]
fn the_preview_reads_the_start_of_a_file() {
    let mut file = String::from("a;b\n");
    for row in 0..PREVIEW_ROWS + 10 {
        file.push_str(&format!("{row};x\n"));
    }
    let path = write_file("probe", file.as_bytes());
    let found = probe(&path, ReadOptions::default(), true);
    assert_eq!(found.options.delimiter, Delimiter::Semicolon);
    assert_eq!(found.columns, ["a", "b"]);
    assert_eq!(found.rows.len(), PREVIEW_ROWS);
    assert_eq!(found.failure, None);
}

#[test]
fn the_preview_reports_a_missing_file_and_a_broken_file() {
    let missing = temp_dir("probe-missing").join("none.csv");
    assert!(
        probe(&missing, ReadOptions::default(), true)
            .failure
            .is_some()
    );
    let path = write_file("probe-broken", b"a,b\n1,2\n3,\"open\n");
    let found = probe(&path, ReadOptions::default(), false);
    assert_eq!(found.rows.len(), 1);
    assert_eq!(
        found.failure.as_deref(),
        Some("The quotes that start at line 3 have no end.")
    );
}

#[test]
fn the_preview_stops_at_its_limit_of_bytes_without_a_fault() {
    // A value above the limit: the read ends inside the quotes.
    let mut file = b"a,b\n1,2\n3,\"".to_vec();
    file.resize(20 * 1024 * 1024, b'x');
    let path = write_file("probe-limit", &file);
    let found = probe(&path, ReadOptions::default(), false);
    assert_eq!(found.rows.len(), 1);
    assert_eq!(found.failure, None);
}

struct Harness {
    manager: Manager,
    events: Receiver<ExecEvent>,
    console: ConsoleId,
}

struct Finished {
    end: ExecutionEnd,
    message: Option<String>,
    rows: Vec<Row>,
}

impl Harness {
    fn at(path: &Path, settings: Settings) -> Self {
        let mut target = SqliteTarget::new(path);
        target.allow_create = true;
        let budget = Arc::new(Budget::new(settings.budget_bytes));
        let connector = Arc::new(SqliteConnector::new(target));
        let (mut manager, events) = Manager::new(budget).unwrap();
        let connection = manager.add_connection(connector, settings);
        let console = manager.open_console(connection).unwrap();
        Self {
            manager,
            events,
            console,
        }
    }

    fn new(name: &str, setup: &[&str]) -> Self {
        let mut harness = Self::at(&temp_db(name), Settings::default());
        let statements = setup.iter().map(|sql| (*sql).to_owned()).collect();
        let execution = harness
            .manager
            .run(harness.console, statements, RunOptions::default());
        let finished = harness.wait(execution);
        assert_eq!(
            finished.end,
            ExecutionEnd::Completed,
            "{:?}",
            finished.message
        );
        harness
    }

    fn wait(&mut self, execution: u64) -> Finished {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut rows = Vec::new();
        loop {
            assert!(Instant::now() < deadline, "no event arrived");
            let Ok(event) = self.events.try_recv() else {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            };
            match event {
                ExecEvent::Rows { rows: batch, .. } => rows.extend(batch),
                ExecEvent::ExecutionFinished {
                    execution: id,
                    end,
                    message,
                    ..
                } if id == execution => {
                    return Finished { end, message, rows };
                }
                _ => {}
            }
        }
    }

    fn import(&mut self, job: ImportJob) -> Finished {
        let execution = self.manager.import(self.console, job);
        self.wait(execution)
    }

    fn query(&mut self, sql: &str) -> Vec<Vec<String>> {
        let statements = vec![sql.to_owned()];
        let execution = self
            .manager
            .run(self.console, statements, RunOptions::default());
        let finished = self.wait(execution);
        assert_eq!(
            finished.end,
            ExecutionEnd::Completed,
            "{:?}",
            finished.message
        );
        finished
            .rows
            .iter()
            .map(|row| row.cells().map(Value::display).collect())
            .collect()
    }

    fn count(&mut self) -> usize {
        self.query("SELECT a FROM t").len()
    }
}

const TABLE: &[&str] = &["CREATE TABLE t (a INTEGER PRIMARY KEY, b TEXT NOT NULL)"];

#[test]
fn all_or_nothing_writes_the_whole_file() {
    let mut h = Harness::new("import-all", TABLE);
    let path = write_file("import-all", b"a,b\n1,x\n2,y\n3,z\n");
    let job = plain_job(&path, ImportMode::AllOrNothing, 2);
    let progress = job.progress.clone();
    let finished = h.import(job);
    assert_eq!(
        finished.end,
        ExecutionEnd::Completed,
        "{:?}",
        finished.message
    );
    assert!(finished.rows.is_empty());
    assert_eq!(progress.rows(), 3);
    assert_eq!(h.query("SELECT a, b FROM t ORDER BY a")[2], ["3", "z"]);
}

#[test]
fn all_or_nothing_undoes_all_after_a_failure_in_the_database() {
    let mut h = Harness::new("import-undo", TABLE);
    let path = write_file("import-undo", b"a,b\n1,x\n2,y\n3,z\n1,again\n");
    let job = plain_job(&path, ImportMode::AllOrNothing, 2);
    let progress = job.progress.clone();
    let finished = h.import(job);
    assert_eq!(finished.end, ExecutionEnd::Failed);
    let message = finished.message.unwrap();
    assert!(
        message.starts_with("The batch of lines 4 to 5 failed: "),
        "{message}"
    );
    assert_eq!(progress.rows(), 0);
    assert_eq!(h.count(), 0);
}

#[test]
fn all_or_nothing_undoes_all_after_a_bad_row_of_the_file() {
    let mut h = Harness::new("import-undo-bad", TABLE);
    let path = write_file("import-undo-bad", b"a,b\n1,x\n2,y\n3,z\nwrong\n");
    let finished = h.import(plain_job(&path, ImportMode::AllOrNothing, 2));
    assert_eq!(finished.end, ExecutionEnd::Failed);
    assert_eq!(
        finished.message.as_deref(),
        Some("Line 5: This row has 1 fields. The file has 2 columns.")
    );
    assert_eq!(h.count(), 0);
}

#[test]
fn a_session_stays_usable_after_each_end() {
    let mut h = Harness::new("import-after", TABLE);
    let path = write_file("import-after", b"a,b\n1,x\n1,y\n");
    assert_eq!(
        h.import(plain_job(&path, ImportMode::AllOrNothing, 1)).end,
        ExecutionEnd::Failed
    );
    let path = write_file("import-after-2", b"a,b\n5,x\n");
    assert_eq!(
        h.import(plain_job(&path, ImportMode::AllOrNothing, 1)).end,
        ExecutionEnd::Completed
    );
    assert_eq!(h.count(), 1);
}

#[test]
fn by_batch_keeps_the_saved_batches_and_stops_at_the_failure() {
    let mut h = Harness::new("import-batch", TABLE);
    let path = write_file("import-batch", b"a,b\n1,x\n2,y\n3,z\n1,again\n9,last\n");
    let job = plain_job(&path, ImportMode::ByBatch, 2);
    let progress = job.progress.clone();
    let finished = h.import(job);
    assert_eq!(finished.end, ExecutionEnd::Failed);
    assert!(finished.message.unwrap().contains("lines 4 to 5"));
    assert_eq!(progress.rows(), 2);
    assert_eq!(h.count(), 2);
}

#[test]
fn by_batch_sends_the_rows_before_a_bad_row() {
    let mut h = Harness::new("import-batch-bad", TABLE);
    let path = write_file("import-batch-bad", b"a,b\n1,x\nwrong\n3,z\n");
    let job = plain_job(&path, ImportMode::ByBatch, 10);
    let progress = job.progress.clone();
    let finished = h.import(job);
    assert_eq!(finished.end, ExecutionEnd::Failed);
    assert!(finished.message.unwrap().starts_with("Line 3: "));
    assert_eq!(progress.rows(), 1);
    assert_eq!(h.count(), 1);
}

#[test]
fn log_errors_saves_each_good_row_and_logs_each_bad_row() {
    let mut h = Harness::new("import-log", TABLE);
    let file = b"a,b\n1,x\n1,again\n2,y\nwrong\n3,z\n4,\n";
    let path = write_file("import-log", file);
    let job = plain_job(&path, ImportMode::LogErrors, 3);
    let (log, progress) = (job.log.clone(), job.progress.clone());
    let finished = h.import(job);
    assert_eq!(
        finished.end,
        ExecutionEnd::Completed,
        "{:?}",
        finished.message
    );
    assert_eq!(progress.rows(), 3);
    assert_eq!(progress.errors(), 3);
    assert_eq!(h.count(), 3);
    let text = std::fs::read_to_string(log).unwrap();
    assert!(text.contains("Line 3: UNIQUE constraint failed"), "{text}");
    assert!(text.contains("1,again\n"), "{text}");
    assert!(text.contains("Line 5: This row has 1 fields"), "{text}");
    assert!(
        text.contains("Line 7: NOT NULL constraint failed"),
        "{text}"
    );
    assert!(text.contains("4,\n"), "{text}");
}

#[test]
fn log_errors_stops_at_the_limit() {
    let mut h = Harness::new("import-limit", TABLE);
    let path = write_file("import-limit", b"a,b\n1,x\n1,a\n1,b\n1,c\n1,d\n2,y\n");
    let job = ImportJob {
        error_limit: 2,
        ..plain_job(&path, ImportMode::LogErrors, 10)
    };
    let progress = job.progress.clone();
    let finished = h.import(job);
    assert_eq!(finished.end, ExecutionEnd::Failed);
    assert_eq!(
        finished.message.as_deref(),
        Some("The import stopped after 2 errors.")
    );
    assert_eq!(progress.errors(), 2);
    assert_eq!(progress.rows(), 1);
}

#[test]
fn the_limit_also_counts_bad_rows_of_the_file() {
    let mut h = Harness::new("import-limit-file", TABLE);
    let path = write_file("import-limit-file", b"a,b\nx\ny\nz\n1,ok\n");
    let job = ImportJob {
        error_limit: 2,
        ..plain_job(&path, ImportMode::LogErrors, 10)
    };
    let progress = job.progress.clone();
    let finished = h.import(job);
    assert_eq!(finished.end, ExecutionEnd::Failed);
    assert_eq!(progress.errors(), 2);
    assert_eq!(h.count(), 0);
}

#[test]
fn a_file_fault_ends_each_mode() {
    for mode in ImportMode::ALL {
        let name = format!("import-fault-{mode:?}");
        let mut h = Harness::new(&name, TABLE);
        let path = write_file(&name, b"a,b\n1,x\n2,\"open\n");
        let finished = h.import(plain_job(&path, mode, 10));
        assert_eq!(finished.end, ExecutionEnd::Failed, "{mode:?}");
        assert_eq!(
            finished.message.as_deref(),
            Some("The quotes that start at line 3 have no end.")
        );
        let expected = usize::from(mode != ImportMode::AllOrNothing);
        assert_eq!(h.count(), expected, "{mode:?}");
    }
}

#[test]
fn a_missing_file_fails_before_the_first_row() {
    let mut h = Harness::new("import-missing", TABLE);
    let path = temp_dir("import-missing").join("none.csv");
    let finished = h.import(plain_job(&path, ImportMode::AllOrNothing, 10));
    assert_eq!(finished.end, ExecutionEnd::Failed);
    assert!(finished.message.is_some());
    assert_eq!(h.count(), 0);
}

#[test]
fn a_column_without_a_mapping_keeps_its_default() {
    let mut h = Harness::new(
        "import-default",
        &["CREATE TABLE t (a INTEGER, b TEXT DEFAULT 'fixed', c TEXT)"],
    );
    let path = write_file("import-default", b"c,skip,a\nC,S,1\n");
    let job = job(
        &path,
        &["c", "skip", "a"],
        &[("a", false), ("c", false)],
        ImportMode::AllOrNothing,
        10,
    );
    assert_eq!(h.import(job).end, ExecutionEnd::Completed);
    assert_eq!(h.query("SELECT a, b, c FROM t"), [["1", "fixed", "C"]]);
}

#[test]
fn hostile_text_stays_text() {
    let mut h = Harness::new("import-hostile", TABLE);
    let file = b"a,b\n1,\"x'); DROP TABLE t; --\"\n2,\"\\'\"\n";
    let path = write_file("import-hostile", file);
    let finished = h.import(plain_job(&path, ImportMode::AllOrNothing, 10));
    assert_eq!(
        finished.end,
        ExecutionEnd::Completed,
        "{:?}",
        finished.message
    );
    assert_eq!(
        h.query("SELECT b FROM t ORDER BY a"),
        [["x'); DROP TABLE t; --"], ["\\'"]]
    );
}

#[test]
fn a_read_only_connection_refuses_the_rows() {
    let settings = Settings {
        read_only: true,
        ..Settings::default()
    };
    // The table must exist first, so a second harness opens the same database.
    let creator = Harness::new("import-read-only", TABLE);
    drop(creator);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("import-read-only.db");
    let mut h = Harness::at(&path, settings);
    let file = write_file("import-read-only", b"a,b\n1,x\n");
    let finished = h.import(plain_job(&file, ImportMode::AllOrNothing, 10));
    assert_eq!(finished.end, ExecutionEnd::Failed);
    assert_eq!(h.count(), 0);
}

#[test]
fn stop_undoes_all_or_nothing() {
    let mut h = Harness::new("import-stop", TABLE);
    let mut file = String::from("a,b\n");
    for row in 0..200_000 {
        file.push_str(&format!("{row},x\n"));
    }
    let path = write_file("import-stop", file.as_bytes());
    let job = plain_job(&path, ImportMode::AllOrNothing, 10);
    let progress = job.progress.clone();
    let execution = h.manager.import(h.console, job);
    let deadline = Instant::now() + Duration::from_secs(30);
    while progress.rows() < 100 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    h.manager.stop(h.console);
    let finished = h.wait(execution);
    assert_eq!(
        finished.end,
        ExecutionEnd::Stopped,
        "{:?}",
        finished.message
    );
    assert_eq!(progress.rows(), 0);
    assert_eq!(h.count(), 0);
}

#[test]
fn stop_keeps_the_saved_batches_of_by_batch() {
    let mut h = Harness::new("import-stop-batch", TABLE);
    let mut file = String::from("a,b\n");
    for row in 0..200_000 {
        file.push_str(&format!("{row},x\n"));
    }
    let path = write_file("import-stop-batch", file.as_bytes());
    let job = plain_job(&path, ImportMode::ByBatch, 10);
    let progress = job.progress.clone();
    let execution = h.manager.import(h.console, job);
    let deadline = Instant::now() + Duration::from_secs(30);
    while progress.rows() < 100 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    h.manager.stop(h.console);
    let finished = h.wait(execution);
    assert_eq!(
        finished.end,
        ExecutionEnd::Stopped,
        "{:?}",
        finished.message
    );
    let saved = progress.rows();
    assert!((100..200_000).contains(&saved), "{saved}");
    assert_eq!(h.count(), saved);
}

/// Exports `t`, imports the file into `copy`, and compares the two tables.
fn round_trip(name: &str, export: ExportFormat, import: ImportFormat, null: NullText) {
    let mut h = Harness::new(
        name,
        &[
            "CREATE TABLE t (a INTEGER, b TEXT, c BLOB, d REAL)",
            "CREATE TABLE copy (a INTEGER, b TEXT, c BLOB, d REAL)",
            "INSERT INTO t VALUES (1, 'plain', x'00ff10', 1.5)",
            "INSERT INTO t VALUES (2, '', NULL, NULL)",
            "INSERT INTO t VALUES (3, NULL, x'', -2.25)",
            "INSERT INTO t VALUES (4, 'quote \" and , and ; and ''', x'e9', 0)",
            "INSERT INTO t VALUES (5, 'line\nbreak\ttab\r\nend', NULL, 1e100)",
            "INSERT INTO t VALUES (6, 'ação € 日本語 NULL', NULL, NULL)",
            "INSERT INTO t VALUES (7, '\\', NULL, NULL)",
            "INSERT INTO t VALUES (8, 'NULL', NULL, NULL)",
        ],
    );
    let path = temp_dir(name).join(format!("data.{}", export.extension()));
    let options = ExportOptions {
        format: export,
        null: null.text(),
        ..ExportOptions::default()
    };
    let writer = ExportWriter::create(&path, options).unwrap();
    let run = RunOptions {
        export: Some(writer.clone()),
        ..RunOptions::default()
    };
    let select = vec!["SELECT a, b, c, d FROM t ORDER BY a".to_owned()];
    let execution = h.manager.run(h.console, select, run);
    assert_eq!(h.wait(execution).end, ExecutionEnd::Completed);
    writer.promote(&path).unwrap();

    let read = ReadOptions {
        format: import,
        null_text: null,
        ..ReadOptions::default()
    };
    let found = probe(&path, read, false);
    assert_eq!(found.failure, None);
    assert_eq!(found.columns, ["a", "b", "c", "d"]);
    let target = Target {
        table: "\"main\".\"copy\"".to_owned(),
        columns: found
            .columns
            .iter()
            .enumerate()
            .map(|(source, name)| TargetColumn {
                name: name.clone(),
                binary: name == "c",
                source,
            })
            .collect(),
        dialect: Dialect::Sqlite,
        base64: !import.is_delimited(),
    };
    let job = ImportJob {
        path: path.clone(),
        read,
        columns: found.columns,
        target,
        mode: ImportMode::AllOrNothing,
        batch: 3,
        error_limit: 1000,
        log: log_path(&path, "x"),
        progress: Arc::new(ImportProgress::default()),
    };
    let finished = h.import(job);
    assert_eq!(
        finished.end,
        ExecutionEnd::Completed,
        "{:?}",
        finished.message
    );
    let compare = "SELECT a, b, hex(c), d, typeof(b), typeof(c), typeof(d) FROM";
    assert_eq!(
        h.query(&format!("{compare} t ORDER BY a")),
        h.query(&format!("{compare} copy ORDER BY a")),
        "{name}"
    );
}

#[test]
fn a_file_of_each_export_format_returns_the_same_table() {
    for null in [NullText::Empty, NullText::Word] {
        let tag = format!("{null:?}");
        round_trip(
            &format!("trip-csv-{tag}"),
            ExportFormat::Csv,
            ImportFormat::Csv,
            null,
        );
        round_trip(
            &format!("trip-tsv-{tag}"),
            ExportFormat::Tsv,
            ImportFormat::Tsv,
            null,
        );
    }
    round_trip(
        "trip-json",
        ExportFormat::Json,
        ImportFormat::Json,
        NullText::Empty,
    );
    round_trip(
        "trip-jsonl",
        ExportFormat::JsonLines,
        ImportFormat::JsonLines,
        NullText::Empty,
    );
}

#[test]
fn a_blank_line_of_a_table_of_one_column_returns_null() {
    let mut h = Harness::new(
        "trip-one",
        &[
            "CREATE TABLE t (a TEXT)",
            "CREATE TABLE copy (a TEXT)",
            "INSERT INTO t VALUES ('x'), (NULL), (''), (NULL)",
        ],
    );
    let path = temp_dir("trip-one").join("data.csv");
    let writer = ExportWriter::create(&path, ExportOptions::default()).unwrap();
    let run = RunOptions {
        export: Some(writer.clone()),
        ..RunOptions::default()
    };
    let select = vec!["SELECT a FROM t ORDER BY rowid".to_owned()];
    let execution = h.manager.run(h.console, select, run);
    assert_eq!(h.wait(execution).end, ExecutionEnd::Completed);
    writer.promote(&path).unwrap();

    let mut plain = job(&path, &["a"], &[("a", false)], ImportMode::AllOrNothing, 10);
    plain.target.table = "\"main\".\"copy\"".to_owned();
    assert_eq!(h.import(plain).end, ExecutionEnd::Completed);
    assert_eq!(
        h.query("SELECT a, typeof(a) FROM copy ORDER BY rowid"),
        h.query("SELECT a, typeof(a) FROM t ORDER BY rowid")
    );
}
