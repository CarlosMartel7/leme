//! A disposable pseudo-terminal for end-to-end tests of full-screen commands.
//!
//! # Harness
//!
//! Tests open a pseudo-terminal pair with `openpty(3)` from the C library
//! (through the `libc` crate, already locked as a dependency of Crossterm)
//! and start the command with the PTY's secondary side as its controlling
//! terminal and its standard input, output and error. The test plays the
//! terminal emulator on the primary side: it sets the window size, sends
//! keys, reads the output and inspects the terminal modes.
//!
//! The test keeps its own handle on the secondary side, standing in for the
//! shell the command returns to. After the command exits, the terminal modes
//! it left behind can be inspected, and a typed line can be read back exactly
//! as a shell would read it.
//!
//! This targets Linux, including WSL. `script(1)` was rejected because it
//! cannot set the window size, and the checks would depend on parsing its
//! output. A PTY crate was rejected because it would add a dependency tree
//! for about a hundred lines of system calls.
//!
//! # Screen model
//!
//! Output is applied to a [`Screen`] that understands the subset of terminal
//! sequences Ratatui's Crossterm backend writes: cursor positioning, clearing
//! and printable text. Colours and modes are ignored. Expectations are checked
//! as output is applied, at each cursor jump, so a frame that is quickly
//! replaced, such as a loading message, is still seen. Each expectation
//! continues from where the previous one matched.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::ptr;
use std::time::{Duration, Instant};

/// How long to wait for expected output or an exit before failing.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// A process running in its own pseudo-terminal.
pub struct Pty {
    primary: File,
    /// The secondary side, kept open so the terminal survives the process.
    secondary: File,
    child: Child,
    output: Vec<u8>,
    /// How much of `output` has been applied to `screen`.
    applied: usize,
    screen: Screen,
}

/// The terminal mode flags a full-screen program changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Modes {
    pub input: libc::tcflag_t,
    pub output: libc::tcflag_t,
    pub local: libc::tcflag_t,
}

impl Modes {
    /// Whether input is read line by line with echo, as a shell expects.
    pub fn is_canonical(&self) -> bool {
        self.local & libc::ICANON != 0 && self.local & libc::ECHO != 0
    }
}

impl Pty {
    /// Starts `command` in a new `columns` x `rows` pseudo-terminal.
    pub fn spawn(mut command: Command, columns: u16, rows: u16) -> Self {
        let (primary, secondary) = open_pair(columns, rows);
        command
            .stdin(Stdio::from(duplicate(&secondary)))
            .stdout(Stdio::from(duplicate(&secondary)))
            .stderr(Stdio::from(duplicate(&secondary)));
        // SAFETY: the closure only calls async-signal-safe functions.
        unsafe {
            command.pre_exec(|| {
                // A new session whose controlling terminal is the PTY, so
                // the process sees it on stdin and as /dev/tty.
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().expect("start the process in a PTY");
        // Close this side's copies of the secondary held by the command.
        drop(command);
        Self {
            primary: File::from(primary),
            secondary: File::from(secondary),
            child,
            output: Vec::new(),
            applied: 0,
            screen: Screen::new(usize::from(columns), usize::from(rows)),
        }
    }

    /// Sends `bytes` as if typed on the keyboard.
    pub fn send(&mut self, bytes: &[u8]) {
        self.primary.write_all(bytes).expect("write to the PTY");
    }

    /// Waits until the screen shows `text`, applying output only up to the
    /// point where it first appears.
    pub fn expect(&mut self, text: &str) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if self.apply_until(text) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {text:?}; the screen shows:\n{}",
                self.screen.text()
            );
            self.read_for(Duration::from_millis(50));
        }
    }

    /// The screen after applying all output read so far.
    pub fn screen(&mut self) -> String {
        while let Some(consumed) = self.screen.apply_step(&self.output[self.applied..]) {
            self.applied += consumed;
        }
        self.screen.text()
    }

    /// The modes currently set on the terminal.
    pub fn modes(&self) -> Modes {
        // SAFETY: termios is plain data and is filled in by tcgetattr.
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: the descriptor is open for the life of `self`.
        let result = unsafe { libc::tcgetattr(self.secondary.as_raw_fd(), &mut termios) };
        assert_eq!(result, 0, "tcgetattr: {}", io::Error::last_os_error());
        Modes {
            input: termios.c_iflag,
            output: termios.c_oflag,
            local: termios.c_lflag,
        }
    }

    /// Waits for the process to exit, collecting its remaining output.
    pub fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            self.read_for(Duration::from_millis(20));
            if let Some(status) = self.child.try_wait().expect("check the process") {
                self.read_for(Duration::from_millis(50));
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "the process did not exit; the screen shows:\n{}",
                self.screen()
            );
        }
    }

    /// Types `line` and Enter, then returns what a program reading the
    /// terminal receives within a short wait.
    ///
    /// In canonical mode that is the line with a newline; in raw mode it is
    /// whatever bytes arrived, with Enter still a carriage return.
    pub fn type_line(&mut self, line: &str) -> String {
        self.send(format!("{line}\r").as_bytes());
        let mut received = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !received.ends_with(b"\n") && Instant::now() < deadline {
            if readable(self.secondary.as_raw_fd(), Duration::from_millis(50)) {
                let mut buffer = [0; 256];
                let count = self.secondary.read(&mut buffer).expect("read the terminal");
                received.extend_from_slice(&buffer[..count]);
            }
        }
        String::from_utf8_lossy(&received).into_owned()
    }

    /// All output so far, as raw bytes.
    pub fn raw_output(&self) -> &[u8] {
        &self.output
    }

    /// Applies unapplied output until the screen shows `text`, checking at
    /// each cursor jump and at the end of the available output.
    fn apply_until(&mut self, text: &str) -> bool {
        loop {
            if self.screen.text().contains(text) {
                return true;
            }
            match self.screen.apply_step(&self.output[self.applied..]) {
                Some(consumed) => self.applied += consumed,
                None => return false,
            }
        }
    }

    fn read_for(&mut self, timeout: Duration) {
        if !readable(self.primary.as_raw_fd(), timeout) {
            return;
        }
        let mut buffer = [0; 65536];
        match self.primary.read(&mut buffer) {
            Ok(count) => self.output.extend_from_slice(&buffer[..count]),
            // Linux reports EIO once no process has the terminal open.
            Err(error) if error.raw_os_error() == Some(libc::EIO) => {}
            Err(error) => panic!("read from the PTY: {error}"),
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// A grid of characters updated by terminal output.
pub struct Screen {
    columns: usize,
    rows: usize,
    cells: Vec<char>,
    row: usize,
    column: usize,
}

impl Screen {
    pub fn new(columns: usize, rows: usize) -> Self {
        Self {
            columns,
            rows,
            cells: vec![' '; columns * rows],
            row: 0,
            column: 0,
        }
    }

    /// The screen's rows, joined by newlines, without trailing spaces.
    pub fn text(&self) -> String {
        self.cells
            .chunks(self.columns)
            .map(|row| row.iter().collect::<String>().trim_end().to_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Applies `bytes` up to and including the next cursor jump, returning
    /// how many bytes were used, or `None` when `bytes` holds no complete
    /// sequence or character.
    pub fn apply_step(&mut self, bytes: &[u8]) -> Option<usize> {
        let mut index = 0;
        loop {
            let Some(&byte) = bytes.get(index) else {
                return (index > 0).then_some(index);
            };
            if byte == 0x1b {
                let Some((length, jumped)) = self.escape(&bytes[index..]) else {
                    return (index > 0).then_some(index);
                };
                index += length;
                if jumped {
                    return Some(index);
                }
                continue;
            }
            if byte < 0x20 {
                self.control(byte);
                index += 1;
                continue;
            }
            let length = utf8_length(byte);
            let Some(encoded) = bytes.get(index..index + length) else {
                return (index > 0).then_some(index);
            };
            let character = std::str::from_utf8(encoded)
                .ok()
                .and_then(|text| text.chars().next())
                .unwrap_or('\u{fffd}');
            self.put(character);
            index += length;
        }
    }

    /// Applies one escape sequence, returning its length and whether it
    /// moved the cursor to an absolute position.
    fn escape(&mut self, bytes: &[u8]) -> Option<(usize, bool)> {
        match bytes.get(1)? {
            b'[' => {
                let end = bytes[2..]
                    .iter()
                    .position(|byte| (0x40..=0x7e).contains(byte))?
                    + 2;
                let parameters = std::str::from_utf8(&bytes[2..end]).unwrap_or("");
                let jumped = self.csi(parameters, bytes[end]);
                Some((end + 1, jumped))
            }
            b']' => {
                let end = bytes[2..]
                    .iter()
                    .position(|&byte| byte == 0x07 || byte == 0x1b)?
                    + 2;
                let length = if bytes[end] == 0x1b { end + 2 } else { end + 1 };
                (length <= bytes.len()).then_some((length, false))
            }
            _ => Some((2, false)),
        }
    }

    fn csi(&mut self, parameters: &str, command: u8) -> bool {
        if let Some(private) = parameters.strip_prefix('?') {
            // Entering or leaving the alternate screen starts a blank grid.
            if private == "1049" {
                self.clear(0, self.cells.len());
            }
            return false;
        }
        let numbers: Vec<usize> = parameters
            .split(';')
            .map(|number| number.parse().unwrap_or(0))
            .collect();
        let first = numbers.first().copied().unwrap_or(0);
        let count = first.max(1);
        let here = self.row * self.columns + self.column;
        match command {
            b'H' | b'f' => {
                self.row = count.min(self.rows) - 1;
                self.column = numbers
                    .get(1)
                    .copied()
                    .unwrap_or(0)
                    .max(1)
                    .min(self.columns)
                    - 1;
                return true;
            }
            b'A' => self.row = self.row.saturating_sub(count),
            b'B' => self.row = (self.row + count).min(self.rows - 1),
            b'C' => self.column = (self.column + count).min(self.columns - 1),
            b'D' => self.column = self.column.saturating_sub(count),
            b'G' => self.column = count.min(self.columns) - 1,
            b'J' => match first {
                0 => self.clear(here, self.cells.len()),
                1 => self.clear(0, here + 1),
                _ => self.clear(0, self.cells.len()),
            },
            b'K' => {
                let start = self.row * self.columns;
                match first {
                    0 => self.clear(here, start + self.columns),
                    1 => self.clear(start, here + 1),
                    _ => self.clear(start, start + self.columns),
                }
            }
            _ => {}
        }
        false
    }

    fn control(&mut self, byte: u8) {
        match byte {
            b'\r' => self.column = 0,
            b'\n' => self.row = (self.row + 1).min(self.rows - 1),
            0x08 => self.column = self.column.saturating_sub(1),
            _ => {}
        }
    }

    fn put(&mut self, character: char) {
        if self.column < self.columns {
            self.cells[self.row * self.columns + self.column] = character;
        }
        self.column += 1;
    }

    fn clear(&mut self, start: usize, end: usize) {
        let end = end.min(self.cells.len());
        if start < end {
            self.cells[start..end].fill(' ');
        }
    }
}

fn utf8_length(first: u8) -> usize {
    match first {
        0xf0.. => 4,
        0xe0.. => 3,
        0xc0.. => 2,
        _ => 1,
    }
}

fn open_pair(columns: u16, rows: u16) -> (OwnedFd, OwnedFd) {
    let size = libc::winsize {
        ws_row: rows,
        ws_col: columns,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let (mut primary, mut secondary): (RawFd, RawFd) = (-1, -1);
    // SAFETY: openpty writes two descriptors and reads the window size.
    let result = unsafe {
        libc::openpty(
            &mut primary,
            &mut secondary,
            ptr::null_mut(),
            ptr::null(),
            &size,
        )
    };
    assert_eq!(result, 0, "openpty: {}", io::Error::last_os_error());
    // SAFETY: openpty succeeded, so both descriptors are open and owned here.
    let pair = unsafe {
        (
            OwnedFd::from_raw_fd(primary),
            OwnedFd::from_raw_fd(secondary),
        )
    };
    for fd in [&pair.0, &pair.1] {
        // The process must not inherit the primary side, or its exit would
        // go unnoticed; it receives the secondary only as stdio.
        // SAFETY: setting a flag on a descriptor owned here.
        let result = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
        assert_eq!(result, 0, "fcntl: {}", io::Error::last_os_error());
    }
    pair
}

fn duplicate(fd: &OwnedFd) -> OwnedFd {
    fd.try_clone().expect("duplicate the PTY descriptor")
}

fn readable(fd: RawFd, timeout: Duration) -> bool {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = libc::c_int::try_from(timeout.as_millis()).unwrap_or(libc::c_int::MAX);
    // SAFETY: polling one valid descriptor.
    let result = unsafe { libc::poll(&mut poll, 1, millis) };
    result > 0 && poll.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0
}
