//! Exclusive timer ownership: one dashboard per project owns the timer, and
//! only it shows or writes the timer.
//!
//! Dashboards here are assembled exactly as `leme dashboard` assembles them,
//! from real collections of disposable Git repositories, the app state
//! machine and the timer writer, against an isolated state database. Clock
//! values are supplied. Separate-process cases run this test binary again as
//! a lock-holding helper (see [`helper_lock_holder_process`]).

mod support;

use std::env;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use leme::dashboard::app::{App, Effect, Input, TimerControl};
use leme::dashboard::{Snapshot, TimerAccess, collect_with};
use leme::state::{Claim, StateError, Store, TimerOwnership, TimerWrite, TimerWriter};
use leme::time::Moment;
use leme::time::pomodoro::{Mode, Saved, Timer};
use rusqlite::Connection;
use support::{TestDir, add_worktree, init_repository};

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

fn wall(seconds: u64) -> SystemTime {
    UNIX_EPOCH + secs(1_700_000_000 + seconds)
}

/// A moment where the steady clock and the wall clock agree.
fn at(seconds: u64) -> Moment {
    Moment {
        steady: secs(1_000 + seconds),
        wall: wall(seconds),
    }
}

/// The isolated state database of a test.
fn database(root: &TestDir) -> PathBuf {
    root.path().join("state/leme/leme.db")
}

fn open_store(path: &Path) -> impl FnOnce() -> Result<Store, StateError> + use<> {
    let path = path.to_path_buf();
    move || Store::open(&path)
}

fn collect(directory: &Path, path: &Path, ownership: &TimerOwnership) -> Snapshot {
    collect_with(directory, open_store(path), ownership).expect("collect the project")
}

/// One open dashboard: its collections, app state and timer writer, wired as
/// in `leme dashboard`.
struct Dashboard {
    app: App,
    directory: PathBuf,
    database: PathBuf,
    ownership: TimerOwnership,
    writer: TimerWriter,
    results: Receiver<Result<(), StateError>>,
    /// Timer writes this dashboard's app requested.
    writes: Vec<TimerWrite>,
}

impl Dashboard {
    /// Opens a dashboard in `directory` at `now`.
    fn open(directory: &Path, database: &Path, now: Moment) -> Self {
        let ownership = TimerOwnership::new();
        let (sender, results) = mpsc::channel();
        let path = database.to_path_buf();
        let writer = TimerWriter::spawn(
            ownership.clone(),
            move || Store::open(&path),
            move |result| drop(sender.send(result)),
        );
        let (app, _) = App::new();
        let mut dashboard = Self {
            app,
            directory: directory.to_path_buf(),
            database: database.to_path_buf(),
            ownership,
            writer,
            results,
            writes: Vec::new(),
        };
        dashboard.tick(now);
        dashboard.refresh();
        dashboard
    }

    /// Runs a collection, as at opening or after `r`.
    fn refresh(&mut self) {
        let snapshot = collect(&self.directory, &self.database, &self.ownership);
        let effect = self.app.handle(Input::Collected(Ok(snapshot)));
        self.perform(effect);
    }

    fn tick(&mut self, now: Moment) {
        let effect = self.app.handle(Input::Tick(now));
        self.perform(effect);
    }

    /// Presses `c` at `now`: a tick, then the key.
    fn press(&mut self, c: char, now: Moment) {
        self.tick(now);
        let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let effect = self.app.handle(Input::Key(key));
        self.perform(effect);
    }

    fn perform(&mut self, effect: Option<Effect>) {
        let write = match effect {
            None | Some(Effect::Collect) => return,
            Some(Effect::SaveTimer { project_id, timer }) => TimerWrite::Save { project_id, timer },
            Some(Effect::ClearTimer { project_id }) => TimerWrite::Clear { project_id },
        };
        self.writes.push(write.clone());
        self.writer.send(write);
    }

    /// Waits for every requested write and hands the results to the app.
    fn settle(&mut self) {
        let expected = self.writes.len();
        let mut delivered = 0;
        while delivered < expected {
            let result = self
                .results
                .recv_timeout(secs(10))
                .expect("a timer write result");
            assert!(result.is_ok(), "an owner's write failed: {result:?}");
            self.app.handle(Input::TimerWritten(result));
            delivered += 1;
        }
        self.writes.clear();
    }

    fn control(&self) -> &TimerControl {
        self.app.timer_control()
    }
}

/// The saved timer as a newly opened process would read it.
fn stored(path: &Path, repository: &Path) -> Option<Saved> {
    let mut store = Store::open(path).unwrap();
    let id = store.project(repository).unwrap().id;
    store.timer(&id).unwrap()
}

// The two audited failures.

#[test]
fn an_older_dashboards_completion_cannot_overwrite_a_newer_interval() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    let path = database(&root);
    let mut owner = Dashboard::open(&repository, &path, at(0));
    let mut other = Dashboard::open(&repository, &path, at(0));
    assert_eq!(owner.control(), &TimerControl::Owner);
    assert_eq!(other.control(), &TimerControl::HeldElsewhere);

    owner.press('b', at(0));
    owner.settle();
    // The audited sequence: a second dashboard cancels the break and starts
    // a focus interval, then the first one's break runs out.
    other.press('x', at(5));
    other.press('f', at(10));
    assert_eq!(other.app.timer(), None, "nothing to cancel or start here");
    owner.press('x', at(10));
    owner.press('f', at(10));
    owner.settle();
    let newer = Timer::start(Mode::Focus, at(10)).saved();

    for seconds in [301, 302, 1_000] {
        other.tick(at(seconds));
        owner.tick(at(seconds));
    }
    owner.settle();

    assert!(other.writes.is_empty(), "a non-owner never writes");
    assert_eq!(stored(&path, &repository), Some(newer));
}

#[test]
fn a_queued_pause_after_a_start_cannot_replace_another_dashboards_timer() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    let path = database(&root);
    let mut owner = Dashboard::open(&repository, &path, at(0));
    owner.press('f', at(0));
    owner.settle();
    let focus = Timer::start(Mode::Focus, at(0)).saved();

    // The audited sequence: a second window presses b, then p before any
    // reply could arrive.
    let mut other = Dashboard::open(&repository, &path, at(5));
    other.press('b', at(5));
    other.press('p', at(6));
    assert!(other.writes.is_empty(), "neither key reaches the writer");
    assert_eq!(other.app.timer(), None);
    assert_eq!(stored(&path, &repository), Some(focus));

    // Even a write that reached a non-owner's writer would be refused.
    let project_id = other.app.snapshot().unwrap().project_id.clone();
    let paused_break = Timer::start(Mode::Break, at(5)).saved();
    other.writer.send(TimerWrite::Save {
        project_id: project_id.clone(),
        timer: paused_break,
    });
    other.writer.send(TimerWrite::Clear { project_id });
    for _ in 0..2 {
        let result = other.results.recv_timeout(secs(10)).unwrap();
        assert!(
            matches!(result, Err(StateError::TimerNotOwned)),
            "{result:?}"
        );
    }
    assert_eq!(stored(&path, &repository), Some(focus));
}

// Who becomes owner.

#[test]
fn simultaneous_claims_in_one_process_have_exactly_one_winner() {
    const CLAIMS: usize = 8;
    let root = TestDir::new();
    let path = database(&root);
    let project_id = Store::open(&path)
        .unwrap()
        .project(Path::new("/work/leme"))
        .unwrap()
        .id;
    let barrier = Arc::new(Barrier::new(CLAIMS));

    let claims: Vec<(TimerOwnership, Claim)> = thread::scope(|scope| {
        let handles: Vec<_> = (0..CLAIMS)
            .map(|_| {
                let (barrier, path, project_id) = (barrier.clone(), &path, &project_id);
                scope.spawn(move || {
                    // Each claim is a separate dashboard with its own
                    // connection and lock file descriptor.
                    let store = Store::open(path).unwrap();
                    let ownership = TimerOwnership::new();
                    barrier.wait();
                    let claim = ownership.claim(&store, project_id).unwrap();
                    (ownership, claim)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });

    let owners: Vec<_> = claims
        .iter()
        .filter(|(_, claim)| *claim == Claim::Owner)
        .collect();
    assert_eq!(owners.len(), 1, "{claims:?}");
    assert!(owners[0].0.owns(&project_id));
    let others = claims
        .iter()
        .filter(|(ownership, _)| !ownership.owns(&project_id));
    assert_eq!(others.count(), CLAIMS - 1);
}

#[test]
fn simultaneous_claims_from_separate_processes_have_exactly_one_winner() {
    const PROCESSES: usize = 6;
    let root = TestDir::new();
    let path = database(&root);
    Store::open(&path).unwrap();

    let mut holders: Vec<Holder> = (0..PROCESSES).map(|_| Holder::spawn(&path)).collect();
    let owners = holders
        .iter_mut()
        .map(Holder::owner)
        .filter(|owner| *owner)
        .count();
    assert_eq!(owners, 1);
    for holder in holders {
        assert!(holder.exit().success());
    }
}

#[test]
fn every_checkout_of_a_repository_shares_one_lock() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    let worktree = root.path().join("feature");
    init_repository(&repository);
    add_worktree(&repository, &worktree, "feature/one");
    let nested = worktree.join("src/deep");
    std::fs::create_dir_all(&nested).unwrap();
    let path = database(&root);

    let main = TimerOwnership::new();
    let first = collect(&repository, &path, &main);
    assert!(matches!(first.timer, TimerAccess::Owned(None)));

    for directory in [&worktree, &nested] {
        let snapshot = collect(directory, &path, &TimerOwnership::new());
        assert_eq!(snapshot.project_id, first.project_id, "{directory:?}");
        assert_eq!(snapshot.timer, TimerAccess::HeldElsewhere, "{directory:?}");
    }

    // The linked worktree owns the timer once the main checkout lets go.
    drop(main);
    let linked = collect(&worktree, &path, &TimerOwnership::new());
    assert!(matches!(linked.timer, TimerAccess::Owned(None)));
}

#[test]
fn separate_projects_have_independent_timers() {
    let root = TestDir::new();
    let (one, two) = (root.path().join("one"), root.path().join("two"));
    init_repository(&one);
    init_repository(&two);
    let path = database(&root);

    let mut first = Dashboard::open(&one, &path, at(0));
    let mut second = Dashboard::open(&two, &path, at(0));
    let mut first_again = Dashboard::open(&one, &path, at(0));
    assert_eq!(first.control(), &TimerControl::Owner);
    assert_eq!(second.control(), &TimerControl::Owner);
    assert_eq!(first_again.control(), &TimerControl::HeldElsewhere);

    first.press('f', at(0));
    second.press('b', at(0));
    first_again.press('b', at(1));
    first.settle();
    second.settle();
    assert_eq!(
        stored(&path, &one),
        Some(Timer::start(Mode::Focus, at(0)).saved())
    );
    assert_eq!(
        stored(&path, &two),
        Some(Timer::start(Mode::Break, at(0)).saved())
    );
}

// When ownership ends.

#[test]
fn the_lock_is_released_when_the_owner_exits_normally() {
    let root = TestDir::new();
    let path = database(&root);
    let mut holder = Holder::spawn(&path);
    assert!(holder.owner());
    assert_eq!(claim(&path), Claim::NotOwner);

    assert!(holder.exit().success(), "a normal exit");
    assert_eq!(claim(&path), Claim::Owner);
}

#[test]
fn the_lock_is_released_when_the_owner_crashes() {
    let root = TestDir::new();
    let path = database(&root);
    let mut holder = Holder::spawn(&path);
    assert!(holder.owner());
    assert_eq!(claim(&path), Claim::NotOwner);

    holder.child.kill().expect("SIGKILL the owner");
    holder.child.wait().unwrap();
    assert_eq!(claim(&path), Claim::Owner);
    // The kernel released the lock; the file was never removed.
    let lock_files = std::fs::read_dir(path.parent().unwrap().join("locks"))
        .unwrap()
        .count();
    assert_eq!(lock_files, 1);
}

#[test]
fn a_non_owner_stays_one_until_reopened_after_the_owner_exits() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    let path = database(&root);
    let mut owner = Dashboard::open(&repository, &path, at(0));
    owner.press('f', at(0));
    owner.press('p', at(60));
    owner.settle();
    let mut other = Dashboard::open(&repository, &path, at(70));
    drop(owner);

    other.refresh();
    other.press('x', at(80));
    assert_eq!(other.control(), &TimerControl::HeldElsewhere, "no takeover");
    assert!(other.writes.is_empty());
    let project_id = other.app.snapshot().unwrap().project_id.clone();
    assert!(
        !other.ownership.owns(&project_id),
        "a refresh must not quietly take the lock"
    );

    // A dashboard opened now becomes owner, even with the old non-owner
    // still open, and restores the paused interval.
    let reopened = Dashboard::open(&repository, &path, at(90));
    assert_eq!(reopened.control(), &TimerControl::Owner);
    let timer = reopened.app.timer().expect("the restored timer");
    assert_eq!(timer.remaining, Mode::Focus.duration() - secs(60));
    other.refresh();
    assert_eq!(other.control(), &TimerControl::HeldElsewhere);
}

#[test]
fn pending_writes_finish_before_ownership_moves() {
    let root = TestDir::new();
    let repository = root.path().join("project");
    init_repository(&repository);
    let path = database(&root);
    let mut owner = Dashboard::open(&repository, &path, at(0));
    // Claiming only touches the lock file, so this probe works while the
    // database is locked.
    let mut probe = Store::open(&path).unwrap();
    let project_id = probe.project(&repository).unwrap().id;

    // Another process holds the database's write lock, so the owner's next
    // write waits in its writer.
    let blocker = Connection::open(&path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    owner.press('f', at(0));

    // The owner quits: its app and handle go, and dropping the writer waits
    // for the pending write.
    let Dashboard {
        writer, ownership, ..
    } = owner;
    drop(ownership);
    let quitting = thread::spawn(move || drop(writer));
    thread::sleep(Duration::from_millis(200));
    assert!(!quitting.is_finished(), "the write is still pending");
    let early = TimerOwnership::new();
    assert_eq!(
        early.claim(&probe, &project_id).unwrap(),
        Claim::NotOwner,
        "ownership is still held"
    );

    blocker.execute_batch("COMMIT").unwrap();
    quitting.join().unwrap();
    let next = Dashboard::open(&repository, &path, at(30));
    assert_eq!(next.control(), &TimerControl::Owner);
    assert_eq!(
        stored(&path, &repository),
        Some(Timer::start(Mode::Focus, at(0)).saved()),
        "the new owner sees the finished write"
    );
}

/// Claims the timer of the helper's project with a new dashboard ownership.
fn claim(path: &Path) -> Claim {
    let mut store = Store::open(path).unwrap();
    let id = store.project(Path::new(HELPER_REPOSITORY)).unwrap().id;
    TimerOwnership::new().claim(&store, &id).unwrap()
}

// A separate process that claims a timer and holds it.

const HELPER_DATABASE: &str = "LEME_TEST_LOCK_HOLDER_DATABASE";
const HELPER_REPOSITORY: &str = "/work/lock-holder";

/// A helper process that claimed the timer of [`HELPER_REPOSITORY`].
struct Holder {
    child: Child,
    stdout: BufReader<ChildStdout>,
}

impl Holder {
    fn spawn(database: &Path) -> Self {
        let mut child = Command::new(env::current_exe().unwrap())
            .args(["--exact", "helper_lock_holder_process", "--nocapture"])
            .args(["--test-threads=1", "--quiet"])
            .env(HELPER_DATABASE, database)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the lock holder");
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self { child, stdout }
    }

    /// Whether the helper became owner; waits for it to report.
    fn owner(&mut self) -> bool {
        let mut line = String::new();
        loop {
            line.clear();
            assert_ne!(self.stdout.read_line(&mut line).unwrap(), 0, "no report");
            match line.trim() {
                "OWNER" => return true,
                "NOT OWNER" => return false,
                _ => {}
            }
        }
    }

    /// Lets the helper return normally and waits for it.
    fn exit(mut self) -> std::process::ExitStatus {
        drop(self.child.stdin.take());
        self.child.wait().unwrap()
    }
}

/// Not a test of its own: when started by [`Holder`], claims the timer like
/// a dashboard, reports the outcome and holds it until its input closes.
#[test]
fn helper_lock_holder_process() {
    let Some(database) = env::var_os(HELPER_DATABASE) else {
        return;
    };
    let mut store = Store::open(Path::new(&database)).unwrap();
    let id = store.project(Path::new(HELPER_REPOSITORY)).unwrap().id;
    let ownership = TimerOwnership::new();
    let claim = ownership.claim(&store, &id).unwrap();
    println!(
        "{}",
        if claim == Claim::Owner {
            "OWNER"
        } else {
            "NOT OWNER"
        }
    );
    let mut rest = String::new();
    while std::io::stdin().read_line(&mut rest).unwrap() > 0 {}
    drop(ownership);
}
