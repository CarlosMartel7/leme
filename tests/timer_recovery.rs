//! Saves the focus/break timer in a real SQLite state database, reopens it as
//! a new dashboard process would, and checks what that process recovers.
//!
//! Each test uses its own database in a disposable directory. Clock values
//! are supplied, so no test waits for real time to pass. Which dashboard may
//! write the timer is tested in `timer_ownership.rs`.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use leme::config::Preferences;
use leme::dashboard::app::{App, Effect, Input, TimerStatus};
use leme::dashboard::{Snapshot, TimerAccess};
use leme::git::Checkout;
use leme::state::Store;
use leme::time::Moment;
use leme::time::pomodoro::{Mode, Phase, Saved, SavedPhase, Timer};
use rusqlite::{Connection, params};
use support::TestDir;

const FOCUS: Duration = Duration::from_secs(25 * 60);
const BREAK: Duration = Duration::from_secs(5 * 60);

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// A wall-clock time `seconds` after an arbitrary fixed date.
fn wall(seconds: u64) -> SystemTime {
    UNIX_EPOCH + secs(1_700_000_000 + seconds)
}

/// A moment in a fresh process: its steady clock has nothing in common with
/// the process that saved the timer.
fn moment(steady: u64, wall_seconds: u64) -> Moment {
    Moment {
        steady: secs(steady),
        wall: wall(wall_seconds),
    }
}

fn database(root: &TestDir) -> PathBuf {
    root.path().join("state/leme.db")
}

/// Opens the store at `path` and registers the project whose main checkout
/// is `repository`, returning both.
fn open(path: &Path, repository: &str) -> (Store, String) {
    let mut store = Store::open(path).expect("open the state database");
    let project = store.project(Path::new(repository)).expect("project");
    (store, project.id)
}

/// What a new process reads back for `repository`.
fn reopened(path: &Path, repository: &str) -> Option<Saved> {
    let (mut store, project_id) = open(path, repository);
    store.timer(&project_id).expect("read the timer")
}

fn focus(phase: SavedPhase) -> Saved {
    Saved {
        mode: Mode::Focus,
        duration: FOCUS,
        phase,
    }
}

#[test]
fn a_paused_interval_reopens_paused_with_the_same_time_left() {
    let root = TestDir::new();
    let path = database(&root);
    let (mut store, project_id) = open(&path, "/work/leme");
    let mut timer = Timer::start(Mode::Focus, moment(500, 0));
    timer.pause(Moment {
        steady: secs(500) + Duration::from_millis(754_321),
        wall: wall(754),
    });
    store.save_timer(&project_id, &timer.saved()).unwrap();
    drop(store);

    let saved = reopened(&path, "/work/leme").expect("a saved timer");
    assert_eq!(
        saved,
        focus(SavedPhase::Paused {
            remaining: FOCUS - Duration::from_millis(754_321),
        })
    );
    // Days later, in a process with an unrelated steady clock.
    let now = moment(3, 3 * 86_400);
    let recovered = Timer::restore(&saved, now);
    assert_eq!(recovered.phase(), Phase::Paused);
    assert_eq!(
        recovered.remaining(now),
        FOCUS - Duration::from_millis(754_321)
    );
}

#[test]
fn a_running_interval_reopens_with_the_closed_time_deducted() {
    let root = TestDir::new();
    let path = database(&root);
    let (mut store, project_id) = open(&path, "/work/leme");
    let timer = Timer::start(Mode::Break, moment(9_000, 1_000));
    store.save_timer(&project_id, &timer.saved()).unwrap();
    drop(store);

    let saved = reopened(&path, "/work/leme").expect("a saved timer");
    let now = moment(12, 1_120);
    let recovered = Timer::restore(&saved, now);
    assert_eq!(recovered.mode(), Mode::Break);
    assert_eq!(recovered.phase(), Phase::Running);
    assert_eq!(recovered.remaining(now), BREAK - secs(120));
    // It keeps counting on the new process's steady clock.
    assert_eq!(recovered.remaining(moment(72, 1_180)), BREAK - secs(180));
}

#[test]
fn an_interval_that_ran_out_while_closed_is_shown_and_saved_as_elapsed() {
    let root = TestDir::new();
    let path = database(&root);
    let (mut store, project_id) = open(&path, "/work/leme");
    store
        .save_timer(
            &project_id,
            &focus(SavedPhase::Running {
                remaining: secs(600),
                since: wall(1_000),
            }),
        )
        .unwrap();
    drop(store);

    // A new dashboard loads the snapshot and receives its first tick.
    let (mut store, project_id) = open(&path, "/work/leme");
    let saved = store.timer(&project_id).unwrap();
    let (mut app, _) = App::new();
    app.handle(Input::Collected(Ok(snapshot(&project_id, saved))));
    let effect = app.handle(Input::Tick(moment(4, 9_000)));
    assert_eq!(
        app.timer(),
        Some(TimerStatus {
            mode: Mode::Focus,
            phase: Phase::Elapsed,
            remaining: Duration::ZERO,
        })
    );
    apply(&mut store, effect.expect("the elapsed state is saved"));
    drop(store);

    let elapsed = focus(SavedPhase::Elapsed {
        finished_at: wall(1_600),
    });
    assert_eq!(reopened(&path, "/work/leme"), Some(elapsed));
    let later = Timer::restore(&elapsed, moment(1, 20_000));
    assert_eq!(later.phase(), Phase::Elapsed);
    assert_eq!(later.finished_at(), Some(wall(1_600)));
}

#[test]
fn every_change_made_from_the_dashboard_survives_a_reopen() {
    let root = TestDir::new();
    let path = database(&root);
    let (mut store, project_id) = open(&path, "/work/leme");
    let (mut app, _) = App::new();
    app.handle(Input::Tick(moment(100, 0)));
    app.handle(Input::Collected(Ok(snapshot(&project_id, None))));

    for (key, now) in [('f', moment(100, 0)), ('p', moment(160, 60))] {
        app.handle(Input::Tick(now));
        let effect = app.handle(char_key(key)).expect("a timer write");
        apply(&mut store, effect);
    }
    assert_eq!(
        reopened(&path, "/work/leme"),
        Some(focus(SavedPhase::Paused {
            remaining: FOCUS - secs(60),
        }))
    );

    app.handle(Input::Tick(moment(400, 300)));
    apply(&mut store, app.handle(char_key(' ')).expect("resume"));
    assert_eq!(
        reopened(&path, "/work/leme"),
        Some(focus(SavedPhase::Running {
            remaining: FOCUS - secs(60),
            since: wall(300),
        }))
    );

    app.handle(Input::Tick(moment(410, 310)));
    apply(&mut store, app.handle(char_key('x')).expect("cancel"));
    assert_eq!(reopened(&path, "/work/leme"), None);
}

#[test]
fn each_project_keeps_its_own_timer() {
    let root = TestDir::new();
    let path = database(&root);
    let (mut store, first) = open(&path, "/work/one");
    let (_, second) = open(&path, "/work/two");
    let one = focus(SavedPhase::Paused {
        remaining: secs(100),
    });
    let two = Timer::start(Mode::Break, moment(10, 0)).saved();

    store.save_timer(&first, &one).unwrap();
    store.save_timer(&second, &two).unwrap();
    store.clear_timer(&first).unwrap();
    drop(store);

    assert_eq!(reopened(&path, "/work/one"), None);
    assert_eq!(reopened(&path, "/work/two"), Some(two));
}

#[test]
fn an_existing_database_gains_the_timer_table_and_keeps_its_project() {
    let root = TestDir::new();
    let path = database(&root);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        // A database written before timers existed: schema version 1.
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../src/state/migrations/001_project.sql"))
            .unwrap();
        connection
            .execute(
                "INSERT INTO projects (id, repository) VALUES ('p1', ?1)",
                params![b"/work/leme".as_slice()],
            )
            .unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
    }

    let (mut store, project_id) = open(&path, "/work/leme");
    assert_eq!(project_id, "p1");
    assert_eq!(store.timer(&project_id).unwrap(), None);
    let paused = focus(SavedPhase::Paused {
        remaining: secs(42),
    });
    store.save_timer(&project_id, &paused).unwrap();
    drop(store);

    let connection = Connection::open(&path).unwrap();
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 2);
    assert_eq!(reopened(&path, "/work/leme"), Some(paused));
}

#[test]
fn the_schema_rejects_timers_it_cannot_recover() {
    let root = TestDir::new();
    let path = database(&root);
    let (mut store, project_id) = open(&path, "/work/leme");
    assert!(
        store
            .save_timer(
                "no-such-project",
                &focus(SavedPhase::Paused { remaining: secs(1) })
            )
            .is_err(),
        "a timer belongs to a registered project"
    );
    drop(store);

    let connection = Connection::open(&path).unwrap();
    let insert =
        |phase: &str, remaining: Option<i64>, since: Option<i64>, finished: Option<i64>| {
            connection.execute(
                "INSERT OR REPLACE INTO timers
                 (project_id, mode, duration_ms, phase, remaining_ms, running_since_ms,
                  finished_at_ms)
             VALUES (?1, 'focus', 1500000, ?2, ?3, ?4, ?5)",
                params![project_id, phase, remaining, since, finished],
            )
        };
    assert!(
        insert("running", Some(1_000), None, None).is_err(),
        "no start"
    );
    assert!(insert("paused", None, None, None).is_err(), "no time left");
    assert!(
        insert("elapsed", None, None, None).is_err(),
        "no finish time"
    );
    assert!(
        insert("paused", Some(1_500_001), None, None).is_err(),
        "too long"
    );
    assert!(insert("paused", Some(-1), None, None).is_err(), "negative");
    assert!(
        insert("stopped", Some(1_000), None, None).is_err(),
        "unknown phase"
    );
    assert!(insert("paused", Some(1_000), None, None).is_ok());
}

/// Carries out a timer write the way the dashboard's writer does.
fn apply(store: &mut Store, effect: Effect) {
    match effect {
        Effect::SaveTimer { project_id, timer } => store.save_timer(&project_id, &timer).unwrap(),
        Effect::ClearTimer { project_id } => store.clear_timer(&project_id).unwrap(),
        Effect::Collect => panic!("expected a timer write"),
    }
}

fn snapshot(project_id: &str, timer: Option<Saved>) -> Snapshot {
    Snapshot {
        project_id: project_id.to_owned(),
        checkout: Checkout {
            repository: PathBuf::from("/work/leme"),
            branch: Some("main".to_owned()),
            checkout: PathBuf::from("/work/leme"),
        },
        preferences: Preferences::default(),
        tasks: Vec::new(),
        timer: TimerAccess::Owned(timer),
    }
}

fn char_key(c: char) -> Input {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    Input::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}
