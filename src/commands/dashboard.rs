//! The `leme dashboard` command: runs the dashboard in the current terminal.

use std::env;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use clap::Args;
use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use super::Error;
use crate::dashboard::app::{App, Effect, Input};
use crate::dashboard::{self, CollectError, Snapshot, view};

#[derive(Debug, Args)]
pub struct DashboardArgs {}

/// How long to wait for a key before checking for a finished collection.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

type Collected = Result<Snapshot, CollectError>;

pub fn run(_args: &DashboardArgs) -> Result<(), Error> {
    let directory = env::current_dir().map_err(Error::CurrentDirectory)?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::NotATerminal);
    }
    let mut session = Session::start().map_err(Error::Terminal)?;
    let (sender, receiver) = mpsc::channel();
    let (mut app, effect) = App::new();
    perform(effect, &directory, &sender);
    while !app.should_quit() {
        session
            .terminal
            .draw(|frame| view::render(&app, frame))
            .map_err(Error::Terminal)?;
        let Some(input) = next_input(&receiver).map_err(Error::Terminal)? else {
            continue;
        };
        if let Some(effect) = app.handle(input) {
            perform(effect, &directory, &sender);
        }
    }
    Ok(())
}

/// Carries out an effect requested by the app.
fn perform(effect: Effect, directory: &Path, sender: &Sender<Collected>) {
    match effect {
        Effect::Collect => collect_in_background(directory.to_path_buf(), sender.clone()),
    }
}

/// Collects a snapshot on another thread so keys stay responsive meanwhile.
fn collect_in_background(directory: PathBuf, sender: Sender<Collected>) {
    thread::spawn(move || {
        // The receiver is gone only when the dashboard has already closed.
        let _ = sender.send(dashboard::collect(&directory));
    });
}

/// Waits for the next input for the app.
///
/// Returns `None` when the screen only needs to be redrawn, as after a resize.
fn next_input(receiver: &Receiver<Collected>) -> io::Result<Option<Input>> {
    loop {
        if let Ok(result) = receiver.try_recv() {
            return Ok(Some(Input::Collected(result)));
        }
        if !event::poll(POLL_INTERVAL)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => return Ok(Some(Input::Key(key))),
            Event::Resize(..) => return Ok(None),
            _ => {}
        }
    }
}

/// The terminal in raw mode on the alternate screen.
///
/// Dropping it restores the normal screen and input mode, whether the
/// dashboard quits, fails to draw or returns early with an error. Ratatui's
/// panic hook restores the terminal on a panic as well.
struct Session {
    terminal: DefaultTerminal,
}

impl Session {
    fn start() -> io::Result<Self> {
        match ratatui::try_init() {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                // Setup may have enabled raw mode before failing.
                ratatui::restore();
                Err(error)
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        ratatui::restore();
    }
}
