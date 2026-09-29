use clap::{ArgAction::HelpLong, Error, Parser, Subcommand};
use reqwest::Proxy;

use crate::request::{parse_header, Request, RequestSettings};
use crate::utils::{default_user_agent, num_of_cores, version};
use crate::PepeError;

#[derive(Parser, Debug, Clone)]
#[command(name = "pepe")]
#[command(version = version())]
#[command(author = "Omar MHAIMDAT")]
#[command(about = "HTTP load generator")]
#[clap(disable_help_flag = true)]
#[command(arg_required_else_help = true)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(short, long, action = HelpLong)]
    pub help: Option<bool>,

    /// Number of requests to perform
    #[arg(short, long, default_value_t = 100)]
    pub number: u32,

    /// Number of concurrent requests at a time
    #[arg(short, long, default_value_t = num_of_cores())]
    pub concurrency: u32,

    /// Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)
    #[arg(short = 'z', long)]
    pub duration: Option<String>,

    /// Curl mode to parse curl command, e.g. pepe --curl -- 'curl -X POST http://localhost:8080'
    #[arg(long)]
    pub curl: bool,

    /// HTTP method, e.g. GET, POST, PUT, DELETE
    #[arg(short, long, default_value_t = String::from("GET"))]
    pub method: String,

    /// HTTP headers, e.g. -H 'Accept: application/json'
    #[arg(short = 'H', long)]
    pub headers: Vec<String>,

    /// Time in seconds to wait for a response
    #[arg(short, long, default_value_t = 20)]
    pub timeout: u32,

    /// HTTP request body
    #[arg(short = 'd', long)]
    pub body: Option<String>,

    /// User-Agent string, default is pepe/{version}
    #[arg(short, long, default_value_t = default_user_agent())]
    pub user_agent: String,

    /// Proxy server URL: http://user:pass@host:port or socks5://host:port
    #[arg(short, long)]
    pub proxy: Option<String>,

    /// Disable HTTP compression, e.g. gzip
    #[arg(long)]
    pub disable_compression: bool,

    /// Disable HTTP keepalive, e.g. Connection: close
    #[arg(long)]
    pub disable_keepalive: bool,

    /// Prevent http redirects
    #[arg(long)]
    pub disable_redirects: bool,

    /// Output results in JSON format
    #[arg(long)]
    pub json: bool,

    /// HTTP url to request
    #[arg(default_value_t = String::from(""))]
    pub url: String,

    /// List of arguments to pass to curl command
    #[arg(last = true, default_value = "")]
    pub args: Vec<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Update pepe to the latest release
    SelfUpdate,
}

impl Cli {
    pub fn validate(&mut self) -> Result<(), Error> {
        if self.concurrency > self.number && self.duration.is_none() {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!(
                    "Concurrency cannot be greater than the number of requests. -c {} -n {}",
                    self.concurrency, self.number
                ),
            ));
        }

        if !self.curl && self.url.is_empty() {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "URL is required",
            ));
        }

        if self.timeout == 0 || self.timeout > 120 {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "Timeout must be between 1 and 120 seconds",
            ));
        }

        // Validate and parse duration if provided
        if let Some(ref duration_str) = self.duration {
            Self::parse_duration(duration_str)?;
        }

        if self.curl {
            // Print the curl command
            let curl_command = self
                .args
                .iter()
                .map(|arg| {
                    if arg.contains(' ') || arg.contains('{') {
                        format!("'{}'", arg)
                    } else {
                        arg.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            // stderr, so `--json` output on stdout stays valid JSON
            eprintln!("Curl command: {}", curl_command);
            let parsed_request = curl_parser::ParsedRequest::load(&curl_command, Some(()));
            if parsed_request.is_err() {
                eprintln!("Error: {}", parsed_request.err().unwrap());
                std::process::exit(1);
            }
            self.method = parsed_request.as_ref().unwrap().method.clone().to_string();
            self.url = parsed_request.as_ref().unwrap().url.clone().to_string();
            self.headers = parsed_request
                .as_ref()
                .unwrap()
                .headers
                .clone()
                .iter()
                .map(|(k, v)| format!("{}: {}", k, String::from_utf8_lossy(v.as_bytes())))
                .collect();
            let body = parsed_request.as_ref().unwrap().body.join(" ");
            if !body.is_empty() {
                eprintln!("Body: {}", body);
                self.body = Some(body);
            }
        }

        for header in &self.headers {
            parse_header(header)
                .map_err(|e| Error::raw(clap::error::ErrorKind::ValueValidation, e))?;
        }

        let method = reqwest::Method::from_bytes(self.method.as_bytes());
        if method.is_err() {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!("Invalid method: {}", self.method),
            ));
        }

        if self.proxy.is_some() {
            if self.proxy.as_ref().unwrap().starts_with("socks4") {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    "Socks4 proxy is not supported by reqwest.",
                ));
            }
            let proxy = Proxy::all(self.proxy.as_ref().unwrap());
            if proxy.is_err() {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("Invalid proxy URL: {}", self.proxy.as_ref().unwrap()),
                ));
            }
        }
        Ok(())
    }

    /// Parse duration string like "10s", "5m", "2h" into milliseconds
    pub fn parse_duration(duration_str: &str) -> Result<u64, Error> {
        let duration_str = duration_str.trim().to_lowercase();

        let (num_str, unit) = if let Some(idx) = duration_str.find(|c: char| c.is_alphabetic()) {
            duration_str.split_at(idx)
        } else {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "Duration must have a unit (s, m, h). Examples: 10s, 5m, 2h",
            ));
        };

        let num: u64 = num_str.trim().parse().map_err(|_| {
            Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!("Invalid duration number: {}", num_str),
            )
        })?;

        let unit_ms: u64 = match unit {
            "s" | "sec" | "second" | "seconds" => 1000,
            "m" | "min" | "minute" | "minutes" => 60 * 1000,
            "h" | "hour" | "hours" => 60 * 60 * 1000,
            _ => {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("Invalid duration unit: {}. Valid units: s, m, h", unit),
                ))
            }
        };
        let milliseconds = num.checked_mul(unit_ms).ok_or_else(|| {
            Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!("Duration is too long: {}", duration_str),
            )
        })?;

        if milliseconds == 0 {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "Duration must be greater than 0",
            ));
        }

        Ok(milliseconds)
    }

    pub fn settings(&self) -> RequestSettings {
        RequestSettings {
            user_agent: self.user_agent.clone(),
            timeout: self.timeout,
            proxy: self.proxy.clone(),
            disable_compression: self.disable_compression,
            disable_keepalive: self.disable_keepalive,
            disable_redirects: self.disable_redirects,
        }
    }

    /// Test length for `--duration` runs (already validated by `validate`)
    pub fn run_duration(&self) -> Option<std::time::Duration> {
        self.duration
            .as_deref()
            .and_then(|d| Self::parse_duration(d).ok())
            .map(std::time::Duration::from_millis)
    }

    pub fn request(&self) -> Result<Request, PepeError> {
        Request::new(
            self.url.clone(),
            self.method.clone(),
            self.body.clone(),
            &self.headers,
            self.settings(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(Cli::parse_duration("10s").unwrap(), 10_000);
        assert_eq!(Cli::parse_duration(" 5 min ").unwrap(), 300_000);
        assert_eq!(Cli::parse_duration("2H").unwrap(), 7_200_000);
    }

    #[test]
    fn rejects_bad_durations() {
        for bad in ["10", "s", "0s", "-1s", "10x", "1.5s", "99999999999999999h"] {
            assert!(
                Cli::parse_duration(bad).is_err(),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn validate_rejects_invalid_header() {
        let mut cli = Cli::parse_from(["pepe", "-c", "1", "-H", "not a header", "http://x"]);
        assert!(cli.validate().is_err());
    }

    #[test]
    fn duration_mode_ignores_request_count() {
        let mut cli = Cli::parse_from(["pepe", "-z", "5s", "-n", "1", "-c", "8", "http://x"]);
        assert!(cli.validate().is_ok());
        assert_eq!(cli.run_duration(), Some(std::time::Duration::from_secs(5)));
    }
}
