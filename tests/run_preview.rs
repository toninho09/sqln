use crate::common::{Fixture, ctrl, key, shift};
use crossterm::event::KeyCode;
use edtui::Index2;
use sqln::editor::{KeyboardMode, RunPreview, RunRefusal, RunScope};
use sqln::sql::{Dialect, statement_at};

#[test]
fn the_preview_uses_one_based_lines_and_keeps_editor_state() {
    let mut f = Fixture::conventional("-- note\nSELECT 'ação',\n  2;\nSELECT 3;");
    f.editor.set_cursor(Index2::new(2, 2));
    let cursor = f.editor.cursor();
    let revision = f.editor.revision();
    for _ in 0..3 {
        assert_eq!(
            f.editor.run_preview(),
            Ok(RunPreview {
                first_line: 2,
                last_line: 3
            })
        );
    }
    let plan = f.editor.plan_run(RunScope::Default).unwrap();
    assert_eq!(plan.statements, ["SELECT 'ação',\n  2"]);
    assert_eq!(plan.rows, [1]);
    assert_eq!(f.editor.cursor(), cursor);
    assert_eq!(f.editor.revision(), revision);
    assert!(f.editor.selection_range().is_none());
}

#[test]
fn forward_and_backward_selections_preview_the_same_executable_lines() {
    for backward in [false, true] {
        let mut f = Fixture::conventional("SELECT 1;\nSELECT 2;\nSELECT 3;");
        if backward {
            f.editor.set_cursor(Index2::new(2, 0));
            f.press_all(&[shift(KeyCode::Up), shift(KeyCode::Up)]);
        } else {
            f.press_all(&[shift(KeyCode::Down), shift(KeyCode::Down)]);
        }
        let selection = f.editor.selection_range();
        assert_eq!(
            f.editor.run_preview(),
            Ok(RunPreview {
                first_line: 1,
                last_line: 2
            })
        );
        assert_eq!(f.editor.selection_range(), selection);
        assert_eq!(
            f.editor.plan_run(RunScope::Default).unwrap().statements,
            ["SELECT 1", "SELECT 2"]
        );
        assert_eq!(
            f.editor
                .plan_run(RunScope::Script)
                .unwrap()
                .statements
                .len(),
            3
        );
    }
}

#[test]
fn a_partial_selection_uses_only_selected_text_in_both_modes() {
    for mode in [KeyboardMode::Conventional, KeyboardMode::Vim] {
        let mut f = Fixture::new("SELECT 123;", mode);
        f.editor.set_cursor(Index2::new(0, 7));
        if mode == KeyboardMode::Conventional {
            f.press_all(&[shift(KeyCode::Right), shift(KeyCode::Right)]);
        } else {
            f.press_all(&[key(KeyCode::Char('v')), key(KeyCode::Right)]);
        }
        let selected = f.selected().unwrap();
        assert_eq!(selected, "12");
        assert_eq!(
            f.editor.run_preview(),
            Ok(RunPreview {
                first_line: 1,
                last_line: 1
            })
        );
        assert_eq!(
            f.editor.plan_run(RunScope::Default).unwrap().statements,
            [selected]
        );
        f.press(if mode == KeyboardMode::Conventional {
            shift(KeyCode::Right)
        } else {
            key(KeyCode::Right)
        });
        assert!(f.editor.run_preview().is_ok());
        assert_eq!(
            f.editor.plan_run(RunScope::Default).unwrap().statements,
            ["123"]
        );
    }
}

#[test]
fn comments_and_blank_text_do_not_preview_another_statement() {
    for text in ["", "-- comment", "SELECT 1;\n\n-- comment\nSELECT 2;"] {
        let mut f = Fixture::conventional(text);
        if text.starts_with("SELECT") {
            f.editor.set_cursor(Index2::new(1, 0));
        }
        assert_eq!(f.editor.run_preview(), Err(RunRefusal::CursorOutside));
        assert_eq!(
            f.editor.plan_run(RunScope::Default),
            Err(RunRefusal::CursorOutside)
        );
    }
    let mut f = Fixture::conventional("-- comment\nSELECT 1;");
    f.press(shift(KeyCode::Down));
    assert_eq!(f.editor.run_preview(), Err(RunRefusal::NoStatement));
    assert_eq!(
        f.editor.plan_run(RunScope::Default),
        Err(RunRefusal::NoStatement)
    );
}

#[test]
fn edits_undo_cursor_moves_and_dialect_changes_update_the_preview() {
    let mut f = Fixture::conventional("SELECT 1;\nSELECT 2;");
    assert_eq!(f.editor.run_preview().unwrap().first_line, 1);
    f.editor.set_cursor(Index2::new(1, 0));
    assert_eq!(f.editor.run_preview().unwrap().first_line, 2);
    f.editor.on_paste("\n");
    assert_eq!(f.editor.run_preview().unwrap().first_line, 3);
    f.press(ctrl('z'));
    assert_eq!(f.editor.run_preview().unwrap().first_line, 2);

    let mut f = Fixture::conventional("PING\nGET key");
    assert_eq!(f.editor.run_preview().unwrap().last_line, 2);
    f.editor.set_dialect(Dialect::Redis);
    assert_eq!(f.editor.run_preview().unwrap().last_line, 1);
}

#[test]
fn cached_statement_resolution_keeps_the_existing_dialect_boundaries() {
    for (dialect, text) in [
        (
            Dialect::Postgres,
            "SELECT 'ação;';\n-- gap\nSELECT $$a;b$$;",
        ),
        (
            Dialect::MySql,
            "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; SELECT 2; END//\nDELIMITER ;\nSELECT 3;",
        ),
        (
            Dialect::Sqlite,
            "CREATE TRIGGER t AFTER INSERT ON x BEGIN SELECT 1; SELECT 2; END;\nSELECT 3;",
        ),
        (Dialect::Redis, "# note\nPING\nGET key"),
        (
            Dialect::Kafka,
            "TOPICS\nSELECT *\nFROM orders LIMIT 1;\nGROUPS",
        ),
    ] {
        let mut f = Fixture::with_dialect(text, KeyboardMode::Conventional, dialect);
        for (row, line) in text.split('\n').enumerate() {
            for col in 0..=line.chars().count() {
                let offset = text
                    .split('\n')
                    .take(row)
                    .map(|line| line.len() + 1)
                    .sum::<usize>()
                    + line
                        .char_indices()
                        .nth(col)
                        .map_or(line.len(), |(offset, _)| offset);
                f.editor.set_cursor(Index2::new(row, col));
                let expected = statement_at(text, dialect, offset);
                let preview = f.editor.run_preview();
                let plan = f.editor.plan_run(RunScope::Default);
                match expected {
                    Some(statement) => {
                        let preview = preview.unwrap();
                        assert_eq!(plan.unwrap().statements, [statement.text(text)]);
                        assert_eq!(
                            preview.first_line,
                            text[..statement.start]
                                .bytes()
                                .filter(|byte| *byte == b'\n')
                                .count()
                                + 1
                        );
                        assert_eq!(
                            preview.last_line,
                            text[..statement.end - 1]
                                .bytes()
                                .filter(|byte| *byte == b'\n')
                                .count()
                                + 1
                        );
                    }
                    None => {
                        assert_eq!(preview, Err(RunRefusal::CursorOutside));
                        assert_eq!(plan, Err(RunRefusal::CursorOutside));
                    }
                }
            }
        }
    }
}
