//! Drives the focus/break timer with supplied clock values: the timer itself,
//! then the dashboard keys that act on it.
//!
//! No test reads a real clock. A [`Moment`] pairs a steady-clock reading with
//! a wall-clock time, so the two can be moved apart to simulate a change of
//! the system time.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use leme::config::Preferences;
use leme::dashboard::app::{App, Effect, Input, TimerStatus};
use leme::dashboard::{Snapshot, TimerAccess, view};
use leme::git::Checkout;
use leme::state::StateError;
use leme::time::Moment;
use leme::time::pomodoro::{DRIFT_TOLERANCE, Mode, Phase, Saved, SavedPhase, Timer};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const FOCUS: Duration = Duration::from_secs(25 * 60);
const BREAK: Duration = Duration::from_secs(5 * 60);
const PROJECT: &str = "0123456789abcdef0123456789abcdef";

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

/// A wall-clock time `seconds` after an arbitrary fixed date.
fn wall(seconds: u64) -> SystemTime {
    UNIX_EPOCH + secs(1_700_000_000 + seconds)
}

/// The moment `steady` seconds into the boot, with the wall clock showing
/// [`wall`]`(wall_seconds)`.
fn moment(steady: u64, wall_seconds: u64) -> Moment {
    Moment {
        steady: secs(steady),
        wall: wall(wall_seconds),
    }
}

/// A moment where the wall clock and the steady clock agree.
fn at(seconds: u64) -> Moment {
    moment(1_000 + seconds, seconds)
}

// The timer on its own.

#[test]
fn a_new_interval_runs_for_its_mode_length() {
    let focus = Timer::start(Mode::Focus, at(0));
    assert_eq!(focus.mode(), Mode::Focus);
    assert_eq!(focus.duration(), FOCUS);
    assert_eq!(focus.phase(), Phase::Running);
    assert_eq!(focus.remaining(at(0)), FOCUS);
    assert_eq!(focus.remaining(at(90)), FOCUS - secs(90));

    let rest = Timer::start(Mode::Break, at(0));
    assert_eq!(rest.duration(), BREAK);
    assert_eq!(rest.remaining(at(60)), BREAK - secs(60));
}

#[test]
fn pausing_freezes_the_time_left_until_resumed() {
    let mut timer = Timer::start(Mode::Focus, at(0));
    assert!(timer.pause(at(100)));
    assert_eq!(timer.phase(), Phase::Paused);
    assert_eq!(timer.remaining(at(100)), FOCUS - secs(100));
    assert_eq!(timer.remaining(at(5_000)), FOCUS - secs(100), "paused");

    assert!(timer.resume(at(5_000)));
    assert_eq!(timer.phase(), Phase::Running);
    assert_eq!(timer.remaining(at(5_060)), FOCUS - secs(160));

    assert!(timer.pause(at(5_100)));
    assert!(timer.resume(at(6_000)));
    assert_eq!(timer.remaining(at(6_000)), FOCUS - secs(200));
}

#[test]
fn pause_and_resume_only_act_in_their_phase() {
    let mut timer = Timer::start(Mode::Focus, at(0));
    assert!(!timer.resume(at(10)), "already running");
    assert_eq!(timer.remaining(at(10)), FOCUS - secs(10));

    assert!(timer.pause(at(10)));
    assert!(!timer.pause(at(20)), "already paused");
    assert_eq!(timer.remaining(at(20)), FOCUS - secs(10));

    let mut done = Timer::start(Mode::Break, at(0));
    assert!(done.update(at(300)));
    assert!(!done.pause(at(301)));
    assert!(!done.resume(at(301)));
    assert_eq!(done.phase(), Phase::Elapsed);
}

#[test]
fn an_interval_elapses_when_its_time_runs_out() {
    let mut timer = Timer::start(Mode::Break, at(0));
    assert!(!timer.update(at(299)), "nothing to save before the end");
    assert_eq!(timer.phase(), Phase::Running);
    assert_eq!(timer.remaining(at(299)), secs(1));

    assert!(timer.update(at(303)));
    assert_eq!(timer.phase(), Phase::Elapsed);
    assert_eq!(timer.remaining(at(303)), Duration::ZERO);
    assert_eq!(timer.finished_at(), Some(wall(300)), "when it ran out");
    assert!(!timer.update(at(400)), "elapsed stays elapsed");
}

#[test]
fn the_time_left_never_goes_below_zero() {
    let timer = Timer::start(Mode::Break, at(0));
    assert_eq!(timer.remaining(at(10_000)), Duration::ZERO);
    // A steady reading from before the start, as across a reboot, counts as
    // no time passed.
    assert_eq!(timer.remaining(moment(0, 50)), BREAK);
}

#[test]
fn pausing_after_the_time_ran_out_elapses_instead() {
    let mut timer = Timer::start(Mode::Break, at(0));
    assert!(timer.pause(at(320)));
    assert_eq!(timer.phase(), Phase::Elapsed);
    assert_eq!(timer.finished_at(), Some(wall(300)));
}

#[test]
fn moving_the_system_clock_does_not_change_the_countdown() {
    for wall_seconds in [3_600 + 10, 0, 7 * 24 * 3_600] {
        let mut timer = Timer::start(Mode::Focus, at(0));
        // Ten steady seconds pass while the wall clock jumps.
        let jumped = moment(1_010, wall_seconds);
        assert_eq!(timer.remaining(jumped), FOCUS - secs(10), "{wall_seconds}");
        timer.update(jumped);
        assert_eq!(timer.phase(), Phase::Running);
        assert_eq!(timer.remaining(jumped), FOCUS - secs(10));
        assert_eq!(
            timer.remaining(moment(1_070, wall_seconds + 60)),
            FOCUS - secs(70)
        );
    }
}

#[test]
fn a_clock_change_moves_the_saved_start_so_recovery_stays_right() {
    let mut timer = Timer::start(Mode::Focus, at(0));
    // Ten seconds in, the wall clock is set one hour ahead.
    let jumped = moment(1_010, 3_610);
    assert!(timer.update(jumped), "the saved start must be corrected");
    assert_eq!(
        timer.saved().phase,
        SavedPhase::Running {
            remaining: FOCUS - secs(10),
            since: wall(3_610),
        }
    );

    // Closed now and reopened 50 wall-clock seconds later, after a reboot:
    // 60 seconds have run in total, not one hour.
    let reopened = Timer::restore(&timer.saved(), moment(5, 3_660));
    assert_eq!(reopened.phase(), Phase::Running);
    assert_eq!(reopened.remaining(moment(5, 3_660)), FOCUS - secs(60));
}

#[test]
fn a_clock_set_back_is_corrected_too() {
    let mut timer = Timer::start(Mode::Focus, at(1_000));
    let jumped = moment(2_010, 400);
    assert!(timer.update(jumped));
    assert_eq!(timer.remaining(jumped), FOCUS - secs(10));
    assert_eq!(
        timer.saved().phase,
        SavedPhase::Running {
            remaining: FOCUS - secs(10),
            since: wall(400),
        }
    );
}

#[test]
fn small_differences_between_the_clocks_are_tolerated() {
    let mut timer = Timer::start(Mode::Focus, at(0));
    let saved = timer.saved();
    let slightly_off = Moment {
        steady: secs(1_100),
        wall: wall(100) + DRIFT_TOLERANCE,
    };
    assert!(!timer.update(slightly_off), "no write for reading jitter");
    assert_eq!(timer.saved(), saved);
}

#[test]
fn the_finish_time_follows_the_steady_clock_after_a_jump() {
    let mut timer = Timer::start(Mode::Break, at(0));
    // 310 steady seconds later the wall clock reads a day ahead.
    assert!(timer.update(moment(1_310, 86_400 + 310)));
    assert_eq!(timer.phase(), Phase::Elapsed);
    assert_eq!(timer.finished_at(), Some(wall(86_400 + 300)));
}

#[test]
fn the_saved_form_reads_back_unchanged() {
    let start = Moment {
        steady: secs(1_000),
        wall: wall(0) + Duration::from_nanos(123_456_789),
    };
    let mut timer = Timer::start(Mode::Focus, start);
    let saved = timer.saved();
    assert_eq!(
        saved.phase,
        SavedPhase::Running {
            remaining: FOCUS,
            since: wall(0) + Duration::from_millis(123),
        },
        "rounded down to whole milliseconds"
    );
    let later = Moment {
        steady: secs(1_000) + Duration::from_nanos(61_500_700_000),
        wall: wall(61),
    };
    timer.pause(later);
    let saved = timer.saved();
    assert_eq!(
        saved.phase,
        SavedPhase::Paused {
            remaining: FOCUS - Duration::from_millis(61_501),
        }
    );
    assert_eq!(Timer::restore(&saved, at(9_999)).saved(), saved);
}

// Recovery from the saved form.

fn saved(phase: SavedPhase) -> Saved {
    Saved {
        mode: Mode::Focus,
        duration: FOCUS,
        phase,
    }
}

#[test]
fn a_paused_interval_reopens_paused_with_the_same_time_left() {
    let paused = saved(SavedPhase::Paused {
        remaining: secs(600),
    });
    let timer = Timer::restore(&paused, at(86_400));
    assert_eq!(timer.phase(), Phase::Paused);
    assert_eq!(timer.remaining(at(90_000)), secs(600));
    assert_eq!(timer.saved(), paused);
}

#[test]
fn a_running_interval_loses_the_time_spent_closed() {
    let running = saved(SavedPhase::Running {
        remaining: secs(600),
        since: wall(1_000),
    });
    let now = moment(7, 1_240);
    let timer = Timer::restore(&running, now);
    assert_eq!(timer.phase(), Phase::Running);
    assert_eq!(timer.remaining(now), secs(360));
    assert_eq!(timer.remaining(moment(67, 1_300)), secs(300));
}

#[test]
fn an_interval_that_ran_out_while_closed_reopens_elapsed() {
    let running = saved(SavedPhase::Running {
        remaining: secs(600),
        since: wall(1_000),
    });
    let timer = Timer::restore(&running, at(5_000));
    assert_eq!(timer.phase(), Phase::Elapsed);
    assert_eq!(timer.finished_at(), Some(wall(1_600)));

    let exactly = Timer::restore(&running, at(1_600));
    assert_eq!(exactly.phase(), Phase::Elapsed);
}

#[test]
fn an_elapsed_interval_reopens_elapsed() {
    let elapsed = saved(SavedPhase::Elapsed {
        finished_at: wall(1_600),
    });
    let timer = Timer::restore(&elapsed, at(2_000));
    assert_eq!(timer.phase(), Phase::Elapsed);
    assert_eq!(timer.finished_at(), Some(wall(1_600)));
    assert!(!elapsed.is_active());
}

#[test]
fn a_wall_clock_behind_the_saved_start_counts_as_no_time_passed() {
    let running = saved(SavedPhase::Running {
        remaining: secs(600),
        since: wall(10_000),
    });
    let now = at(4_000);
    let timer = Timer::restore(&running, now);
    assert_eq!(timer.phase(), Phase::Running);
    assert_eq!(timer.remaining(now), secs(600));
}

// The dashboard keys, driven with supplied ticks.

fn snapshot(timer: Option<Saved>) -> Snapshot {
    Snapshot {
        project_id: PROJECT.to_owned(),
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

fn key(c: char) -> Input {
    Input::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

/// An app that has loaded a project with `timer` saved, as of `now`.
fn loaded(timer: Option<Saved>, now: Moment) -> (App, Option<Effect>) {
    let (mut app, _) = App::new();
    assert_eq!(app.handle(Input::Tick(now)), None);
    let effect = app.handle(Input::Collected(Ok(snapshot(timer))));
    (app, effect)
}

/// Presses `c` at `now`, as the dashboard loop does: a tick, then the key.
fn press(app: &mut App, c: char, now: Moment) -> Option<Effect> {
    assert_eq!(app.handle(Input::Tick(now)), None, "tick before {c:?}");
    app.handle(key(c))
}

fn status(mode: Mode, phase: Phase, remaining: Duration) -> Option<TimerStatus> {
    Some(TimerStatus {
        mode,
        phase,
        remaining,
    })
}

fn save(timer: Saved) -> Option<Effect> {
    Some(Effect::SaveTimer {
        project_id: PROJECT.to_owned(),
        timer,
    })
}

fn screen(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
    terminal.draw(|frame| view::render(app, frame)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer.content.iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn keys_start_pause_resume_and_cancel_the_timer() {
    let (mut app, _) = loaded(None, at(0));
    assert!(app.timer_ready());
    assert_eq!(app.timer(), None);
    assert!(screen(&app).contains("Timer  f focus  b break"));

    let started = press(&mut app, 'f', at(0));
    assert_eq!(
        started,
        Some(Effect::SaveTimer {
            project_id: PROJECT.to_owned(),
            timer: Timer::start(Mode::Focus, at(0)).saved(),
        })
    );
    assert_eq!(app.handle(Input::Tick(at(61))), None);
    assert_eq!(
        app.timer(),
        status(Mode::Focus, Phase::Running, FOCUS - secs(61))
    );
    assert!(screen(&app).contains("Focus 23:59  p pause  x cancel"));

    let paused = press(&mut app, 'p', at(100));
    assert_eq!(
        paused,
        save(saved(SavedPhase::Paused {
            remaining: FOCUS - secs(100),
        }))
    );
    app.handle(Input::Tick(at(500)));
    assert_eq!(
        app.timer(),
        status(Mode::Focus, Phase::Paused, FOCUS - secs(100))
    );
    assert!(screen(&app).contains("Focus 23:20 paused  p resume  x cancel"));

    let resumed = press(&mut app, ' ', at(500));
    assert_eq!(
        resumed,
        save(saved(SavedPhase::Running {
            remaining: FOCUS - secs(100),
            since: wall(500),
        }))
    );

    let cancelled = press(&mut app, 'x', at(510));
    assert_eq!(
        cancelled,
        Some(Effect::ClearTimer {
            project_id: PROJECT.to_owned(),
        })
    );
    assert_eq!(app.timer(), None);
    assert_eq!(press(&mut app, 'x', at(520)), None, "nothing to cancel");
    assert_eq!(press(&mut app, 'p', at(520)), None, "nothing to pause");
}

#[test]
fn a_running_or_paused_interval_is_not_replaced_by_a_new_start() {
    let (mut app, _) = loaded(None, at(0));
    press(&mut app, 'f', at(0));
    assert_eq!(press(&mut app, 'b', at(10)), None);
    assert_eq!(press(&mut app, 'f', at(10)), None);
    press(&mut app, 'p', at(20));
    assert_eq!(press(&mut app, 'b', at(30)), None);
    assert_eq!(
        app.timer(),
        status(Mode::Focus, Phase::Paused, FOCUS - secs(20))
    );
}

#[test]
fn the_countdown_elapses_on_a_tick_and_stays_visible() {
    let (mut app, _) = loaded(None, at(0));
    press(&mut app, 'b', at(0));
    assert_eq!(app.handle(Input::Tick(at(299))), None);
    assert!(screen(&app).contains("Break 00:01"));

    let finished = app.handle(Input::Tick(at(301)));
    assert_eq!(
        finished,
        Some(Effect::SaveTimer {
            project_id: PROJECT.to_owned(),
            timer: Saved {
                mode: Mode::Break,
                duration: BREAK,
                phase: SavedPhase::Elapsed {
                    finished_at: wall(300),
                },
            },
        })
    );
    app.handle(Input::Tick(at(5_000)));
    assert_eq!(
        app.timer(),
        status(Mode::Break, Phase::Elapsed, Duration::ZERO)
    );
    assert!(screen(&app).contains("Break done  f focus  b break  x clear"));

    // A new interval replaces it; x would dismiss it.
    assert!(matches!(
        press(&mut app, 'f', at(5_000)),
        Some(Effect::SaveTimer { .. })
    ));
}

#[test]
fn a_tick_corrects_the_saved_start_after_a_clock_change() {
    let (mut app, _) = loaded(None, at(0));
    press(&mut app, 'f', at(0));
    let effect = app.handle(Input::Tick(moment(1_010, 3_610)));
    assert_eq!(
        effect,
        save(saved(SavedPhase::Running {
            remaining: FOCUS - secs(10),
            since: wall(3_610),
        }))
    );
    assert_eq!(
        app.timer(),
        status(Mode::Focus, Phase::Running, FOCUS - secs(10))
    );
}

#[test]
fn the_saved_timer_is_restored_from_the_first_snapshot_only() {
    let paused = saved(SavedPhase::Paused {
        remaining: secs(600),
    });
    let (mut app, effect) = loaded(Some(paused), at(0));
    assert_eq!(effect, None, "a paused timer needs no write");
    assert_eq!(app.timer(), status(Mode::Focus, Phase::Paused, secs(600)));

    // A refresh carrying another saved timer leaves the screen's timer alone.
    app.handle(key('r'));
    let other = saved(SavedPhase::Paused { remaining: secs(5) });
    assert_eq!(
        app.handle(Input::Collected(Ok(snapshot(Some(other))))),
        None
    );
    assert_eq!(app.timer(), status(Mode::Focus, Phase::Paused, secs(600)));
}

#[test]
fn restoring_waits_for_the_time_and_saves_an_interval_that_ran_out() {
    let running = saved(SavedPhase::Running {
        remaining: secs(600),
        since: wall(1_000),
    });
    let (mut app, _) = App::new();
    assert_eq!(
        app.handle(Input::Collected(Ok(snapshot(Some(running))))),
        None
    );
    assert!(!app.timer_ready(), "the time is not known yet");
    assert_eq!(
        app.handle(key('f')),
        None,
        "timer keys wait for the restore"
    );

    let effect = app.handle(Input::Tick(at(9_000)));
    assert_eq!(
        effect,
        save(saved(SavedPhase::Elapsed {
            finished_at: wall(1_600),
        }))
    );
    assert!(app.timer_ready());
    assert_eq!(
        app.timer(),
        status(Mode::Focus, Phase::Elapsed, Duration::ZERO)
    );
}

#[test]
fn a_dashboard_that_does_not_own_the_timer_never_requests_a_write() {
    let (mut app, _) = App::new();
    app.handle(Input::Tick(at(0)));
    let mut held = snapshot(None);
    held.timer = TimerAccess::HeldElsewhere;
    assert_eq!(app.handle(Input::Collected(Ok(held))), None);
    assert!(!app.timer_ready());

    for (seconds, key) in [(1, 'f'), (2, 'b'), (3, 'p'), (4, ' '), (5, 'x')] {
        assert_eq!(press(&mut app, key, at(seconds)), None, "{key:?}");
    }
    // Ticks long past any interval, and across a clock jump, save nothing.
    for now in [at(301), at(1_501), moment(9_000, 86_400)] {
        assert_eq!(app.handle(Input::Tick(now)), None);
    }
    assert_eq!(app.timer(), None);
}

#[test]
fn a_failed_write_is_shown_until_a_write_succeeds() {
    let (mut app, _) = loaded(None, at(0));
    press(&mut app, 'f', at(0));
    let error = StateError::NoStateDirectory;
    app.handle(Input::TimerWritten(Err(error)));
    let message = app.timer_error().expect("error kept");
    assert!(
        message.starts_with("Timer not saved: cannot locate"),
        "{message}"
    );
    assert!(screen(&app).contains("Timer not saved"));
    assert_eq!(
        app.timer(),
        status(Mode::Focus, Phase::Running, FOCUS),
        "the timer keeps running on screen"
    );

    app.handle(Input::TimerWritten(Ok(())));
    assert_eq!(app.timer_error(), None);
}

#[test]
fn timer_keys_are_ignored_behind_help_and_before_a_project_loads() {
    let (mut app, _) = App::new();
    app.handle(Input::Tick(at(0)));
    assert_eq!(app.handle(key('f')), None, "no project yet");
    assert!(!screen(&app).contains("f focus"));

    let (mut app, _) = loaded(None, at(0));
    app.handle(key('?'));
    assert_eq!(press(&mut app, 'f', at(1)), None);
    assert_eq!(app.timer(), None);
}
