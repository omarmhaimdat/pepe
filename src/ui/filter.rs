//! Filters for the request log: status class, slowness and text search

use crate::response::ResponseStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    All,
    Success,
    Redirect,
    ClientError,
    ServerError,
    /// No response at all: timeouts, connection errors
    NoResponse,
    /// Anything that isn't a 2xx
    Failed,
}

impl Status {
    pub const ALL: [Status; 7] = [
        Status::All,
        Status::Success,
        Status::Redirect,
        Status::ClientError,
        Status::ServerError,
        Status::NoResponse,
        Status::Failed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Status::All => "all",
            Status::Success => "2xx",
            Status::Redirect => "3xx",
            Status::ClientError => "4xx",
            Status::ServerError => "5xx",
            Status::NoResponse => "no response",
            Status::Failed => "failed",
        }
    }

    pub fn next(self) -> Status {
        let i = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn matches(self, stat: &ResponseStats) -> bool {
        let code = stat.status_code.map(|c| c.as_u16());
        match self {
            Status::All => true,
            Status::Success => matches!(code, Some(200..=299)),
            Status::Redirect => matches!(code, Some(300..=399)),
            Status::ClientError => matches!(code, Some(400..=499)),
            Status::ServerError => matches!(code, Some(500..=599)),
            Status::NoResponse => code.is_none(),
            Status::Failed => !matches!(code, Some(200..=299)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Slow {
    #[default]
    Any,
    P50,
    P90,
    P99,
}

impl Slow {
    pub const ALL: [Slow; 4] = [Slow::Any, Slow::P50, Slow::P90, Slow::P99];

    pub fn label(self) -> &'static str {
        match self {
            Slow::Any => "any",
            Slow::P50 => "≥p50",
            Slow::P90 => "≥p90",
            Slow::P99 => "≥p99",
        }
    }

    /// Percentile a request must reach to pass
    pub fn quantile(self) -> Option<f64> {
        match self {
            Slow::Any => None,
            Slow::P50 => Some(50.0),
            Slow::P90 => Some(90.0),
            Slow::P99 => Some(99.0),
        }
    }

    pub fn next(self) -> Slow {
        let i = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub status: Status,
    pub slow: Slow,
    /// Case-insensitive text to find in the status or the response preview
    pub query: String,
    /// The search box has focus
    pub editing: bool,
}

impl Filter {
    pub fn is_active(&self) -> bool {
        self.status != Status::All || self.slow != Slow::Any || !self.query.is_empty()
    }

    pub fn clear(&mut self) {
        *self = Filter::default();
    }

    /// `min_us`: latency the slow filter requires, worked out by the caller
    pub fn matches(&self, stat: &ResponseStats, min_us: u64) -> bool {
        if !self.status.matches(stat) {
            return false;
        }
        if self.slow != Slow::Any && (stat.duration.as_micros() as u64) < min_us {
            return false;
        }
        if self.query.is_empty() {
            return true;
        }
        let query = self.query.to_lowercase();
        let status = match (stat.status_code, stat.error) {
            (Some(code), _) => code.as_u16().to_string(),
            (None, Some(e)) => e.label().to_lowercase(),
            (None, None) => "error".into(),
        };
        status.contains(&query)
            || stat
                .preview_text()
                .is_some_and(|p| p.to_lowercase().contains(&query))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::ErrorKind;
    use reqwest::StatusCode;
    use std::time::Duration;

    fn stat(status: Option<u16>, ms: u64, body: &'static [u8]) -> ResponseStats {
        ResponseStats {
            duration: Duration::from_millis(ms),
            status_code: status.map(|s| StatusCode::from_u16(s).unwrap()),
            error: status.is_none().then_some(ErrorKind::Timeout),
            preview: Some(bytes::Bytes::from_static(body)),
            ..Default::default()
        }
    }

    #[test]
    fn status_filters_split_by_class() {
        let ok = stat(Some(200), 1, b"");
        let moved = stat(Some(301), 1, b"");
        let missing = stat(Some(404), 1, b"");
        let broken = stat(Some(503), 1, b"");
        let timeout = stat(None, 1, b"");
        let f = |status| Filter {
            status,
            ..Default::default()
        };
        let pass = |status, s: &ResponseStats| f(status).matches(s, 0);
        assert!(pass(Status::All, &timeout));
        assert!(pass(Status::Success, &ok) && !pass(Status::Success, &moved));
        assert!(pass(Status::Redirect, &moved));
        assert!(pass(Status::ClientError, &missing) && !pass(Status::ClientError, &broken));
        assert!(pass(Status::ServerError, &broken));
        assert!(pass(Status::NoResponse, &timeout) && !pass(Status::NoResponse, &broken));
        assert!(pass(Status::Failed, &moved) && pass(Status::Failed, &timeout));
        assert!(!pass(Status::Failed, &ok));
    }

    #[test]
    fn slow_filter_uses_the_given_threshold() {
        let f = Filter {
            slow: Slow::P99,
            ..Default::default()
        };
        assert!(f.matches(&stat(Some(200), 50, b""), 40_000));
        assert!(!f.matches(&stat(Some(200), 10, b""), 40_000));
        // "any" ignores the threshold
        assert!(Filter::default().matches(&stat(Some(200), 10, b""), 40_000));
    }

    #[test]
    fn search_looks_at_status_and_body() {
        let f = |q: &str| Filter {
            query: q.into(),
            ..Default::default()
        };
        let s = stat(Some(503), 1, b"{\"error\":\"Upstream Timeout\"}");
        assert!(f("503").matches(&s, 0));
        assert!(f("upstream").matches(&s, 0));
        assert!(!f("200").matches(&s, 0));
        assert!(f("timeout").matches(&stat(None, 1, b""), 0));
    }

    #[test]
    fn cycling_wraps_and_clear_resets() {
        assert_eq!(Status::Failed.next(), Status::All);
        assert_eq!(Slow::P99.next(), Slow::Any);
        let mut f = Filter {
            status: Status::Failed,
            query: "x".into(),
            ..Default::default()
        };
        assert!(f.is_active());
        f.clear();
        assert!(!f.is_active());
    }
}
