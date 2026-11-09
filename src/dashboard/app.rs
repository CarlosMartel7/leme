//! What the dashboard shows and how input changes it.
//!
//! `App` is a plain state machine: it receives [`Input`] and may answer with
//! an [`Effect`] for the caller to carry out. It never reads the terminal,
//! Git or the state database, so it can be driven without a live screen.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{CollectError, Snapshot};

/// Something that happened and may change the screen.
#[derive(Debug)]
pub enum Input {
    /// A key was pressed.
    Key(KeyEvent),
    /// A collection started by [`Effect::Collect`] finished.
    Collected(Result<Snapshot, CollectError>),
}

/// Work the caller must carry out on the app's behalf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Collect a new [`Snapshot`] in the background and deliver it as
    /// [`Input::Collected`]. Collection only reads Git and never starts an
    /// agent.
    Collect,
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

    /// Whether the user asked to leave the dashboard.
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Applies `input` and returns any work the caller must carry out.
    pub fn handle(&mut self, input: Input) -> Option<Effect> {
        match input {
            Input::Key(key) => self.key(key),
            Input::Collected(result) => {
                self.collected(result);
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
            _ => {}
        }
        None
    }

    /// Starts a collection unless one is already running.
    fn refresh(&mut self) -> Option<Effect> {
        if self.load == Load::Loading {
            return None;
        }
        self.load = Load::Loading;
        Some(Effect::Collect)
    }

    fn collected(&mut self, result: Result<Snapshot, CollectError>) {
        match result {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.load = Load::Ready;
                self.select(Some(self.selected));
            }
            Err(error) => self.load = Load::Failed(error.to_string()),
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
