//! What the dashboard shows and how input changes it.
//!
//! `App` is a plain state machine: it receives [`Input`] and may answer with
//! an [`Effect`] for the caller to carry out. It never reads the terminal,
//! a clock, Git or the state database, so it can be driven without a live
//! screen and with any supplied time.

use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{CollectError, Snapshot};
use crate::config::ClockFormat;
use crate::state::StateError;
use crate::time::Moment;
use crate::time::local::Reading;
use crate::time::pomodoro::{Mode, Phase, Saved, Timer};

/// Something that happened and may change the screen.
#[derive(Debug)]
pub enum Input {
    /// A key was pressed.
    Key(KeyEvent),
    /// A collection started by [`Effect::Collect`] finished.
    Collected(Result<Snapshot, CollectError>),
    /// The local time was read. The caller reads it on its own schedule,
    /// independently of collections.
    Clock(Reading),
    /// The current time. The caller sends one regularly, and immediately
    /// before every key, so the timer counts down and timer keys act at the
    /// moment they are pressed.
    Tick(Moment),
    /// A timer write requested by an effect finished. `Some` carries the
    /// running or paused timer another dashboard had already started, which
    /// [`Effect::StartTimer`] left in place.
    TimerWritten(Result<Option<Saved>, StateError>),
}

/// Work the caller must carry out on the app's behalf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Collect a new [`Snapshot`] in the background and deliver it as
    /// [`Input::Collected`]. Collection only reads Git and never starts an
    /// agent.
    Collect,
    /// Save `timer` as the project's new timer unless another running or
    /// paused timer is already saved, and deliver [`Input::TimerWritten`].
    StartTimer { project_id: String, timer: Saved },
    /// Replace the project's saved timer and deliver [`Input::TimerWritten`].
    SaveTimer { project_id: String, timer: Saved },
    /// Remove the project's saved timer and deliver [`Input::TimerWritten`].
    ClearTimer { project_id: String },
}

/// What the dashboard shows about the timer at the latest tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerStatus {
    pub mode: Mode,
    pub phase: Phase,
    /// Time left, zero once elapsed.
    pub remaining: Duration,
}

/// The state of the most recent collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load {
    /// A collection is running.
    Loading,
    /// The last collection succeeded.
    Ready,
    /// The last collection failed with this message.
    Failed(String),
}

/// The dashboard's screen state.
#[derive(Debug)]
pub struct App {
    /// The last successfully collected snapshot, kept while a refresh runs or
    /// after it fails.
    snapshot: Option<Snapshot>,
    load: Load,
    /// Index of the selected task, meaningful only when there are tasks.
    selected: usize,
    help: bool,
    quit: bool,
    /// The most recent local-time reading, if any has arrived.
    clock: Option<Reading>,
    /// The time of the latest tick.
    now: Option<Moment>,
    timer: Option<Timer>,
    /// Whether the saved timer from the first snapshot has been restored.
    /// Timer keys wait for it, so they never act on a stale idle state.
    timer_restored: bool,
    /// Why the last timer write failed, until one succeeds.
    timer_error: Option<String>,
}

impl App {
    /// Returns the app and the first collection it needs.
    ///
    /// The app starts in [`Load::Loading`] with nothing to show, so the caller
    /// must carry out the returned effect.
    pub fn new() -> (Self, Effect) {
        let app = Self {
            snapshot: None,
            load: Load::Loading,
            selected: 0,
            help: false,
            quit: false,
            clock: None,
            now: None,
            timer: None,
            timer_restored: false,
            timer_error: None,
        };
        (app, Effect::Collect)
    }

    /// The last successfully collected snapshot.
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// The state of the most recent collection.
    pub fn load(&self) -> &Load {
        &self.load
    }

    /// Index of the selected task, or `None` when there are no tasks.
    pub fn selected(&self) -> Option<usize> {
        self.task_count()
            .checked_sub(1)
            .map(|last| self.selected.min(last))
    }

    /// Whether the help overlay is open.
    pub fn help_visible(&self) -> bool {
        self.help
    }

    /// The most recent local-time reading, or `None` before the first one.
    pub fn clock(&self) -> Option<&Reading> {
        self.clock.as_ref()
    }

    /// How the clock is written: the project's preference once a snapshot
    /// has loaded, and the default before that or when loading fails.
    pub fn clock_format(&self) -> ClockFormat {
        self.snapshot
            .as_ref()
            .map_or_else(ClockFormat::default, |snapshot| {
                snapshot.preferences.clock_format
            })
    }

    /// The timer as of the latest tick, or `None` when there is none.
    pub fn timer(&self) -> Option<TimerStatus> {
        let (timer, now) = (self.timer.as_ref()?, self.now?);
        Some(TimerStatus {
            mode: timer.mode(),
            phase: timer.phase(),
            remaining: timer.remaining(now),
        })
    }

    /// Whether timer keys work: the project and its saved timer have loaded
    /// and the time is known.
    pub fn timer_ready(&self) -> bool {
        self.timer_restored
    }

    /// Why the last timer write failed, until a later one succeeds.
    pub fn timer_error(&self) -> Option<&str> {
        self.timer_error.as_deref()
    }

    /// Whether the user asked to leave the dashboard.
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Applies `input` and returns any work the caller must carry out.
    pub fn handle(&mut self, input: Input) -> Option<Effect> {
        match input {
            Input::Key(key) => self.key(key),
            Input::Collected(result) => self.collected(result),
            Input::Clock(reading) => {
                self.clock = Some(reading);
                None
            }
            Input::Tick(now) => self.tick(now),
            Input::TimerWritten(result) => {
                self.timer_written(result);
                None
            }
        }
    }

    fn key(&mut self, key: KeyEvent) -> Option<Effect> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return None;
        }
        if self.help {
            match key.code {
                KeyCode::Char('?') | KeyCode::Esc => self.help = false,
                KeyCode::Char('q') => self.quit = true,
                _ => {}
            }
            return None;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('r') => return self.refresh(),
            KeyCode::Up | KeyCode::Char('k') => {
                self.select(self.selected().map(|i| i.saturating_sub(1)))
            }
            KeyCode::Down | KeyCode::Char('j') => self.select(self.selected().map(|i| i + 1)),
            KeyCode::Home | KeyCode::Char('g') => self.select(Some(0)),
            KeyCode::End | KeyCode::Char('G') => self.select(Some(usize::MAX)),
            KeyCode::Char('f') => return self.start_timer(Mode::Focus),
            KeyCode::Char('b') => return self.start_timer(Mode::Break),
            KeyCode::Char('p') | KeyCode::Char(' ') => return self.toggle_pause(),
            KeyCode::Char('x') => return self.cancel_timer(),
            _ => {}
        }
        None
    }

    fn tick(&mut self, now: Moment) -> Option<Effect> {
        self.now = Some(now);
        if let Some(effect) = self.restore_timer() {
            return Some(effect);
        }
        let timer = self.timer.as_mut()?;
        timer.update(now).then(|| self.save_timer())?
    }

    /// Restores the saved timer once both the first snapshot and the time
    /// are known. Returns a save when restoring finished a running interval,
    /// so later processes see it elapsed too.
    fn restore_timer(&mut self) -> Option<Effect> {
        if self.timer_restored {
            return None;
        }
        let (snapshot, now) = (self.snapshot.as_ref()?, self.now?);
        self.timer_restored = true;
        let saved = snapshot.timer?;
        let timer = Timer::restore(&saved, now);
        let finished = saved.is_active() && timer.phase() == Phase::Elapsed;
        self.timer = Some(timer);
        finished.then(|| self.save_timer())?
    }

    /// Starts a new interval unless one is running or paused, which must be
    /// cancelled first so a stray key cannot discard it.
    fn start_timer(&mut self, mode: Mode) -> Option<Effect> {
        let (now, project_id) = self.timer_context()?;
        if self
            .timer
            .as_ref()
            .is_some_and(|timer| timer.phase() != Phase::Elapsed)
        {
            return None;
        }
        let timer = Timer::start(mode, now);
        let saved = timer.saved();
        self.timer = Some(timer);
        Some(Effect::StartTimer {
            project_id,
            timer: saved,
        })
    }

    fn toggle_pause(&mut self) -> Option<Effect> {
        let (now, _) = self.timer_context()?;
        let timer = self.timer.as_mut()?;
        let changed = match timer.phase() {
            Phase::Running => timer.pause(now),
            Phase::Paused => timer.resume(now),
            Phase::Elapsed => false,
        };
        changed.then(|| self.save_timer())?
    }

    /// Cancels a running or paused interval, or dismisses an elapsed one.
    fn cancel_timer(&mut self) -> Option<Effect> {
        let (_, project_id) = self.timer_context()?;
        self.timer.take()?;
        Some(Effect::ClearTimer { project_id })
    }

    /// The time and project a timer key acts on, once timer keys work.
    fn timer_context(&self) -> Option<(Moment, String)> {
        if !self.timer_restored {
            return None;
        }
        Some((self.now?, self.snapshot.as_ref()?.project_id.clone()))
    }

    /// A save of the current timer, or `None` without a timer or project.
    fn save_timer(&self) -> Option<Effect> {
        Some(Effect::SaveTimer {
            project_id: self.snapshot.as_ref()?.project_id.clone(),
            timer: self.timer.as_ref()?.saved(),
        })
    }

    fn timer_written(&mut self, result: Result<Option<Saved>, StateError>) {
        match result {
            Ok(found) => {
                self.timer_error = None;
                // Another dashboard's interval wins over the one just started
                // here, so both show the same timer.
                if let (Some(saved), Some(now)) = (found, self.now) {
                    self.timer = Some(Timer::restore(&saved, now));
                }
            }
            Err(error) => self.timer_error = Some(format!("Timer not saved: {error}")),
        }
    }

    /// Starts a collection unless one is already running.
    fn refresh(&mut self) -> Option<Effect> {
        if self.load == Load::Loading {
            return None;
        }
        self.load = Load::Loading;
        Some(Effect::Collect)
    }

    fn collected(&mut self, result: Result<Snapshot, CollectError>) -> Option<Effect> {
        match result {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.load = Load::Ready;
                self.select(Some(self.selected));
                self.restore_timer()
            }
            Err(error) => {
                self.load = Load::Failed(error.to_string());
                None
            }
        }
    }

    /// Selects `index`, clamped to the task list.
    fn select(&mut self, index: Option<usize>) {
        let last = self.task_count().saturating_sub(1);
        self.selected = index.unwrap_or(0).min(last);
    }

    fn task_count(&self) -> usize {
        self.snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.tasks.len())
    }
}
