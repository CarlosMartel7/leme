//! Drives the dashboard's screen state and drawing without a live terminal.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use leme::config::Preferences;
use leme::dashboard::app::{App, Effect, Input, Load};
use leme::dashboard::{CollectError, Snapshot, view};
use leme::git::{Checkout, DiscoveryError};
use leme::state::Task;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn key(code: KeyCode) -> Input {
    Input::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn char_key(c: char) -> Input {
    key(KeyCode::Char(c))
}

fn snapshot(task_count: usize) -> Snapshot {
    Snapshot {
        project_id: "0123456789abcdef0123456789abcdef".to_owned(),
        checkout: Checkout {
            repository: PathBuf::from("/work/leme"),
            branch: Some("main".to_owned()),
            checkout: PathBuf::from("/work/leme"),
        },
        preferences: Preferences::default(),
        tasks: (0..task_count)
            .map(|index| Task {
                id: format!("T{index}"),
                branch: format!("feature/task-{index}"),
                worktree: PathBuf::from(format!("/work/leme-task-{index}")),
            })
            .collect(),
    }
}

fn failure() -> CollectError {
    CollectError::Discovery(DiscoveryError::NotARepository(PathBuf::from(
        "/tmp/elsewhere",
    )))
}

/// An app whose first collection returned `task_count` tasks.
fn loaded(task_count: usize) -> App {
    let (mut app, _) = App::new();
    assert_eq!(app.handle(Input::Collected(Ok(snapshot(task_count)))), None);
    app
}

/// Draws `app` on a `width` x `height` screen and returns its rows.
fn render(app: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| view::render(app, frame)).unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

fn screen_contains(rows: &[String], text: &str) -> bool {
    rows.iter().any(|row| row.contains(text))
}

#[test]
fn a_new_app_is_loading_and_requests_its_first_collection() {
    let (app, effect) = App::new();
    assert_eq!(effect, Effect::Collect);
    assert_eq!(app.load(), &Load::Loading);
    assert_eq!(app.snapshot(), None);
    assert_eq!(app.selected(), None);
    assert!(!app.help_visible());
    assert!(!app.should_quit());
}

#[test]
fn a_collected_snapshot_shows_the_project_with_an_empty_task_list() {
    let app = loaded(0);
    assert_eq!(app.load(), &Load::Ready);
    assert_eq!(app.snapshot(), Some(&snapshot(0)));
    assert_eq!(app.selected(), None);

    let rows = render(&app, 80, 24);
    assert!(screen_contains(&rows, "0123456789abcdef0123456789abcdef"));
    assert!(screen_contains(&rows, "Tasks (0)"));
    assert!(screen_contains(&rows, "No tasks are registered"));
    assert!(screen_contains(&rows, "Up to date"));
}

#[test]
fn refresh_requests_only_a_read_only_collection() {
    let mut app = loaded(0);
    let effect = app.handle(char_key('r')).expect("refresh effect");
    // Every effect the app can request is listed here, so adding one, such as
    // an agent turn, fails to compile until this test decides about it.
    match effect {
        Effect::Collect => {}
    }
    assert_eq!(app.load(), &Load::Loading);
    assert_eq!(app.snapshot(), Some(&snapshot(0)), "keeps showing old data");
    assert!(screen_contains(&render(&app, 80, 24), "Refreshing"));
}

#[test]
fn refresh_is_ignored_while_a_collection_is_running() {
    let (mut app, _) = App::new();
    assert_eq!(app.handle(char_key('r')), None);

    let mut app = loaded(0);
    assert_eq!(app.handle(char_key('r')), Some(Effect::Collect));
    assert_eq!(app.handle(char_key('r')), None);
}

#[test]
fn a_failed_first_collection_shows_the_error_and_can_be_retried() {
    let (mut app, _) = App::new();
    app.handle(Input::Collected(Err(failure())));
    let Load::Failed(message) = app.load() else {
        panic!("expected a failure, got {:?}", app.load());
    };
    assert!(message.contains("/tmp/elsewhere is not inside a Git repository"));
    assert_eq!(app.snapshot(), None);

    let rows = render(&app, 80, 24);
    assert!(screen_contains(&rows, "Cannot load the project"));
    assert!(screen_contains(&rows, "/tmp/elsewhere is not inside a Git"));

    assert_eq!(app.handle(char_key('r')), Some(Effect::Collect));
    app.handle(Input::Collected(Ok(snapshot(0))));
    assert_eq!(app.load(), &Load::Ready);
}

#[test]
fn a_failed_refresh_keeps_the_previous_snapshot() {
    let mut app = loaded(2);
    app.handle(char_key('r'));
    app.handle(Input::Collected(Err(failure())));
    assert!(matches!(app.load(), Load::Failed(_)));
    assert_eq!(app.snapshot(), Some(&snapshot(2)));

    let rows = render(&app, 80, 24);
    assert!(screen_contains(&rows, "Refresh failed"));
    assert!(screen_contains(&rows, "Tasks (2)"));
}

#[test]
fn help_opens_and_closes_and_blocks_other_keys() {
    let mut app = loaded(3);
    app.handle(char_key('?'));
    assert!(app.help_visible());
    assert!(screen_contains(&render(&app, 80, 24), "Help"));

    assert_eq!(app.handle(char_key('r')), None, "no refresh behind help");
    app.handle(char_key('j'));
    assert_eq!(app.selected(), Some(0), "no navigation behind help");
    assert!(app.help_visible());

    app.handle(char_key('?'));
    assert!(!app.help_visible());
    app.handle(char_key('?'));
    app.handle(key(KeyCode::Esc));
    assert!(!app.help_visible());
    assert!(!app.should_quit(), "Esc closes help before quitting");
}

#[test]
fn q_esc_and_ctrl_c_quit() {
    for input in [
        char_key('q'),
        key(KeyCode::Esc),
        Input::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
    ] {
        let mut app = loaded(0);
        assert_eq!(app.handle(input), None);
        assert!(app.should_quit());
    }
}

#[test]
fn q_and_ctrl_c_quit_from_help_and_while_loading() {
    let mut app = loaded(0);
    app.handle(char_key('?'));
    app.handle(char_key('q'));
    assert!(app.should_quit());

    let mut app = loaded(0);
    app.handle(char_key('?'));
    app.handle(Input::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.should_quit());

    let (mut app, _) = App::new();
    app.handle(char_key('q'));
    assert!(app.should_quit());
}

#[test]
fn key_releases_are_ignored() {
    let mut app = loaded(0);
    let release = KeyEvent::new_with_kind(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    );
    app.handle(Input::Key(release));
    assert!(!app.should_quit());
}

#[test]
fn navigation_stays_within_the_task_list() {
    let mut app = loaded(3);
    assert_eq!(app.selected(), Some(0));
    for _ in 0..5 {
        app.handle(key(KeyCode::Down));
    }
    assert_eq!(app.selected(), Some(2));
    for _ in 0..5 {
        app.handle(char_key('k'));
    }
    assert_eq!(app.selected(), Some(0));
    app.handle(char_key('G'));
    assert_eq!(app.selected(), Some(2));
    app.handle(key(KeyCode::Home));
    assert_eq!(app.selected(), Some(0));
    app.handle(char_key('j'));
    assert_eq!(app.selected(), Some(1));

    let mut empty = loaded(0);
    empty.handle(key(KeyCode::Down));
    assert_eq!(empty.selected(), None);
}

#[test]
fn the_selection_follows_a_refresh_that_shortens_the_list() {
    let mut app = loaded(3);
    app.handle(key(KeyCode::End));
    app.handle(char_key('r'));
    app.handle(Input::Collected(Ok(snapshot(1))));
    assert_eq!(app.selected(), Some(0));
    app.handle(Input::Collected(Ok(snapshot(0))));
    assert_eq!(app.selected(), None);
}

#[test]
fn the_screen_adapts_to_resizing() {
    let mut app = loaded(2);
    let wide = render(&app, 100, 30);
    assert!(screen_contains(
        &wide,
        "↑↓ select  r refresh  ? help  q quit"
    ));
    assert!(screen_contains(&wide, "feature/task-1"));
    assert!(screen_contains(&wide, "/work/leme-task-1"));

    let narrow = render(&app, 32, 16);
    assert!(screen_contains(&narrow, "r refresh  ? help  q quit"));
    assert!(!screen_contains(&narrow, "↑↓ select"));
    assert!(
        screen_contains(&narrow, "Project   …def0123456789abcdef│"),
        "long values keep their end: {narrow:#?}"
    );

    // Drawing never panics, even on screens too small to show everything.
    for (width, height) in [(1, 1), (10, 3), (20, 6), (32, 10)] {
        render(&app, width, height);
    }
    app.handle(char_key('?'));
    for (width, height) in [(1, 1), (10, 3), (20, 6), (32, 10)] {
        render(&app, width, height);
    }
    let (loading, _) = App::new();
    for (width, height) in [(1, 1), (20, 6)] {
        render(&loading, width, height);
    }
}

#[test]
fn the_screen_shows_loading_before_the_first_snapshot() {
    let (app, _) = App::new();
    let rows = render(&app, 40, 16);
    assert!(screen_contains(&rows, "Loading the project"));
    assert!(screen_contains(&rows, "Not loaded yet"));
}
