use std::collections::HashMap;

use reqwest::{header::USER_AGENT, Proxy};

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
    pub method: String,
    pub body: Option<String>,
    pub headers: HashMap<String, String>,
    pub settings: RequestSettings,
}

impl Request {
    pub fn new(
        url: String,
        method: String,
        body: Option<String>,
        headers: Vec<String>,
        settings: RequestSettings,
    ) -> Self {
        let mut header_map = HashMap::new();
        for header in headers {
            let parts: Vec<&str> = header.splitn(2, ':').collect();
            if parts.len() == 2 {
                header_map.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
            }
        }

        Self {
            url,
            method,
            body,
            headers: header_map,
            settings,
        }
    }

    pub fn method(&self) -> reqwest::Method {
        reqwest::Method::from_bytes(self.method.as_bytes()).unwrap_or(reqwest::Method::GET)
    }

    fn parse_headers(headers: &HashMap<String, String>) -> reqwest::header::HeaderMap {
        let mut request_headers = reqwest::header::HeaderMap::new();
        for (name, value) in headers {
            request_headers.insert(
                reqwest::header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                reqwest::header::HeaderValue::from_str(value).unwrap(),
            );
        }
        request_headers
    }

    pub fn build_client(&self) -> Result<reqwest::Client, PepeError> {
        let mut request_headers = Self::parse_headers(&self.headers);

        // Add user agent
        request_headers.insert(
            USER_AGENT,
            self.settings
                .user_agent
                .parse::<reqwest::header::HeaderValue>()
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
            client_builder = client_builder.connection_verbose(true);
        }

        if self.settings.disable_redirects {
            client_builder = client_builder.redirect(reqwest::redirect::Policy::none());
        }

        client_builder =
            client_builder.timeout(std::time::Duration::from_secs(self.settings.timeout as u64));

        let client: reqwest::Client = client_builder
            .build()
            .map_err(|e| PepeError::RequestError(e))?;

        Ok(client)
    }
}
