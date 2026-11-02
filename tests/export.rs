// The bytes of each export format, the file of an export, and the export dialog.

use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sqln::export::form::{AllRows, ExportForm, ExportRequest, ExportSource, Field, RowChoice};
use sqln::export::{
    Encoder, ExportChoices, ExportFormat, ExportOptions, ExportWriter, file_name, resolve_path,
};
use sqln::grid::Value;
use sqln::sql::{Dialect, quote_literal};

fn text(value: &str) -> Value {
    Value::Text(value.into())
}

fn number(value: &str) -> Value {
    Value::Number(value.into())
}

fn binary(bytes: &[u8]) -> Value {
    Value::Binary(bytes.into())
}

fn encode(options: ExportOptions, columns: &[&str], rows: &[Vec<Value>]) -> String {
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, options);
    let columns: Vec<String> = columns.iter().map(|name| (*name).to_owned()).collect();
    encoder.start(&columns).unwrap();
    for row in rows {
        encoder.row(row).unwrap();
    }
    encoder.finish().unwrap();
    String::from_utf8(out).unwrap()
}

fn with_format(format: ExportFormat) -> ExportOptions {
    ExportOptions {
        format,
        ..ExportOptions::default()
    }
}

fn insert(dialect: Dialect, batch: usize) -> ExportOptions {
    ExportOptions {
        format: ExportFormat::SqlInsert,
        batch,
        table: "t".to_owned(),
        dialect,
        ..ExportOptions::default()
    }
}

#[test]
fn csv_quotes_what_a_reader_needs_and_keeps_the_rest() {
    let rows = vec![vec![
        Value::Null,
        text(""),
        text("a,b"),
        text("say \"hi\""),
        text("two\nlines"),
        text("tab\there"),
        number("1.50"),
    ]];
    assert_eq!(
        encode(
            with_format(ExportFormat::Csv),
            &["a", "b", "c", "d", "e", "f", "g"],
            &rows
        ),
        "a,b,c,d,e,f,g\n,\"\",\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\",tab\there,1.50\n"
    );
}

#[test]
fn tsv_cells_match_the_copy_of_the_grid() {
    let rows = vec![vec![Value::Null, text(""), text("tab\there"), text("a,b")]];
    let mut table = sqln::grid::ResultTable::new(vec!["a".to_owned(); 4]);
    table.append(vec![rows[0].clone().into_boxed_slice()], 0);
    let options = ExportOptions {
        header: false,
        ..with_format(ExportFormat::Tsv)
    };
    assert_eq!(
        encode(options, &["a", "b", "c", "d"], &rows),
        table.tsv(0, 0, 0, 3)
    );
}

#[test]
fn the_options_of_csv_change_the_file() {
    let rows = vec![vec![Value::Null, text("NULL"), text("a;b"), text("a,b")]];
    let options = ExportOptions {
        delimiter: ';',
        null: "NULL",
        bom: true,
        header: false,
        ..with_format(ExportFormat::Csv)
    };
    assert_eq!(
        encode(options, &["a", "b", "c", "d"], &rows),
        "\u{feff}NULL;\"NULL\";\"a;b\";a,b\n"
    );
}

#[test]
fn a_column_name_is_quoted_as_a_value() {
    assert_eq!(
        encode(with_format(ExportFormat::Csv), &["a,b", "c"], &[]),
        "\"a,b\",c\n"
    );
}

#[test]
fn binary_follows_the_grid_in_delimited_text() {
    let rows = vec![vec![binary(b"plain"), binary(&[0, 255])]];
    assert_eq!(
        encode(with_format(ExportFormat::Csv), &["a", "b"], &rows),
        "a,b\nplain,0x00FF\n"
    );
}

#[test]
fn json_writes_one_object_per_line() {
    let rows = vec![
        vec![number("1"), text("x")],
        vec![Value::Null, text("a\"b\nc")],
    ];
    assert_eq!(
        encode(with_format(ExportFormat::Json), &["id", "name"], &rows),
        "[\n{\"id\":1,\"name\":\"x\"},\n{\"id\":null,\"name\":\"a\\\"b\\nc\"}\n]\n"
    );
}

#[test]
fn json_of_no_rows_is_an_empty_array() {
    assert_eq!(
        encode(with_format(ExportFormat::Json), &["id"], &[]),
        "[]\n"
    );
    assert_eq!(
        encode(with_format(ExportFormat::JsonLines), &["id"], &[]),
        ""
    );
}

#[test]
fn json_lines_writes_one_object_for_each_row() {
    let rows = vec![vec![number("1")], vec![number("2")]];
    assert_eq!(
        encode(with_format(ExportFormat::JsonLines), &["id"], &rows),
        "{\"id\":1}\n{\"id\":2}\n"
    );
}

#[test]
fn json_keeps_the_digits_and_quotes_what_is_not_a_number() {
    let long = "123456789012345678901234567890.123456789";
    let rows = vec![vec![
        number("1234.50"),
        number(long),
        number("NaN"),
        number("Infinity"),
        number("inf"),
    ]];
    assert_eq!(
        encode(
            with_format(ExportFormat::JsonLines),
            &["a", "b", "c", "d", "e"],
            &rows
        ),
        format!("{{\"a\":1234.50,\"b\":{long},\"c\":\"NaN\",\"d\":\"Infinity\",\"e\":\"inf\"}}\n")
    );
}

#[test]
fn json_writes_binary_as_base64() {
    let rows = vec![vec![binary(b"plain"), binary(&[0, 255])]];
    assert_eq!(
        encode(with_format(ExportFormat::JsonLines), &["a", "b"], &rows),
        "{\"a\":\"cGxhaW4=\",\"b\":\"AP8=\"}\n"
    );
}

#[test]
fn json_gives_each_repeated_name_a_number() {
    let rows = vec![vec![number("1"), number("2"), number("3"), number("4")]];
    assert_eq!(
        encode(
            with_format(ExportFormat::JsonLines),
            &["id", "id", "id_2", "id"],
            &rows
        ),
        "{\"id\":1,\"id_2\":2,\"id_2_2\":3,\"id_3\":4}\n"
    );
}

#[test]
fn insert_writes_one_statement_per_row() {
    let rows = vec![
        vec![number("1"), text("it's")],
        vec![Value::Null, number("NaN")],
    ];
    assert_eq!(
        encode(insert(Dialect::Postgres, 1), &["id", "name"], &rows),
        "INSERT INTO t (\"id\", \"name\") VALUES (1, 'it''s');\n\
         INSERT INTO t (\"id\", \"name\") VALUES (NULL, 'NaN');\n"
    );
}

#[test]
fn a_group_closes_at_its_size_and_at_the_end() {
    let rows: Vec<Vec<Value>> = ["1", "2", "3"].iter().map(|v| vec![number(v)]).collect();
    assert_eq!(
        encode(insert(Dialect::Sqlite, 2), &["id"], &rows),
        "INSERT INTO t (\"id\") VALUES\n  (1),\n  (2);\n\
         INSERT INTO t (\"id\") VALUES\n  (3);\n"
    );
    assert_eq!(
        encode(insert(Dialect::Sqlite, 3), &["id"], &rows),
        "INSERT INTO t (\"id\") VALUES\n  (1),\n  (2),\n  (3);\n"
    );
}

#[test]
fn each_dialect_writes_its_literals() {
    let row = vec![vec![text("a\\b"), binary(&[0xDE, 0xAD])]];
    assert_eq!(
        encode(insert(Dialect::Postgres, 1), &["a", "b"], &row),
        "INSERT INTO t (\"a\", \"b\") VALUES ('a\\b', '\\xDEAD');\n"
    );
    assert_eq!(
        encode(insert(Dialect::MySql, 1), &["a", "b"], &row),
        "INSERT INTO t (`a`, `b`) VALUES ('a\\\\b', X'DEAD');\n"
    );
    assert_eq!(
        encode(insert(Dialect::Sqlite, 1), &["a", "b"], &row),
        "INSERT INTO t (\"a\", \"b\") VALUES ('a\\b', X'DEAD');\n"
    );
    assert_eq!(quote_literal("o'k", Dialect::MySql), "'o''k'");
}

#[test]
fn a_second_result_adds_no_second_header() {
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, with_format(ExportFormat::Csv));
    encoder.start(&["a".to_owned()]).unwrap();
    encoder.start(&["b".to_owned()]).unwrap();
    encoder.row(&[number("1")]).unwrap();
    encoder.finish().unwrap();
    assert_eq!(String::from_utf8(out).unwrap(), "a\n1\n");
}

fn folder(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&path);
    path
}

/// Counts the temporary files of an export in a folder. Every temporary name
/// ends in `.tmp`, and no final file of an export does.
fn temporaries(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "tmp"))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn two_exports_of_one_destination_use_two_temporary_files() {
    let base = folder("export-unique");
    let destination = base.join("out.csv");
    let first = ExportWriter::create(&destination, with_format(ExportFormat::Csv)).unwrap();
    let second = ExportWriter::create(&destination, with_format(ExportFormat::Csv)).unwrap();
    assert_ne!(
        format!("{first:?}"),
        format!("{second:?}"),
        "each export owns its temporary file"
    );
    first.discard();
    second.discard();
}

#[test]
fn a_cleanup_removes_the_temporary_of_a_stopped_process() {
    use std::time::Duration;

    use sqln::store::file;

    let base = folder("export-orphans");
    fs::create_dir_all(&base).unwrap();
    let destination = base.join("out.csv");
    let orphan = file::temporary_unique(&destination);
    fs::write(&orphan, "rows").unwrap();
    let foreign = base.join("notes.tmp");
    fs::write(&foreign, "not an export").unwrap();
    let live = file::temporary_unique(&destination);
    let held = fs::File::options()
        .write(true)
        .create_new(true)
        .open(&live)
        .unwrap();
    held.try_lock().unwrap();
    file::remove_orphan_temporaries(&base, Duration::ZERO);
    assert!(!orphan.exists(), "the file of a stopped process goes away");
    assert!(foreign.exists(), "a foreign file with the same end stays");
    assert!(live.exists(), "the file of a live writer stays");
    drop(held);
    file::remove_orphan_temporaries(&base, Duration::ZERO);
    assert!(!live.exists(), "the file without a writer goes away");
}

#[test]
fn a_file_appears_only_after_the_promotion() {
    let base = folder("export-writer");
    let destination = base.join("new/folder/out.csv");
    let writer = ExportWriter::create(&destination, with_format(ExportFormat::Csv)).unwrap();
    writer.start(&["a".to_owned()]);
    assert!(writer.write_row(&[number("1")]));
    assert_eq!(writer.rows(), 1);
    assert!(!destination.exists());
    assert_eq!(temporaries(&base.join("new/folder")), 1);
    writer.promote(&destination).unwrap();
    assert_eq!(fs::read_to_string(&destination).unwrap(), "a\n1\n");
    assert_eq!(temporaries(&base.join("new/folder")), 0);
}

#[test]
fn a_discard_leaves_no_file_and_ends_the_writes() {
    let base = folder("export-discard");
    let destination = base.join("out.csv");
    let writer = ExportWriter::create(&destination, with_format(ExportFormat::Csv)).unwrap();
    writer.discard();
    assert!(!writer.write_row(&[number("1")]));
    assert_eq!(writer.rows(), 0);
    assert!(!destination.exists());
    assert_eq!(temporaries(&base), 0);
}

#[test]
fn a_seal_closes_the_format_once() {
    let base = folder("export-seal");
    let destination = base.join("out.json");
    let writer = ExportWriter::create(&destination, with_format(ExportFormat::Json)).unwrap();
    writer.start(&["a".to_owned()]);
    assert!(writer.write_row(&[number("1")]));
    writer.seal();
    writer.seal();
    assert!(!writer.write_row(&[number("2")]));
    writer.promote(&destination).unwrap();
    assert_eq!(
        fs::read_to_string(&destination).unwrap(),
        "[\n{\"a\":1}\n]\n"
    );
}

#[test]
fn a_path_starts_at_the_home_or_at_the_current_folder() {
    let home = Some(PathBuf::from("/home/user"));
    assert_eq!(
        resolve_path("~/out.csv", home.clone()).unwrap(),
        Path::new("/home/user/out.csv")
    );
    assert_eq!(
        resolve_path("  /tmp/out.csv ", home.clone()).unwrap(),
        Path::new("/tmp/out.csv")
    );
    // Another user's folder needs the system, so the text stays as a name.
    assert_eq!(
        resolve_path("~other/out.csv", home.clone()).unwrap(),
        std::env::current_dir().unwrap().join("~other/out.csv")
    );
    assert_eq!(
        resolve_path("out.csv", home).unwrap(),
        std::env::current_dir().unwrap().join("out.csv")
    );
}

#[test]
fn a_file_name_holds_no_separator() {
    assert_eq!(
        file_name("a/b\\c:d\ne", "2026-10-31-1530", ExportFormat::JsonLines),
        "a-b-c-d-e-2026-10-31-1530.jsonl"
    );
}

fn source(dialect: Dialect, all_rows: AllRows, selection: Option<(usize, usize)>) -> ExportSource {
    ExportSource {
        name: "users".to_owned(),
        table: "\"public\".\"users\"".to_owned(),
        dialect,
        loaded: 10,
        selection,
        all_rows,
    }
}

const STAMP: &str = "2026-10-31-1530";

fn form(all_rows: AllRows, selection: Option<(usize, usize)>) -> ExportForm {
    let source = source(Dialect::Postgres, all_rows, selection);
    ExportForm::new(source, &ExportChoices::default(), STAMP)
}

fn press(form: &mut ExportForm, code: KeyCode) -> ExportRequest {
    form.on_key(KeyEvent::new(code, KeyModifiers::NONE), false)
}

#[test]
fn the_first_rows_are_the_selection_then_all_rows_then_the_grid() {
    let cases = [
        (AllRows::Available, Some((2, 3)), RowChoice::Selection),
        (AllRows::Available, None, RowChoice::AllRows),
        (AllRows::Complete, None, RowChoice::LoadedRows),
        (AllRows::Refused("no"), None, RowChoice::LoadedRows),
    ];
    for (all_rows, selection, rows) in cases {
        assert_eq!(form(all_rows, selection).rows, rows);
    }
}

#[test]
fn the_rows_field_offers_only_what_exists() {
    let mut f = form(AllRows::Available, Some((2, 3)));
    press(&mut f, KeyCode::Tab);
    assert_eq!(f.focus, Field::Rows);
    press(&mut f, KeyCode::Right);
    assert_eq!(f.rows, RowChoice::AllRows);
    press(&mut f, KeyCode::Right);
    assert_eq!(f.rows, RowChoice::LoadedRows);
    press(&mut f, KeyCode::Right);
    assert_eq!(f.rows, RowChoice::Selection);

    let mut f = form(AllRows::Complete, None);
    press(&mut f, KeyCode::Tab);
    press(&mut f, KeyCode::Right);
    assert_eq!(f.rows, RowChoice::LoadedRows);
}

#[test]
fn the_extension_follows_the_format_until_the_path_changes() {
    let mut f = form(AllRows::Complete, None);
    assert_eq!(f.path_text(), format!("./users-{STAMP}.csv"));
    press(&mut f, KeyCode::Down);
    assert_eq!(f.format, ExportFormat::Tsv);
    assert_eq!(f.path_text(), format!("./users-{STAMP}.tsv"));

    press(&mut f, KeyCode::Tab);
    press(&mut f, KeyCode::Tab);
    assert_eq!(f.focus, Field::File);
    press(&mut f, KeyCode::Backspace);
    press(&mut f, KeyCode::Char('x'));
    assert_eq!(f.path_text(), format!("./users-{STAMP}.tsx"));
    f.focus_on(Field::Formats);
    press(&mut f, KeyCode::Down);
    assert_eq!(f.format, ExportFormat::Json);
    assert_eq!(f.path_text(), format!("./users-{STAMP}.tsx"));
}

#[test]
fn the_extension_follows_the_format_after_a_new_folder() {
    let mut f = form(AllRows::Complete, None);
    f.set_path(&format!("/data/users-{STAMP}.csv"), false);
    press(&mut f, KeyCode::Down);
    assert_eq!(f.path_text(), format!("/data/users-{STAMP}.tsv"));
}

#[test]
fn a_chosen_file_keeps_its_extension() {
    let mut f = form(AllRows::Complete, None);
    f.set_path("/data/old.txt", true);
    press(&mut f, KeyCode::Down);
    assert_eq!(f.format, ExportFormat::Tsv);
    assert_eq!(f.path_text(), "/data/old.txt");
}

#[test]
fn only_the_path_asks_for_the_file_browser() {
    let mut f = form(AllRows::Complete, None);
    let browse = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
    assert!(matches!(f.on_key(browse, false), ExportRequest::None));
    f.focus_on(Field::File);
    assert!(matches!(f.on_key(browse, false), ExportRequest::Browse));
}

#[test]
fn the_saved_folder_starts_the_path() {
    let choices = ExportChoices {
        folder: Some(PathBuf::from("/data/out")),
        ..ExportChoices::default()
    };
    let source = source(Dialect::Postgres, AllRows::Complete, None);
    let f = ExportForm::new(source, &choices, STAMP);
    assert_eq!(f.path_text(), format!("/data/out/users-{STAMP}.csv"));
}

#[test]
fn each_format_shows_its_fields() {
    let mut f = form(AllRows::Complete, None);
    let base = [Field::Rows, Field::File];
    let fields = |extra: &[Field]| [&base, extra].concat();
    assert_eq!(
        f.fields(),
        fields(&[Field::Header, Field::Delimiter, Field::NullText, Field::Bom])
    );
    press(&mut f, KeyCode::Down);
    assert_eq!(
        f.fields(),
        fields(&[Field::Header, Field::NullText, Field::Bom])
    );
    press(&mut f, KeyCode::Down);
    assert_eq!(f.fields(), fields(&[]));
    press(&mut f, KeyCode::Down);
    assert_eq!(f.fields(), fields(&[]));
    press(&mut f, KeyCode::Down);
    assert_eq!(f.fields(), fields(&[Field::Table, Field::Grouping]));
    f.grouped = true;
    assert_eq!(
        f.fields(),
        fields(&[Field::Table, Field::Grouping, Field::Batch])
    );
}

#[test]
fn redis_has_no_insert() {
    let source = source(Dialect::Redis, AllRows::Complete, None);
    let choices = ExportChoices {
        format: ExportFormat::SqlInsert,
        ..ExportChoices::default()
    };
    let f = ExportForm::new(source, &choices, STAMP);
    assert!(!f.formats().contains(&ExportFormat::SqlInsert));
    assert_eq!(f.format, ExportFormat::Csv);
}

#[test]
fn an_insert_needs_a_table_and_a_group_needs_a_size() {
    let mut empty = source(Dialect::Sqlite, AllRows::Complete, None);
    empty.table = String::new();
    let choices = ExportChoices {
        format: ExportFormat::SqlInsert,
        ..ExportChoices::default()
    };
    let mut f = ExportForm::new(empty, &choices, STAMP);
    assert!(f.options().unwrap_err().contains("table"));
    f.focus_on(Field::Table);
    f.paste("users\nignored");
    assert_eq!(f.options().unwrap().table, "users");
    assert_eq!(f.options().unwrap().batch, 1, "one per row is the default");

    f.grouped = true;
    assert_eq!(f.options().unwrap().batch, 100);
    f.focus_on(Field::Batch);
    for _ in 0..3 {
        press(&mut f, KeyCode::Backspace);
    }
    assert!(f.options().unwrap_err().contains("needs a number"));
    press(&mut f, KeyCode::Char('x'));
    press(&mut f, KeyCode::Char('2'));
    assert_eq!(f.options().unwrap().batch, 2);
}

#[test]
fn enter_moves_to_the_next_field_and_presses_the_buttons() {
    let mut f = form(AllRows::Complete, None);
    assert!(matches!(press(&mut f, KeyCode::Enter), ExportRequest::None));
    assert_eq!(f.focus, Field::Rows);
    f.focus_on(Field::File);
    assert!(matches!(press(&mut f, KeyCode::Enter), ExportRequest::None));
    assert_eq!(f.focus, Field::Header);
    f.focus_on(Field::Export);
    assert!(matches!(
        press(&mut f, KeyCode::Enter),
        ExportRequest::Export
    ));
    f.focus_on(Field::Cancel);
    assert!(matches!(
        press(&mut f, KeyCode::Enter),
        ExportRequest::Cancel
    ));
    assert!(matches!(press(&mut f, KeyCode::Esc), ExportRequest::Cancel));
}

#[test]
fn vim_keys_stay_text_in_a_field_of_text() {
    let mut f = form(AllRows::Complete, None);
    f.focus_on(Field::Header);
    let vim = |code| KeyEvent::new(code, KeyModifiers::NONE);
    f.on_key(vim(KeyCode::Char('l')), true);
    assert!(!f.header);
    f.focus_on(Field::File);
    let before = f.path_text().len();
    f.on_key(vim(KeyCode::Char('j')), true);
    assert_eq!(f.path_text().len(), before + 1);
    assert_eq!(f.focus, Field::File);
}
