//! The terminal in dashboard mode, and getting it back: raw mode, the
//! alternate screen, Ctrl-C as a signal, frames written off the screen's
//! loop so a terminal that stops reading can't stop it reading keys, and a
//! way out when the dashboard is stuck anyway.

use std::io::{stdout, Write};
use std::ops::{Deref, DerefMut};
use std::sync::{mpsc, OnceLock};
use std::time::Duration;

use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use tokio::sync::watch;

use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, Event, KeyCode, KeyEvent,
        KeyEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

pub(crate) fn restore_terminal() {
    let _ = disable_raw_mode();
    // The last frames first, so they can't land on the shell after it
    caught_up_within(Duration::from_secs(1));
    let _ = execute!(
        stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
}

/// Frames for the terminal, written in order by a thread of their own. A
/// terminal that stops reading (Ghostty while a window is dragged, or one
/// busy handing over a flood of mouse events, which waits for pepe to read
/// them) then blocks that thread, not the screen's loop: keys and the wheel
/// are still read, which is what lets such a terminal go on.
struct Writer {
    frames: mpsc::Sender<Vec<u8>>,
    /// Frames handed over and not yet written
    queued: watch::Sender<usize>,
}

static WRITER: OnceLock<Writer> = OnceLock::new();

fn writer() -> &'static Writer {
    WRITER.get_or_init(|| {
        let (frames, rx) = mpsc::channel::<Vec<u8>>();
        let (queued, _) = watch::channel(0usize);
        let done = queued.clone();
        std::thread::Builder::new()
            .name("pepe-screen".into())
            .spawn(move || {
                for frame in rx {
                    let mut out = stdout().lock();
                    let _ = out.write_all(&frame).and_then(|()| out.flush());
                    drop(out);
                    done.send_modify(|n: &mut usize| *n = n.saturating_sub(1));
                }
            })
            .expect("a thread to write the screen");
        Writer { frames, queued }
    })
}

/// Whether the terminal hasn't taken the last frame yet
pub(crate) fn behind() -> bool {
    *writer().queued.borrow() > 0
}

/// Once the terminal has taken every frame handed to it
pub(crate) async fn caught_up() {
    let mut queued = writer().queued.subscribe();
    let _ = queued.wait_for(|&n| n == 0).await;
}

/// Wait, but not past `limit`, for the frames to be written
fn caught_up_within(limit: Duration) {
    // Nothing to wait for if no frame was ever drawn
    let Some(writer) = WRITER.get() else {
        return;
    };
    let start = std::time::Instant::now();
    while *writer.queued.borrow() > 0 && start.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What ratatui writes: kept until it flushes, then handed to the writer
#[derive(Default)]
pub(crate) struct Out(Vec<u8>);

impl Write for Out {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.0.is_empty() {
            return Ok(());
        }
        let writer = writer();
        writer.queued.send_modify(|n| *n += 1);
        if writer.frames.send(std::mem::take(&mut self.0)).is_err() {
            writer.queued.send_modify(|n| *n -= 1);
            return Err(std::io::Error::other("the screen's writer stopped"));
        }
        Ok(())
    }
}

/// A screen's terminal. `draw` skips the frame while the terminal is still
/// taking the last one, and remembers it owes one: a loop waits on
/// `caught_up()` while `owes()`, then draws again.
pub(crate) struct Screen {
    terminal: Terminal<CrosstermBackend<Out>>,
    owed: bool,
}

impl Screen {
    pub(crate) fn new() -> std::io::Result<Self> {
        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(Out::default()))?,
            owed: false,
        })
    }

    pub(crate) fn draw(&mut self, render: impl FnOnce(&mut Frame)) -> std::io::Result<()> {
        if behind() {
            self.owed = true;
            return Ok(());
        }
        self.owed = false;
        self.terminal.draw(render)?;
        Ok(())
    }

    /// A frame was skipped and is still to be drawn
    pub(crate) fn owes(&self) -> bool {
        self.owed
    }
}

impl Deref for Screen {
    type Target = Terminal<CrosstermBackend<Out>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for Screen {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

/// Keys, the mouse, pastes and resizes, as each screen reads them
pub(crate) type Input =
    std::pin::Pin<Box<dyn futures::Stream<Item = std::io::Result<Event>> + Send>>;

/// How long after an Esc the rest of a split escape sequence may still come
const SPLIT_WAIT: Duration = Duration::from_millis(30);

/// The terminal's events, without the debris of a split escape sequence.
/// When a read ends just after the ESC that starts a mouse report (likely
/// while the wheel floods the terminal), crossterm takes the ESC for the
/// Esc key and the rest, `[<65;80;25M`, for typed keys: the inspector
/// closed, the digits changed tabs, and a second Esc quit. An Esc followed
/// within moments by `[` is such a sequence, and is dropped up to its end.
pub(crate) fn input() -> Input {
    whole_sequences(crossterm::event::EventStream::new())
}

fn whole_sequences(
    events: impl futures::Stream<Item = std::io::Result<Event>> + Unpin + Send + 'static,
) -> Input {
    use futures::StreamExt;
    Box::pin(futures::stream::unfold(
        (events, None::<Event>),
        |(mut events, mut held)| async move {
            loop {
                let event = match held.take() {
                    Some(event) => event,
                    None => match events.next().await? {
                        Ok(event) => event,
                        Err(e) => return Some((Err(e), (events, None))),
                    },
                };
                if !is_key(&event, KeyCode::Esc) {
                    return Some((Ok(event), (events, None)));
                }
                let next = match tokio::time::timeout(SPLIT_WAIT, events.next()).await {
                    Ok(Some(Ok(next))) => next,
                    // Nothing more: a real Esc
                    _ => return Some((Ok(event), (events, None))),
                };
                if !is_key(&next, KeyCode::Char('[')) {
                    return Some((Ok(event), (events, Some(next))));
                }
                // Parameters, up to the final byte that ends the sequence
                loop {
                    match tokio::time::timeout(SPLIT_WAIT, events.next()).await {
                        Ok(Some(Ok(Event::Key(KeyEvent {
                            code: KeyCode::Char(c),
                            ..
                        }))))
                            if c.is_ascii_digit() || matches!(c, ';' | '<' | '?') => {}
                        // The final byte, or something that isn't part of it
                        Ok(Some(Ok(Event::Key(KeyEvent {
                            code: KeyCode::Char(c),
                            ..
                        }))))
                            if ('@'..='~').contains(&c) =>
                        {
                            break
                        }
                        Ok(Some(Ok(other))) => {
                            held = Some(other);
                            break;
                        }
                        Ok(Some(Err(e))) => return Some((Err(e), (events, None))),
                        Ok(None) => return None,
                        Err(_) => break,
                    }
                }
            }
        },
    ))
}

fn is_key(event: &Event, code: KeyCode) -> bool {
    matches!(event, Event::Key(key) if key.code == code
        && key.kind == KeyEventKind::Press
        && key.modifiers.is_empty())
}

/// Puts the terminal into dashboard mode and restores it when dropped, so
/// every exit path (including `?` errors) leaves a usable shell behind
pub(crate) struct TerminalGuard;

impl TerminalGuard {
    pub(crate) fn enter() -> std::io::Result<Self> {
        enable_raw_mode()?;
        #[cfg(unix)]
        keep_ctrl_c_a_signal();
        // Bracketed paste: a pasted curl command arrives whole, not as keys.
        // The mouse: every screen takes clicks and the wheel; with it
        // captured the terminal no longer turns the wheel into arrow keys,
        // so each screen does that itself (`ui::kit::click`).
        execute!(
            stdout(),
            EnterAlternateScreen,
            Hide,
            EnableBracketedPaste,
            EnableClicks
        )?;
        Ok(Self)
    }
}

/// Clicks and the wheel, without the pointer's every move. crossterm's
/// `EnableMouseCapture` also asks for motion (1002, 1003): no screen uses
/// it, and each move was a full redraw. A terminal that hands over input
/// before it reads output then stalls writing the moves while pepe stalls
/// writing frames, and the screen freezes.
struct EnableClicks;

impl crossterm::Command for EnableClicks {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        // Press and release, then the RXVT and SGR encodings, so columns
        // past 223 still come through
        f.write_str("\x1b[?1000h\x1b[?1015h\x1b[?1006h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        crossterm::event::EnableMouseCapture.execute_winapi()
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        crossterm::event::EnableMouseCapture.is_ansi_code_supported()
    }
}

/// Raw mode turns Ctrl-C into an ordinary key, which only gets through while
/// the dashboard is reading keys. Keep it a signal instead, so it works even
/// when the dashboard is stuck (see `CtrlCWatchdog`). Ctrl-Z and Ctrl-\ stay
/// plain keys: suspending mid-dashboard would leave the terminal in raw mode.
#[cfg(unix)]
pub(crate) fn keep_ctrl_c_a_signal() {
    use std::os::fd::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();
    // SAFETY: termios is plain data, filled in by tcgetattr before use
    unsafe {
        if libc::isatty(fd) != 1 {
            return;
        }
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return;
        }
        let disabled = libc::fpathconf(fd, libc::_PC_VDISABLE);
        let disabled = if disabled < 0 {
            0
        } else {
            disabled as libc::cc_t
        };
        t.c_lflag |= libc::ISIG;
        t.c_cc[libc::VSUSP] = disabled;
        t.c_cc[libc::VQUIT] = disabled;
        libc::tcsetattr(fd, libc::TCSANOW, &t);
    }
}

/// If the dashboard hasn't quit a second after Ctrl-C (for example because
/// it's blocked writing to a terminal that stopped reading), restore the
/// terminal and exit from here instead. Disarmed when dropped.
pub(crate) struct CtrlCWatchdog(tokio::task::JoinHandle<()>);

impl CtrlCWatchdog {
    pub(crate) fn arm() -> Self {
        Self(tokio::spawn(async {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            force_exit();
        }))
    }
}

/// Exit now, even if the dashboard is blocked writing to a terminal that
/// stopped reading
pub(crate) fn force_exit() -> ! {
    let _ = disable_raw_mode();
    // Drop the frames the terminal hasn't read, so the reset below is next
    #[cfg(unix)]
    discard_pending_output();
    // Leave the alternate screen; from a thread, since the write may block
    std::thread::spawn(|| {
        let _ = std::io::stderr().write_all(b"\x1b[?1049l\x1b[?25h");
    });
    std::thread::sleep(Duration::from_millis(200));
    std::process::exit(130);
}

/// Throw away output queued for the terminal but not yet read by it
#[cfg(unix)]
pub(crate) fn discard_pending_output() {
    // SAFETY: tcflush only takes a file descriptor and a flag
    unsafe {
        libc::tcflush(libc::STDIN_FILENO, libc::TCOFLUSH);
    }
}

impl Drop for CtrlCWatchdog {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn key(code: KeyCode) -> std::io::Result<Event> {
        Ok(Event::Key(KeyEvent::from(code)))
    }

    fn chars(text: &str) -> Vec<std::io::Result<Event>> {
        text.chars().map(|c| key(KeyCode::Char(c))).collect()
    }

    async fn read(events: Vec<std::io::Result<Event>>) -> Vec<Event> {
        whole_sequences(futures::stream::iter(events))
            .map(|e| e.unwrap())
            .collect()
            .await
    }

    #[tokio::test]
    async fn a_split_mouse_report_is_dropped() {
        let mut events = vec![key(KeyCode::Down), key(KeyCode::Esc)];
        events.extend(chars("[<65;80;25M"));
        events.push(key(KeyCode::Up));
        assert_eq!(
            read(events).await,
            [
                Event::Key(KeyEvent::from(KeyCode::Down)),
                Event::Key(KeyEvent::from(KeyCode::Up))
            ]
        );
    }

    #[tokio::test]
    async fn esc_is_still_esc() {
        let events = vec![
            key(KeyCode::Esc),
            key(KeyCode::Char('q')),
            key(KeyCode::Esc),
        ];
        assert_eq!(
            read(events).await,
            [
                Event::Key(KeyEvent::from(KeyCode::Esc)),
                Event::Key(KeyEvent::from(KeyCode::Char('q'))),
                Event::Key(KeyEvent::from(KeyCode::Esc)),
            ]
        );
    }

    #[tokio::test]
    async fn what_breaks_off_a_sequence_is_kept() {
        let mut events = vec![key(KeyCode::Esc)];
        events.extend(chars("[<6"));
        events.push(Ok(Event::Resize(80, 24)));
        assert_eq!(read(events).await, [Event::Resize(80, 24)]);
    }
}
