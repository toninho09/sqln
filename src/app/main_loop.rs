//! The terminal session and the main loop.

use std::cell::RefCell;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossterm::event::{poll, read};

use super::{App, Options};
use crate::clipboard::{SharedClipboard, SystemClipboard};
use crate::terminal::Session;
use crate::terminal::{ring_bell, watch_terminal};
use crate::ui::theme::Theme;

pub fn run(options: Options) -> io::Result<()> {
    let clipboard: SharedClipboard = Rc::new(RefCell::new(SystemClipboard::new()));
    let report_folder = options.store.as_ref().map(|store| store.global.dir.clone());
    // The start of the terminal can wait for an answer, and a signal can arrive meanwhile.
    let signal = signal_flag()?;
    // The watchdog starts before the session, because a query of the terminal can
    // also spin when the terminal closes during the start.
    watch_terminal();
    let mut session = Session::start(report_folder)?;
    let mut app = App::new(options, clipboard, Theme::from_env());
    let result = catch_unwind(AssertUnwindSafe(|| {
        event_loop(&mut app, &mut session, &signal)
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

/// Sets a flag for each signal that ends a process.
fn signal_flag() -> io::Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
        for signal in [SIGHUP, SIGINT, SIGQUIT, SIGTERM] {
            signal_hook::flag::register(signal, flag.clone())?;
        }
    }
    Ok(flag)
}

fn event_loop(app: &mut App, session: &mut Session, signal: &AtomicBool) -> io::Result<()> {
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
        if poll(app.key_wait())? {
            app.on_event(read()?);
            while poll(Duration::ZERO)? {
                app.on_event(read()?);
            }
            dirty = true;
        }
        dirty |= app.drain_events();
        if app.take_bell() {
            ring_bell();
        }
    }
}
