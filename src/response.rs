use crate::cache::CacheStatus;
use serde::{Serialize, ser::SerializeMap};

#[derive(Debug, Clone, Serialize)]
pub struct ResponseStats {
    #[serde(serialize_with = "serialize_duration")]
    pub duration: std::time::Duration,
    #[serde(serialize_with = "serialize_status_code")]
    pub status_code: Option<reqwest::StatusCode>,
    pub content_length: Option<u64>,
    pub partial_response: Option<String>,
    #[serde(serialize_with = "serialize_dns_times")]
    pub dns_times: Option<(std::time::Duration, std::time::Duration)>,
    #[serde(skip)]
    pub cache_status: Option<CacheStatus>,
}

fn serialize_duration<S>(duration: &std::time::Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_u128(duration.as_millis())
}

fn serialize_status_code<S>(status: &Option<reqwest::StatusCode>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match status {
        Some(code) => serializer.serialize_u16(code.as_u16()),
        None => serializer.serialize_none(),
    }
}

fn serialize_dns_times<S>(
    dns_times: &Option<(std::time::Duration, std::time::Duration)>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match dns_times {
        Some((lookup, resolution)) => {
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("lookup_ms", &lookup.as_millis())?;
            map.serialize_entry("resolution_ms", &resolution.as_millis())?;
            map.end()
        }
        None => serializer.serialize_none(),
    }
}

impl Default for ResponseStats {
    fn default() -> Self {
        Self {
            duration: std::time::Duration::default(),
            status_code: None,
            content_length: None,
            partial_response: None,
            dns_times: None,
            cache_status: None,
        }
    }
}

impl ResponseStats {
    pub async fn from_response(
        resp: Result<reqwest::Response, reqwest::Error>,
        start: std::time::Instant,
        dns_times: (std::time::Duration, std::time::Duration),
    ) -> Self {
        let response_headers = resp
            .as_ref()
            .map(|r| r.headers().clone())
            .unwrap_or_default();

        let cache_status = CacheStatus::parse_headers(&response_headers);
        let stats = match resp {
            Ok(resp) => {
                let status_code = resp.status();
                let content_length = resp.content_length();
                let text = resp.text().await.unwrap_or_else(|_| "".to_string());
                let text = text.trim().replace("\n", " ").replace("\r", " ");
                let truncated_text = if text.len() > 100 {
                    text.chars().take(100).collect::<String>()
                } else {
                    text
                };

                ResponseStats {
                    duration: start.elapsed(),
                    status_code: Some(status_code),
                    content_length,
                    partial_response: Some(truncated_text),
                    dns_times: None,
                    cache_status,
                }
            }
            Err(e) => {
                // Capture timeout errors
                let status_code = e.status();
                let content_length = None;
                let partial_response = None;
                ResponseStats {
                    duration: start.elapsed(),
                    status_code,
                    content_length,
                    partial_response,
                    dns_times: Some(dns_times),
                    cache_status,
                }
            }
        };

        stats
    }
}
