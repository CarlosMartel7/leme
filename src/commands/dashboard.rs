//! The `leme dashboard` command: runs the dashboard in the current terminal.

use std::env;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use clap::Args;
use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use super::Error;
use crate::dashboard::app::{App, Effect, Input};
use crate::dashboard::{self, view};
use crate::state::{Store, TimerOwnership, TimerWrite, TimerWriter};
use crate::time::Moment;
use crate::time::local::{self, Reading};

#[derive(Debug, Args)]
pub struct DashboardArgs {}

/// How long to wait for a key before checking for other inputs.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// How often the app receives the time. Short enough that the countdown and
/// the header change within a fraction of a second of each new value.
const TICK_INTERVAL: Duration = Duration::from_millis(250);

pub fn run(_args: &DashboardArgs) -> Result<(), Error> {
    let directory = env::current_dir().map_err(Error::CurrentDirectory)?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(Error::NotATerminal);
    }
    let mut session = Session::start().map_err(Error::Terminal)?;
    let (sender, receiver) = mpsc::channel();
    // Collections claim the project's timer lock through this; the writer
    // keeps a clone, so the lock outlives every pending timer write.
    let ownership = TimerOwnership::new();
    // Declared after the session so it is dropped first: pending timer
    // writes finish before the terminal is restored and the process exits.
    let writer = TimerWriter::spawn(ownership.clone(), Store::open_default, {
        let sender = sender.clone();
        // The receiver is gone only when the dashboard has closed.
        move |result| drop(sender.send(Input::TimerWritten(result)))
    });
    let context = Context {
        directory,
        sender,
        ownership,
        writer,
    };
    let (mut app, effect) = App::new();
    context.perform(effect);
    let mut ticker = Ticker::default();
    while !app.should_quit() {
        session
            .terminal
            .draw(|frame| view::render(&app, frame))
            .map_err(Error::Terminal)?;
        for input in next_inputs(&receiver, &mut ticker).map_err(Error::Terminal)? {
            if let Some(effect) = app.handle(input) {
                context.perform(effect);
            }
        }
    }
    Ok(())
}

/// What the loop needs to carry out the app's effects.
struct Context {
    directory: PathBuf,
    sender: Sender<Input>,
    ownership: TimerOwnership,
    writer: TimerWriter,
}

impl Context {
    /// Carries out an effect requested by the app.
    fn perform(&self, effect: Effect) {
        let write = match effect {
            Effect::Collect => {
                self.collect_in_background();
                return;
            }
            Effect::SaveTimer { project_id, timer } => TimerWrite::Save { project_id, timer },
            Effect::ClearTimer { project_id } => TimerWrite::Clear { project_id },
        };
        self.writer.send(write);
    }

    /// Collects a snapshot on another thread so keys stay responsive
    /// meanwhile.
    fn collect_in_background(&self) {
        let (directory, sender) = (self.directory.clone(), self.sender.clone());
        let ownership = self.ownership.clone();
        thread::spawn(move || {
            // The receiver is gone only when the dashboard has already closed.
            let _ = sender.send(Input::Collected(dashboard::collect(&directory, &ownership)));
        });
    }
}

/// Waits for the next inputs for the app, in the order to apply them.
///
/// Returns nothing when the screen only needs to be redrawn, as after a
/// resize. A key always comes with the current time just before it, so timer
/// keys act at the moment they are pressed.
fn next_inputs(receiver: &Receiver<Input>, ticker: &mut Ticker) -> io::Result<Vec<Input>> {
    loop {
        if let Some(inputs) = ticker.due() {
            return Ok(inputs);
        }
        if let Ok(input) = receiver.try_recv() {
            return Ok(vec![input]);
        }
        if !event::poll(POLL_INTERVAL)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) => return Ok(vec![Input::Tick(Moment::now()), Input::Key(key)]),
            Event::Resize(..) => return Ok(Vec::new()),
            _ => {}
        }
    }
}

/// Delivers the time to the app on a regular schedule, independently of
/// collections.
#[derive(Default)]
struct Ticker {
    last_reading: Option<Reading>,
    ticked_at: Option<Instant>,
}

impl Ticker {
    /// Every [`TICK_INTERVAL`], the current [`Moment`] and, when it changed,
    /// the local time for the header.
    fn due(&mut self) -> Option<Vec<Input>> {
        if self
            .ticked_at
            .is_some_and(|ticked_at| ticked_at.elapsed() < TICK_INTERVAL)
        {
            return None;
        }
        self.ticked_at = Some(Instant::now());
        let mut inputs = vec![Input::Tick(Moment::now())];
        let reading = local::now();
        if self.last_reading.as_ref() != Some(&reading) {
            self.last_reading = Some(reading.clone());
            inputs.push(Input::Clock(reading));
        }
        Some(inputs)
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
