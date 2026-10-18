//! Commands for a run `pepe slack serve` started: pause it, resume it,
//! change its concurrency, stop it. They come a line each on the run's
//! stdin (`pause U123`, `concurrency 63 U123`), so the listener can steer
//! the run it spawned the way the dashboard's keys steer one on screen.

use tokio::sync::mpsc;

use crate::load::LoadHandle;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Pause,
    Resume,
    Concurrency(usize),
    Stop,
    /// A question asked in the run's thread: the run answers there
    Ask(String),
}

/// A command and who sent it (a Slack user id, or nothing)
pub type Said = (Command, Option<String>);

/// `pause U1`, `resume`, `concurrency 63 U1`, `stop`
pub fn parse(line: &str) -> Option<Said> {
    let mut words = line.split_whitespace();
    let command = match words.next()? {
        "pause" => Command::Pause,
        "resume" => Command::Resume,
        "stop" => Command::Stop,
        "concurrency" => Command::Concurrency(words.next()?.parse().ok().filter(|n| *n > 0)?),
        // `ask U1 why are errors up`: who asks first, the question after
        "ask" => {
            let by = words.next()?.to_string();
            let question = words.collect::<Vec<_>>().join(" ");
            return Some((Command::Ask(question), Some(by)));
        }
        _ => return None,
    };
    Some((command, words.next().map(str::to_string)))
}

/// The commands for this run, when `pepe slack serve` started it; none
/// otherwise, and stdin is left alone
pub fn listen() -> Option<mpsc::UnboundedReceiver<Said>> {
    std::env::var(crate::slack::RUN_ENV).ok()?;
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::Builder::new()
        .name("pepe-control".into())
        .spawn(move || {
            let mut line = String::new();
            while std::io::stdin().read_line(&mut line).is_ok_and(|n| n > 0) {
                if let Some(said) = parse(&line) {
                    if tx.send(said).is_err() {
                        break;
                    }
                }
                line.clear();
            }
        })
        .ok()?;
    Some(rx)
}

/// The next command, or never when there is no one to send them
pub async fn next(rx: &mut Option<mpsc::UnboundedReceiver<Said>>) -> Said {
    match rx {
        Some(rx) => match rx.recv().await {
            Some(said) => said,
            None => std::future::pending().await,
        },
        None => std::future::pending().await,
    }
}

/// Apply it to the run; true when it asks the run to end
pub fn apply(said: &Said, load: &LoadHandle) -> bool {
    let (command, by) = said;
    let what = match command {
        Command::Pause => {
            load.set_paused(true);
            "paused it".to_string()
        }
        Command::Resume => {
            load.set_paused(false);
            "resumed it".to_string()
        }
        Command::Concurrency(n) => {
            let before = load.concurrency();
            let now = load.set_concurrency(*n);
            if now >= before {
                format!("raised it to {now} at once")
            } else {
                format!("lowered it to {now} at once")
            }
        }
        Command::Stop => "stopped it".to_string(),
        // Answered by the run's loop, which has the numbers
        Command::Ask(_) => return false,
    };
    crate::webhook::steered(load.is_paused(), load.concurrency(), by.as_deref(), &what);
    *command == Command::Stop
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_say_a_command_and_who() {
        assert_eq!(
            parse("pause U1\n"),
            Some((Command::Pause, Some("U1".into())))
        );
        assert_eq!(parse("resume"), Some((Command::Resume, None)));
        assert_eq!(
            parse("concurrency 63 U2"),
            Some((Command::Concurrency(63), Some("U2".into())))
        );
        assert_eq!(parse("concurrency 0"), None);
        assert_eq!(parse("concurrency many"), None);
        assert_eq!(parse("dance"), None);
        assert_eq!(
            parse("ask U3 why are errors up?\n"),
            Some((Command::Ask("why are errors up?".into()), Some("U3".into())))
        );
    }
}
