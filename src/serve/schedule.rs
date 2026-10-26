//! Saved tests run on a schedule: every so often, by the server, as if a
//! token named `schedule:ID` had asked. Kept in `.pepe/serve/schedules.json`
//! so they outlast the server. A schedule that comes due while the server
//! is at `--max-runs` waits for the next time, and the audit says so.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::Server;

/// The shortest interval a schedule may have
pub const LEAST: Duration = Duration::from_secs(60);
/// How often the server looks for a schedule that is due
const LOOK_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Schedule {
    pub id: String,
    pub test: String,
    pub every_s: u64,
    /// Shared out to workers, as `POST /api/runs` takes it; none runs here
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workers: Option<Value>,
    pub by: String,
    pub next_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_skipped: Option<String>,
}

pub struct Schedules {
    path: PathBuf,
    list: Mutex<Vec<Schedule>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Schedules {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("schedules.json");
        let list = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Schedules {
            path,
            list: Mutex::new(list),
        }
    }

    fn save(&self, list: &[Schedule]) {
        let text = serde_json::to_string_pretty(list).unwrap_or_default();
        let tmp = self.path.with_extension("tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(tmp, &self.path);
        }
    }

    pub fn list(&self) -> Vec<Schedule> {
        self.list.lock().map(|l| l.clone()).unwrap_or_default()
    }

    /// Add one: its first run is one interval from now
    pub fn add(
        &self,
        test: &str,
        every: Duration,
        workers: Option<Value>,
        by: &str,
    ) -> Result<Schedule, String> {
        crate::store::validate_name(test)?;
        if every < LEAST {
            return Err(format!(
                "a schedule runs at most every {}s",
                LEAST.as_secs()
            ));
        }
        let Ok(mut list) = self.list.lock() else {
            return Err("the schedules can't be read".into());
        };
        let n = list.len() + 1;
        let mut id = format!("s{n}");
        let mut k = n;
        while list.iter().any(|s| s.id == id) {
            k += 1;
            id = format!("s{k}");
        }
        let schedule = Schedule {
            id,
            test: test.to_string(),
            every_s: every.as_secs(),
            workers,
            by: by.to_string(),
            next_unix: now() + every.as_secs(),
            last_run: None,
            last_skipped: None,
        };
        list.push(schedule.clone());
        self.save(&list);
        Ok(schedule)
    }

    pub fn remove(&self, id: &str) -> bool {
        let Ok(mut list) = self.list.lock() else {
            return false;
        };
        let before = list.len();
        list.retain(|s| s.id != id);
        let removed = list.len() != before;
        if removed {
            self.save(&list);
        }
        removed
    }

    /// The schedules due now, each moved on to its next time
    fn due(&self) -> Vec<Schedule> {
        let Ok(mut list) = self.list.lock() else {
            return Vec::new();
        };
        let t = now();
        let mut due = Vec::new();
        for s in list.iter_mut() {
            if s.next_unix <= t {
                due.push(s.clone());
                // Missed times aren't owed: the next is from now
                s.next_unix = t + s.every_s.max(LEAST.as_secs());
            }
        }
        if !due.is_empty() {
            self.save(&list);
        }
        due
    }

    fn note(&self, id: &str, ran: Option<String>, skipped: Option<String>) {
        if let Ok(mut list) = self.list.lock() {
            if let Some(s) = list.iter_mut().find(|s| s.id == id) {
                if ran.is_some() {
                    s.last_run = ran;
                }
                if skipped.is_some() {
                    s.last_skipped = skipped;
                }
            }
            self.save(&list);
        }
    }
}

/// Start the schedules as they come due, for as long as the server runs
pub async fn tick(server: Arc<Server>) {
    loop {
        tokio::time::sleep(LOOK_EVERY).await;
        for schedule in server.schedules.due() {
            let who = format!("schedule:{}", schedule.id);
            let mut body = json!({ "test": schedule.test });
            if let Some(workers) = &schedule.workers {
                body["workers"] = workers.clone();
            }
            match super::api::start(&server, &who, body).await {
                Ok(view) => {
                    let id = view["id"].as_str().map(str::to_string);
                    server.schedules.note(&schedule.id, id, None);
                }
                Err((_, why)) => {
                    server.audit(&who, "schedule_skipped", json!({ "why": why }));
                    server.schedules.note(
                        &schedule.id,
                        None,
                        Some(format!("{}: {why}", crate::store::stamp(SystemTime::now()))),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_are_kept_and_come_due() {
        let dir = std::env::temp_dir().join(format!("pepe-schedules-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let schedules = Schedules::load(&dir);
        assert!(schedules
            .add("checkout", Duration::from_secs(10), None, "ci")
            .unwrap_err()
            .contains("at most every 60s"));
        assert!(schedules.add("../etc", LEAST, None, "ci").is_err());
        let s = schedules
            .add("checkout", Duration::from_secs(3600), None, "ci")
            .unwrap();
        assert_eq!(s.id, "s1");
        assert!(schedules.due().is_empty(), "the first is an hour away");
        // Due now: it comes back once, then waits its interval again
        schedules.list.lock().unwrap()[0].next_unix = 0;
        assert_eq!(schedules.due().len(), 1);
        assert!(schedules.due().is_empty());
        // Kept on disk
        assert_eq!(Schedules::load(&dir).list().len(), 1);
        assert!(schedules.remove("s1"));
        assert!(!schedules.remove("s1"));
        assert!(Schedules::load(&dir).list().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
