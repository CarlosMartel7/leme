//! Draws the dashboard's state into a Ratatui frame.
//!
//! Rendering only reads an [`App`]; it never collects data. Every part of the
//! layout fits a narrow terminal: long values keep their most specific end,
//! and messages wrap instead of being cut off.

use std::path::Path;
use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use super::app::{App, Load, TimerStatus};
use super::{Snapshot, clock};
use crate::time::pomodoro::Phase;

/// Width of the labels in the project header, including the gap after them.
const LABEL_WIDTH: usize = 10;

/// Below this width the footer shows the short form of its key hints.
const COMPACT_WIDTH: u16 = 48;

/// Height of the project header: four rows plus its border.
const HEADER_HEIGHT: u16 = 6;

/// Height of the header before any project is loaded, leaving the rest of a
/// small terminal to the loading or error message.
const EMPTY_HEADER_HEIGHT: u16 = 3;

/// Draws `app` over the whole frame.
pub fn render(app: &App, frame: &mut Frame) {
    let header_height = match app.snapshot() {
        Some(_) => HEADER_HEIGHT,
        None => EMPTY_HEADER_HEIGHT,
    };
    let [header, body, timer, status, footer] = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(app, frame, header);
    match app.snapshot() {
        Some(snapshot) => render_tasks(snapshot, app.selected(), frame, body),
        None => render_placeholder(app.load(), frame, body),
    }
    render_timer(app, frame, timer);
    render_status(app, frame, status);
    render_footer(frame, footer);
    if app.help_visible() {
        render_help(frame, frame.area());
    }
}

fn render_header(app: &App, frame: &mut Frame, area: Rect) {
    let mut block = Block::bordered().title(" Leme ".bold());
    if let Some(reading) = app.clock() {
        let time = clock::display(reading, app.clock_format());
        block = block.title_top(Line::from(format!(" {time} ")).right_aligned());
    }
    let snapshot = app.snapshot();
    let width = usize::from(block.inner(area).width).saturating_sub(LABEL_WIDTH);
    let lines = match snapshot {
        Some(snapshot) => vec![
            field("Project", &snapshot.project_id, width),
            field(
                "Branch",
                snapshot
                    .checkout
                    .branch
                    .as_deref()
                    .unwrap_or("(detached HEAD)"),
                width,
            ),
            field(
                "Checkout",
                &display_path(&snapshot.checkout.checkout),
                width,
            ),
            field(
                "Settings",
                &format!(
                    "{} · {}",
                    snapshot.preferences.integration_branch, snapshot.preferences.preferred_agent
                ),
                width,
            ),
        ],
        None => vec![Line::from("Not loaded yet".dim())],
    };
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true }).block(block);
    frame.render_widget(paragraph, area);
}

/// A labelled header row whose value is shortened to `width` characters.
fn field<'a>(label: &'a str, value: &str, width: usize) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<LABEL_WIDTH$}"), Style::new().dim()),
        Span::raw(keep_end(value, width)),
    ])
}

fn render_tasks(snapshot: &Snapshot, selected: Option<usize>, frame: &mut Frame, area: Rect) {
    let block = Block::bordered().title(format!(" Tasks ({}) ", snapshot.tasks.len()));
    if snapshot.tasks.is_empty() {
        let message = Paragraph::new(vec![
            Line::from("No tasks are registered for this project yet."),
            Line::from(""),
            Line::from("Tasks appear here once a feature is opened.".dim()),
        ])
        .wrap(Wrap { trim: true })
        .block(block);
        frame.render_widget(message, area);
        return;
    }
    let width = usize::from(block.inner(area).width).saturating_sub(2);
    let items = snapshot.tasks.iter().map(|task| {
        ListItem::new(vec![
            Line::from(vec![
                Span::styled(keep_end(&task.id, width), Style::new().bold()),
                Span::raw("  "),
                Span::raw(task.branch.clone()),
            ]),
            Line::from(keep_end(&display_path(&task.worktree), width).dim()),
        ])
    });
    let list = List::new(items)
        .block(block)
        .highlight_symbol("> ")
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list, area, &mut state);
}

/// The body shown before any snapshot has been collected.
fn render_placeholder(load: &Load, frame: &mut Frame, area: Rect) {
    let (block, lines) = match load {
        Load::Loading | Load::Ready => (
            Block::bordered().title(" Tasks "),
            vec![Line::from("Loading the project…")],
        ),
        Load::Failed(message) => (
            Block::bordered()
                .title(" Cannot load the project ")
                .border_style(Style::new().fg(Color::Red)),
            vec![
                Line::from(message.clone()),
                Line::from(""),
                Line::from("Press r to retry or q to quit.".dim()),
            ],
        ),
    };
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true }).block(block);
    frame.render_widget(paragraph, area);
}

/// One line with the focus/break timer and the keys that act on it.
fn render_timer(app: &App, frame: &mut Frame, area: Rect) {
    if !app.timer_ready() {
        frame.render_widget(Paragraph::new(""), area);
        return;
    }
    let width = usize::from(area.width);
    let (state, hints) = match app.timer() {
        None => (Span::raw("Timer"), "f focus  b break"),
        Some(timer) => timer_state(timer),
    };
    let line = match app.timer_error() {
        Some(error) => {
            let room = width.saturating_sub(state.width() + 2);
            Line::from(vec![
                state,
                Span::raw("  "),
                Span::raw(keep_start(error, room)).red(),
            ])
        }
        None => Line::from(vec![state, Span::raw("  "), Span::raw(hints).dim()]),
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// The timer's state, styled, and the keys that act on it.
fn timer_state(timer: TimerStatus) -> (Span<'static>, &'static str) {
    let label = timer.mode.label();
    let remaining = countdown(timer.remaining);
    match timer.phase {
        Phase::Running => (
            Span::raw(format!("{label} {remaining}")).green().bold(),
            "p pause  x cancel",
        ),
        Phase::Paused => (
            Span::raw(format!("{label} {remaining} paused")).yellow(),
            "p resume  x cancel",
        ),
        Phase::Elapsed => (
            Span::raw(format!("{label} done")).magenta().bold(),
            "f focus  b break  x clear",
        ),
    }
}

/// `remaining` as `mm:ss`, or `h:mm:ss` from an hour up, rounded up to the
/// next second so the display reaches `00:00` only when time runs out.
fn countdown(remaining: Duration) -> String {
    let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// One line describing the most recent collection.
fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    let width = usize::from(area.width);
    let line = match (app.load(), app.snapshot()) {
        (Load::Loading, Some(_)) => Line::from("Refreshing…".yellow()),
        (Load::Failed(message), Some(_)) => {
            Line::from(keep_start(&format!("Refresh failed: {message}"), width).red())
        }
        (Load::Ready, _) => Line::from("Up to date".green()),
        // Without a snapshot the body already shows the load state.
        (Load::Loading | Load::Failed(_), None) => Line::default(),
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn render_footer(frame: &mut Frame, area: Rect) {
    let hints = if area.width < COMPACT_WIDTH {
        "r refresh  ? help  q quit"
    } else {
        "↑↓ select  r refresh  ? help  q quit"
    };
    frame.render_widget(Paragraph::new(hints.dim()), area);
}

fn render_help(frame: &mut Frame, area: Rect) {
    let rows = [
        ("↑ k / ↓ j", "select a task"),
        ("g / G", "first / last task"),
        ("f / b", "start focus / break"),
        ("p Space", "pause / resume timer"),
        ("x", "cancel the timer"),
        ("r", "refresh"),
        ("?", "close this help"),
        ("q Esc", "quit"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(keys, action)| {
            Line::from(vec![
                Span::styled(format!("{keys:<11}"), Style::new().bold()),
                Span::raw(*action),
            ])
        })
        .collect();
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX) + 2;
    let popup = area.centered(Constraint::Max(36), Constraint::Max(height));
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::bordered().title(" Help ")),
        popup,
    );
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `text` shortened to `width` characters by dropping its start, so a path
/// keeps its most specific components.
fn keep_end(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_owned();
    }
    let Some(kept) = width.checked_sub(1) else {
        return String::new();
    };
    let tail: String = text.chars().skip(count - kept).collect();
    format!("…{tail}")
}

/// `text` shortened to `width` characters by dropping its end.
fn keep_start(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let Some(kept) = width.checked_sub(1) else {
        return String::new();
    };
    let head: String = text.chars().take(kept).collect();
    format!("{head}…")
}
