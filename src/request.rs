use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use reqwest::{Method, Proxy};

use crate::PepeError;

#[derive(Debug, Clone)]
pub struct RequestSettings {
    pub timeout: u32,
    pub disable_compression: bool,
    pub disable_keepalive: bool,
    pub disable_redirects: bool,
    pub proxy: Option<String>,
    pub user_agent: String,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub url: String,
    /// Parsed once here instead of on every request
    pub method: Method,
    /// Bytes clones are a refcount bump, so sending the body costs no copy
    pub body: Option<Bytes>,
    pub headers: HeaderMap,
    pub settings: RequestSettings,
}

/// Parse a `Name: value` header, as passed to `-H`
pub fn parse_header(header: &str) -> Result<(HeaderName, HeaderValue), String> {
    let (name, value) = header
        .split_once(':')
        .ok_or_else(|| format!("Invalid header {header:?}: expected 'Name: value'"))?;
    let name = HeaderName::from_bytes(name.trim().as_bytes())
        .map_err(|_| format!("Invalid header name in {header:?}"))?;
    let value = HeaderValue::from_str(value.trim())
        .map_err(|_| format!("Invalid header value in {header:?}"))?;
    Ok((name, value))
}

impl Request {
    pub fn new(
        url: String,
        method: String,
        body: Option<String>,
        headers: &[String],
        settings: RequestSettings,
    ) -> Result<Self, PepeError> {
        let mut header_map = HeaderMap::new();
        for header in headers {
            let (name, value) = parse_header(header).map_err(PepeError::HeaderParseError)?;
            // append, not insert: repeated headers (e.g. several Cookie) are kept
            header_map.append(name, value);
        }

        Ok(Self {
            url,
            method: Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET),
            body: body.map(Bytes::from),
            headers: header_map,
            settings,
        })
    }

    pub fn build_client(&self) -> Result<reqwest::Client, PepeError> {
        let mut request_headers = self.headers.clone();
        request_headers.insert(
            USER_AGENT,
            self.settings
                .user_agent
                .parse::<HeaderValue>()
                .map_err(|e| PepeError::HeaderParseError(e.to_string()))?,
        );

        let mut client_builder = reqwest::Client::builder()
            .default_headers(request_headers)
            .timeout(std::time::Duration::from_secs(self.settings.timeout as u64));

        if let Some(proxy_url) = &self.settings.proxy {
            let proxy =
                Proxy::all(proxy_url).map_err(|e| PepeError::HeaderParseError(e.to_string()))?;
            client_builder = client_builder.proxy(proxy);
        }

        if self.settings.disable_compression {
            client_builder = client_builder.no_gzip();
        }

        if self.settings.disable_keepalive {
            // No idle connections kept, so every request opens a new one
            client_builder = client_builder.pool_max_idle_per_host(0);
        }

        if self.settings.disable_redirects {
            client_builder = client_builder.redirect(reqwest::redirect::Policy::none());
        }

        client_builder.build().map_err(PepeError::RequestError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> RequestSettings {
        RequestSettings {
            timeout: 5,
            disable_compression: false,
            disable_keepalive: false,
            disable_redirects: false,
            proxy: None,
            user_agent: "pepe/test".into(),
        }
    }

    #[test]
    fn parses_header_and_trims() {
        let (name, value) = parse_header("  Accept :  application/json ").unwrap();
        assert_eq!(name, "accept");
        assert_eq!(value, "application/json");
    }

    #[test]
    fn keeps_colons_in_value() {
        let (_, value) = parse_header("Referer: http://example.com:8080/").unwrap();
        assert_eq!(value, "http://example.com:8080/");
    }

    #[test]
    fn rejects_invalid_headers() {
        assert!(parse_header("no-colon").is_err());
        assert!(parse_header("bad name: x").is_err());
        assert!(parse_header("X-Test: bad\nvalue").is_err());
    }

    #[test]
    fn keeps_repeated_headers() {
        let headers = vec!["Cookie: a=1".to_string(), "Cookie: b=2".to_string()];
        let request =
            Request::new("http://x".into(), "GET".into(), None, &headers, settings()).unwrap();
        assert_eq!(request.headers.get_all("cookie").iter().count(), 2);
    }
}
