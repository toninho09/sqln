//! The terminal session and the main loop.

use std::cell::RefCell;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::Instant;

use crossterm::event::{Event, read};

use super::{App, Options};
use crate::clipboard::{SharedClipboard, SystemClipboard};
use crate::terminal::Session;
use crate::terminal::{ring_bell, watch_terminal};
use crate::ui::theme::Theme;

pub fn run(options: Options) -> io::Result<()> {
    let clipboard: SharedClipboard = Rc::new(RefCell::new(SystemClipboard::new()));
    let report_folder = options.store.as_ref().map(|store| store.global.dir.clone());
    let (sender, receiver) = sync_channel(LOOP_CAPACITY);
    // The start of the terminal can wait for an answer, and a signal can arrive meanwhile.
    let signal = signal_flag(sender.clone())?;
    // The watchdog starts before the session, because a query of the terminal can
    // also spin when the terminal closes during the start.
    watch_terminal();
    let mut session = Session::start(report_folder)?;
    let mut app = App::new(options, clipboard, Theme::from_env());
    let waker = sender.clone();
    app.wake().install(move || {
        // A full channel already holds a message, so the loop wakes anyway.
        let _ = waker.try_send(Message::Wake);
    });
    // The terminal queries of the start are over, so one thread can own the input now.
    read_input(sender)?;
    let result = catch_unwind(AssertUnwindSafe(|| {
        event_loop(&mut app, &mut session, &signal, &receiver)
    }));
    match result {
        Ok(Ok(())) => Ok(()),
        // A closed terminal ends the loop with an error, and the text still needs a place.
        Ok(Err(error)) => {
            app.save_without_questions();
            app.shutdown();
            Err(error)
        }
        // The panic hook restored the terminal and wrote the report. The text comes next.
        Err(panic) => {
            let _ = catch_unwind(AssertUnwindSafe(|| app.save_without_questions()));
            resume_unwind(panic);
        }
    }
}

/// The messages that the main loop holds before it reads them. A full channel stops the
/// input thread until the loop reads.
const LOOP_CAPACITY: usize = 64;

/// What wakes the main loop besides its own timers.
enum Message {
    Input(io::Result<Event>),
    /// Background work delivered a result.
    Wake,
    /// A signal that ends the process. The flag holds it; the message only wakes the loop.
    Signal,
}

/// Sets a flag for each signal that ends a process, and wakes the loop.
fn signal_flag(sender: SyncSender<Message>) -> io::Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
        let terms = [SIGHUP, SIGINT, SIGQUIT, SIGTERM];
        for signal in terms {
            signal_hook::flag::register(signal, flag.clone())?;
        }
        let mut signals = signal_hook::iterator::Signals::new(terms)?;
        std::thread::Builder::new()
            .name("sqln-signals".into())
            .spawn(move || {
                for _ in signals.forever() {
                    let _ = sender.try_send(Message::Signal);
                }
            })?;
    }
    #[cfg(not(unix))]
    drop(sender);
    Ok(flag)
}

/// Reads the terminal on its own thread, so a wake can end the wait of the loop.
fn read_input(sender: SyncSender<Message>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("sqln-input".into())
        .spawn(move || {
            loop {
                let event = read();
                let failed = event.is_err();
                // A closed terminal ends the reads, and the loop reports the error.
                if sender.send(Message::Input(event)).is_err() || failed {
                    return;
                }
            }
        })?;
    Ok(())
}

fn event_loop(
    app: &mut App,
    session: &mut Session,
    signal: &AtomicBool,
    messages: &Receiver<Message>,
) -> io::Result<()> {
    let mut dirty = true;
    loop {
        // A signal comes first, because a closed terminal cannot draw.
        if signal.load(Ordering::Relaxed) {
            crate::log::event("signal");
            app.save_without_questions();
            app.shutdown();
            return Ok(());
        }
        if dirty {
            session.screen.draw(|frame| app.draw(frame))?;
            dirty = false;
        }
        if app.should_quit() {
            crate::log::event("exit");
            app.shutdown();
            return Ok(());
        }
        let now = Instant::now();
        let wait = app.next_wakeup(now).saturating_duration_since(now);
        let mut message = match messages.recv_timeout(wait) {
            Ok(message) => Some(message),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("the terminal input stopped"));
            }
        };
        // The keys that wait come before the next frame.
        while let Some(next) = message {
            match next {
                Message::Input(event) => {
                    app.on_event(event?);
                    dirty = true;
                }
                Message::Wake | Message::Signal => {}
            }
            message = messages.try_recv().ok();
        }
        dirty |= app.drain_events();
        if app.take_bell() {
            ring_bell();
        }
    }
}
