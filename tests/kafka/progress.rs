// The counts of a Kafka read on the summary line. These tests need no server: a fake
// connector writes the counts, and the test moves the clock.

use super::*;
use sqln::connector::StatementEnd;
use sqln::exec::{ProgressSnapshot, ReadEnd, ReadMeter, ReadProgress, percent};
use sqln::input::Keymap;
use sqln::ui::text;
use tokio::sync::Notify;

fn snapshot() -> ProgressSnapshot {
    ProgressSnapshot {
        filtered: true,
        scanned: 12_345,
        matched: 200,
        bytes: 4_404_019,
        percent: Some(45),
        lag: None,
        end: ReadEnd::None,
    }
}

#[test]
fn the_progress_texts_put_the_important_counts_first() {
    assert_eq!(
        text::kafka_reading(&snapshot(), Some(1_234)),
        "Reading 45% · 12,345 messages scanned · 200 matched · 1,234 msg/s · 4.2 MiB"
    );
    let plain = ProgressSnapshot {
        filtered: false,
        percent: None,
        lag: Some(3_400),
        ..snapshot()
    };
    assert_eq!(
        text::kafka_progress(&plain, None),
        "12,345 messages scanned · lag 3,400 · 4.2 MiB"
    );
    let ends = [
        (ReadEnd::None, ""),
        (ReadEnd::End, " The read reached the end of each partition."),
        (ReadEnd::Limit, " The read stopped at LIMIT."),
        (ReadEnd::Scan, " The read stopped at the SCAN limit."),
        (ReadEnd::Stopped, " The read stopped before the end."),
    ];
    for (end, reason) in ends {
        let progress = ProgressSnapshot { end, ..snapshot() };
        assert_eq!(
            text::kafka_read_end(&progress),
            format!("12,345 messages scanned, 4.2 MiB.{reason}")
        );
    }
}

#[test]
fn a_size_uses_the_largest_unit_with_one_decimal() {
    assert_eq!(text::byte_size(0), "0 B");
    assert_eq!(text::byte_size(1023), "1023 B");
    assert_eq!(text::byte_size(1024), "1.0 KiB");
    assert_eq!(text::byte_size(1536), "1.5 KiB");
    assert_eq!(text::byte_size(1024 * 1024 - 1), "1.0 MiB");
    assert_eq!(text::byte_size(5 * 1024 * 1024 * 1024), "5.0 GiB");
    assert_eq!(text::byte_size(u64::MAX), "16777216.0 TiB");
}

#[test]
fn a_percent_needs_a_whole_and_stays_at_100_or_less() {
    assert_eq!(percent(1, 0), None);
    assert_eq!(percent(0, 7), Some(0));
    assert_eq!(percent(45, 100), Some(45));
    assert_eq!(percent(9, 4), Some(100));
    assert_eq!(percent(u64::MAX, u64::MAX), Some(100));
}

#[test]
fn the_counts_keep_the_last_values_of_the_connector() {
    let progress = ReadProgress::new(false);
    assert_eq!(progress.snapshot().percent, None);
    assert_eq!(progress.snapshot().lag, None);
    progress.add_scanned(10);
    progress.add_scanned(5);
    progress.add_matched();
    progress.set_percent(Some(250));
    progress.set_lag(Some(9));
    progress.set_end(ReadEnd::Scan);
    let snapshot = progress.snapshot();
    assert_eq!(
        snapshot,
        ProgressSnapshot {
            filtered: false,
            scanned: 2,
            matched: 1,
            bytes: 15,
            percent: Some(100),
            lag: Some(9),
            end: ReadEnd::Scan,
        }
    );
    progress.set_lag(None);
    assert_eq!(progress.snapshot().lag, None);
}

#[test]
fn the_speed_needs_one_second_between_two_measures() {
    let progress = Arc::new(ReadProgress::new(false));
    let mut meter = ReadMeter::new(progress.clone());
    let start = Instant::now();
    meter.sample(start);
    for _ in 0..300 {
        progress.add_scanned(1);
    }
    meter.sample(start + Duration::from_millis(500));
    assert_eq!(meter.rate(), None);
    meter.sample(start + Duration::from_secs(2));
    assert_eq!(meter.rate(), Some(150));
    meter.sample(start + Duration::from_secs(3));
    assert_eq!(meter.rate(), Some(0));
}

/// A console whose connector tracks `progress`, sends one row, and waits for `release`
/// or for a stop. F6 runs and F7 stops.
fn progress_screen(progress: Arc<ReadProgress>, release: Arc<Notify>) -> Screen {
    let handler: crate::common::mock::RunHandler = Arc::new(move |_, mut sink| {
        let progress = progress.clone();
        let release = release.clone();
        Box::pin(async move {
            sink.track(progress);
            sink.set_columns(vec!["key".to_owned()]).await;
            sink.push([Value::Text("first-event")].into_iter().collect())
                .await;
            sink.flush().await;
            let stop = sink.stop_token();
            tokio::select! {
                () = release.notified() => {}
                () = stop.cancelled() => {}
            }
            let (rows, limit) = sink.finish().await;
            Ok(StatementEnd {
                rows,
                limit,
                has_columns: true,
                ..StatementEnd::default()
            })
        })
    });
    let connector = Arc::new(crate::common::mock::MockConnector::new(
        Dialect::Kafka,
        handler,
    ));
    let overrides = [
        ("run".to_owned(), vec!["f6".to_owned()]),
        ("stop".to_owned(), vec!["f7".to_owned()]),
    ]
    .into_iter()
    .collect();
    let mode = KeyboardMode::Conventional;
    let options = Options {
        connector: Some(connector),
        keyboard: mode,
        keymap: Some(Keymap::with_overrides(mode, &overrides).unwrap()),
        ..Options::default()
    };
    Screen {
        app: App::new(
            options,
            Rc::new(RefCell::new(MemoryClipboard::default())),
            Theme::plain(),
        ),
    }
}

fn run(screen: &mut Screen, sql: &str) {
    use crossterm::event::KeyCode;
    screen.app.on_event(Event::Paste(sql.to_owned()));
    screen
        .app
        .on_event(Event::Key(crate::common::key(KeyCode::F(6))));
}

fn wait_idle(screen: &mut Screen) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while screen.app.console().is_busy() {
        screen.app.drain_events();
        assert!(Instant::now() < deadline, "the read does not end");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_finite_read_shows_its_counts_and_the_reason_for_its_end() {
    for filtered in [true, false] {
        let progress = Arc::new(ReadProgress::new(filtered));
        let release = Arc::new(Notify::new());
        let mut screen = progress_screen(progress.clone(), release.clone());
        run(&mut screen, "CONSUME orders WHERE key = 'first'");
        screen.wait_for("Reading · 0 messages scanned");
        for _ in 0..3 {
            progress.add_scanned(1024);
        }
        progress.add_matched();
        progress.set_percent(Some(45));
        let start = Instant::now();
        screen.app.tick_at(start);
        for _ in 0..500 {
            progress.add_scanned(1024);
        }
        screen.app.tick_at(start + Duration::from_secs(1));
        for (width, height) in [(80, 24), (120, 35)] {
            let text = screen.screen_at(width, height);
            assert!(
                text.contains("Reading 45% · 503 messages scanned"),
                "{text}"
            );
            assert_eq!(text.contains("· 1 matched"), filtered, "{text}");
            assert!(text.contains("first-event"), "{text}");
        }
        // A narrow screen cuts the end of the line, so the last counts need a wide one.
        let wide = screen.screen_at(120, 35);
        assert!(wide.contains("500 msg/s · 503.0 KiB"), "{wide}");
        progress.set_end(ReadEnd::Limit);
        release.notify_one();
        wait_idle(&mut screen);
        let text = screen.screen_at(120, 35);
        assert!(
            text.contains(
                "1 row read. 503 messages scanned, 503.0 KiB. The read stopped at LIMIT."
            ),
            "{text}"
        );
        assert!(!text.contains("Reading"), "{text}");
    }
}

#[test]
fn a_live_read_shows_its_lag_and_gives_no_reason_at_the_stop() {
    use crossterm::event::KeyCode;
    let progress = Arc::new(ReadProgress::new(false));
    let release = Arc::new(Notify::new());
    let mut screen = progress_screen(progress.clone(), release);
    run(&mut screen, "TAIL orders");
    screen.wait_for("1 row received. Live. · 0 messages scanned");
    let start = Instant::now();
    screen.app.tick_at(start);
    progress.add_scanned(2048);
    progress.set_lag(Some(3_400));
    screen.app.tick_at(start + Duration::from_secs(1));
    for (width, height) in [(80, 24), (120, 35)] {
        let text = screen.screen_at(width, height);
        assert!(text.contains("Live"), "{text}");
        assert!(text.contains("lag 3,400"), "{text}");
    }
    let wide = screen.screen_at(120, 35);
    assert!(wide.contains("1 msg/s · 2.0 KiB"), "{wide}");
    screen
        .app
        .on_event(Event::Key(crate::common::key(KeyCode::F(7))));
    wait_idle(&mut screen);
    let text = screen.screen_at(120, 35);
    assert!(
        text.contains("1 row received. 1 message scanned, 2.0 KiB."),
        "{text}"
    );
    assert!(!text.contains("The read"), "{text}");
    assert!(!text.contains("Live"), "{text}");
}

// The tests below need a server.

fn counts(run: &Run) -> ProgressSnapshot {
    run.progress
        .as_ref()
        .expect("the read sends its counts")
        .snapshot()
}

fn wait_until(progress: &ReadProgress, done: impl Fn(&ProgressSnapshot) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let snapshot = progress.snapshot();
        if done(&snapshot) {
            return;
        }
        assert!(Instant::now() < deadline, "{snapshot:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn consume_counts_its_messages_and_names_its_end() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("progress");
    create_topic(&uri, &topic, 2);
    produce(
        &uri,
        &topic,
        &[
            Out::new(0, "a", b"one"),
            Out::new(0, "b", b"two"),
            Out::new(1, "c", b"three"),
            Out::new(1, "d", b"four"),
        ],
    );
    let mut h = Harness::open(&uri);
    let full = counts(&h.run_ok(&format!("CONSUME {topic}")));
    assert_eq!(
        full,
        ProgressSnapshot {
            filtered: false,
            scanned: 4,
            matched: 4,
            // Four keys of one byte, and the values.
            bytes: 4 + 3 + 3 + 5 + 4,
            percent: Some(100),
            lag: None,
            end: ReadEnd::End,
        }
    );
    let limited = counts(&h.run_ok(&format!("CONSUME {topic} WHERE NOT key = 'a' LIMIT 2")));
    assert!(limited.filtered);
    assert_eq!((limited.matched, limited.end), (2, ReadEnd::Limit));
    assert!(limited.scanned >= 2, "{limited:?}");
    let scanned = counts(&h.run_ok(&format!("CONSUME {topic} SCAN 3")));
    assert_eq!((scanned.scanned, scanned.end), (3, ReadEnd::Scan));
    let settings = Settings {
        row_cap: 2,
        ..Settings::default()
    };
    let mut capped = Harness::with_settings(network_connector(parse_uri(&uri).unwrap()), settings);
    let run = capped.run_ok(&format!("CONSUME {topic}"));
    assert_eq!(run.rows.len(), 2);
    // The grid stopped the read, and its summary gives the reason.
    assert_eq!(counts(&run).end, ReadEnd::None);
}

#[test]
fn tail_keeps_a_known_lag_after_it_reads_the_high_offsets_again() {
    let Some(uri) = kafka_uri() else { return };
    let topic = unique("tail-lag");
    create_topic(&uri, &topic, 1);
    produce(
        &uri,
        &topic,
        &[
            Out::new(0, "a", b"1"),
            Out::new(0, "b", b"2"),
            Out::new(0, "c", b"3"),
        ],
    );
    let mut h = Harness::open(&uri);
    let execution = h.start_live(&format!("TAIL {topic} FROM earliest"));
    let progress = loop {
        match h.next() {
            ExecEvent::Columns { progress, .. } => break progress.expect("counts"),
            ExecEvent::ExecutionFinished { end, message, .. } => {
                panic!("the run ended: {end:?} {message:?}")
            }
            _ => {}
        }
    };
    wait_until(&progress, |counts| {
        counts.scanned == 3 && counts.lag == Some(0)
    });
    // A check of the high offsets runs each 2 seconds. A failed check removes the lag.
    std::thread::sleep(Duration::from_millis(2_500));
    produce(&uri, &topic, &[Out::new(0, "d", b"4")]);
    wait_until(&progress, |counts| {
        counts.scanned == 4 && counts.lag == Some(0)
    });
    assert_eq!(progress.snapshot().percent, None);
    h.manager.stop(h.console);
    let run = h.collect(execution);
    assert_eq!(run.end, Some(ExecutionEnd::Stopped), "{run:?}");
    assert_eq!(progress.snapshot().end, ReadEnd::Stopped);
}
