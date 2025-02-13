use clap::{ArgAction::HelpLong, Error, Parser};
use curl_parser;
use reqwest::{header::USER_AGENT, Proxy};

use crate::utils::{default_user_agent, num_of_cores, version};
use crate::PepeError;

#[derive(Parser, Debug, Clone)]
#[command(name = "pepe")]
#[command(version = version())]
#[command(about = "HTTP load generator")]
#[clap(disable_help_flag = true)]
pub struct Cli {
    #[arg(short, long, action = HelpLong)]
    pub help: Option<bool>,

    /// Number of requests to perform
    #[arg(short, long, default_value_t = 100)]
    pub number: u32,

    /// Number of concurrent requests at a time
    #[arg(short, long, default_value_t = num_of_cores())]
    pub concurrency: u32,

    // TODO: Implement duration
    /// Duration of the test, e.g. 10s, 3m, 2h
    #[arg(short = 'z', long)]
    pub duration: Option<String>,

    /// Curl mode to parse curl command, e.g. pepe --curl -- 'curl -X POST http://localhost:8080'
    #[arg(short, long)]
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

    /// HTTP url to request
    #[arg(default_value_t = String::from(""))]
    pub url: String,

    /// List of arguments to pass to curl command
    #[arg(last = true, default_value = "")]
    pub args: Vec<String>,
}

impl Cli {
    pub fn validate(&mut self) -> Result<(), Error> {
        if self.concurrency > self.number {
            eprintln!(
                "Error: Number of workers cannot be smaller than the number of requests. -c {} -n {}",
                self.concurrency, self.number
            );
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!(
                    "Number of workers cannot be smaller than the number of requests. -c {} -n {}",
                    self.concurrency, self.number
                ),
            ));
        }

        if self.curl == false && self.url.is_empty() {
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
                .map(|(k, v)| format!("{}: {}", k, v.to_str().unwrap()))
                .collect();
            self.body = Some(parsed_request.as_ref().unwrap().body.clone().join(" "));
            // print body
            if let Some(body) = &self.body {
                println!("Body: {}", body);
            }
        }

        let method = reqwest::Method::from_bytes(self.method.as_bytes());
        if !method.is_ok() {
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

    pub fn build_client(&self) -> Result<reqwest::Client, PepeError> {
        let mut request_headers = reqwest::header::HeaderMap::new();

        // Add user agent
        request_headers.insert(
            USER_AGENT,
            self.user_agent
                .parse::<reqwest::header::HeaderValue>()
                .map_err(|e| PepeError::HeaderParseError(e.to_string()))?,
        );

        // Parse headers safely
        for header in &self.headers {
            let parts: Vec<&str> = header.splitn(2, ':').collect();
            if parts.len() == 2 {
                let name = reqwest::header::HeaderName::from_bytes(parts[0].trim().as_bytes())
                    .map_err(|e| PepeError::HeaderParseError(e.to_string()))?;
                let value = reqwest::header::HeaderValue::from_str(parts[1].trim())
                    .map_err(|e| PepeError::HeaderParseError(e.to_string()))?;
                request_headers.insert(name, value);
            }
        }

        let mut client_builder = reqwest::Client::builder()
            .default_headers(request_headers)
            .timeout(std::time::Duration::from_secs(self.timeout as u64));

        if let Some(proxy_url) = &self.proxy {
            let proxy =
                Proxy::all(proxy_url).map_err(|e| PepeError::HeaderParseError(e.to_string()))?;
            client_builder = client_builder.proxy(proxy);
        }

        if self.disable_compression {
            client_builder = client_builder.no_gzip();
        }

        if self.disable_keepalive {
            client_builder = client_builder.connection_verbose(true);
        }

        if self.disable_redirects {
            client_builder = client_builder.redirect(reqwest::redirect::Policy::none());
        }

        client_builder =
            client_builder.timeout(std::time::Duration::from_secs(self.timeout as u64));

        let client: reqwest::Client = client_builder
            .build()
            .map_err(|e| PepeError::RequestError(e))?;

        Ok(client)
    }
}
