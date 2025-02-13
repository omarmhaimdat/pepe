use crate::cache::CacheStatus;

#[derive(Debug, Clone)]
pub struct ResponseStats {
    pub duration: std::time::Duration,
    pub status_code: Option<reqwest::StatusCode>,
    pub content_length: Option<u64>,
    pub partial_response: Option<String>,
    pub dns_times: Option<(std::time::Duration, std::time::Duration)>,
    pub cache_status: Option<CacheStatus>,
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
                let text = resp.text().await.unwrap_or_default();
                let text = text.trim().replace("\n", " ").replace("\r", " ");
                let truncated_text = if text.len() > 100 {
                    text[..100].to_string()
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
