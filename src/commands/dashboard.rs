//! The `leme dashboard` command: runs the dashboard in the current terminal.

use std::env;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use clap::Args;
use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;

use super::Error;
use crate::dashboard::app::{App, Effect, Input};
use crate::dashboard::{self, view};
use crate::state::{StartOutcome, StateError, Store};
use crate::time::Moment;
use crate::time::local::{self, Reading};
use crate::time::pomodoro::Saved;

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
    // Declared after the session so it is dropped first: pending timer
    // writes finish before the terminal is restored and the process exits.
    let writer = TimerWriter::spawn(sender.clone());
    let (mut app, effect) = App::new();
    perform(effect, &directory, &sender, &writer);
    let mut ticker = Ticker::default();
    while !app.should_quit() {
        session
            .terminal
            .draw(|frame| view::render(&app, frame))
            .map_err(Error::Terminal)?;
        for input in next_inputs(&receiver, &mut ticker).map_err(Error::Terminal)? {
            if let Some(effect) = app.handle(input) {
                perform(effect, &directory, &sender, &writer);
            }
        }
    }
    Ok(())
}

/// Carries out an effect requested by the app.
fn perform(effect: Effect, directory: &Path, sender: &Sender<Input>, writer: &TimerWriter) {
    let write = match effect {
        Effect::Collect => {
            collect_in_background(directory.to_path_buf(), sender.clone());
            return;
        }
        Effect::StartTimer { project_id, timer } => TimerWrite::Start { project_id, timer },
        Effect::SaveTimer { project_id, timer } => TimerWrite::Save { project_id, timer },
        Effect::ClearTimer { project_id } => TimerWrite::Clear { project_id },
    };
    writer.send(write);
}

/// Collects a snapshot on another thread so keys stay responsive meanwhile.
fn collect_in_background(directory: PathBuf, sender: Sender<Input>) {
    thread::spawn(move || {
        // The receiver is gone only when the dashboard has already closed.
        let _ = sender.send(Input::Collected(dashboard::collect(&directory)));
    });
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

/// A timer change to store.
enum TimerWrite {
    Start { project_id: String, timer: Saved },
    Save { project_id: String, timer: Saved },
    Clear { project_id: String },
}

/// Stores timer changes on one background thread, in the order they were
/// made, so a slow or locked database never blocks keys and a pause followed
/// by a quick resume is never stored the other way round.
struct TimerWriter {
    jobs: Option<Sender<TimerWrite>>,
    thread: Option<JoinHandle<()>>,
}

impl TimerWriter {
    /// Starts the writer. Each result is delivered as
    /// [`Input::TimerWritten`] on `results`.
    fn spawn(results: Sender<Input>) -> Self {
        let (jobs, queue) = mpsc::channel::<TimerWrite>();
        let thread = thread::spawn(move || {
            let mut store = None;
            for job in queue {
                let result = write_timer(&mut store, job);
                // The receiver is gone only when the dashboard has closed.
                let _ = results.send(Input::TimerWritten(result));
            }
        });
        Self {
            jobs: Some(jobs),
            thread: Some(thread),
        }
    }

    fn send(&self, write: TimerWrite) {
        if let Some(jobs) = &self.jobs {
            // The thread only stops after a panic, already reported on stderr.
            let _ = jobs.send(write);
        }
    }
}

impl Drop for TimerWriter {
    /// Waits for queued writes, so quitting right after pausing keeps the
    /// pause.
    fn drop(&mut self) {
        drop(self.jobs.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Applies one timer change, opening the state database on first use and
/// again after a failure to open it.
fn write_timer(store: &mut Option<Store>, job: TimerWrite) -> Result<Option<Saved>, StateError> {
    let store = match store {
        Some(store) => store,
        None => store.insert(Store::open_default()?),
    };
    match job {
        TimerWrite::Start { project_id, timer } => {
            Ok(match store.start_timer(&project_id, &timer)? {
                StartOutcome::Started => None,
                StartOutcome::AlreadyActive(existing) => Some(existing),
            })
        }
        TimerWrite::Save { project_id, timer } => {
            store.save_timer(&project_id, &timer).map(|()| None)
        }
        TimerWrite::Clear { project_id } => store.clear_timer(&project_id).map(|()| None),
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
