//! The terminal in dashboard mode, and getting it back: raw mode, the
//! alternate screen, Ctrl-C as a signal, and a way out when the dashboard
//! is stuck writing to a terminal that stopped reading.

use std::io::stdout;

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

pub(crate) fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen, Show);
}

/// Puts the terminal into dashboard mode and restores it when dropped, so
/// every exit path (including `?` errors) leaves a usable shell behind
pub(crate) struct TerminalGuard;

impl TerminalGuard {
    pub(crate) fn enter() -> std::io::Result<Self> {
        enable_raw_mode()?;
        #[cfg(unix)]
        keep_ctrl_c_a_signal();
        // Bracketed paste: a pasted curl command arrives whole, not as keys
        execute!(stdout(), EnterAlternateScreen, Hide, EnableBracketedPaste)?;
        Ok(Self)
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
    use std::io::Write;
    use std::time::Duration;

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
