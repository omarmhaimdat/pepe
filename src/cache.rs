use reqwest::header::HeaderMap;

const CACHE_HEADERS: [&str; 7] = [
    "x-cache",
    "x-cache-status",
    "cf-cache-status",
    "x-cache-lookup",
    "x-cdn-cache-status",
    "x-backend-cache-status",
    "x-vercel-cache",
];

// CacheStatus is an enum that represents the status of a cache
// These values are extracted from the cache headers of a response
// The values are used to determine if a response was served from cache
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CacheStatus {
    Hit,
    Miss,
    Stale,
    Expired,
    Revalidated,
    Bypass,
    Dynamic,
    Error,
    Unknown,
}

// CacheCategory is an enum that represents the category of a cache status
// Some cache statuses are grouped into categories to simplify the analysis
#[derive(Hash, Debug, PartialEq, Eq, Clone)]
pub enum CacheCategory {
    Hit,
    Miss,
    Unknown,
}

impl CacheCategory {
    pub fn from_cache_status(status: &CacheStatus) -> CacheCategory {
        match status {
            CacheStatus::Hit | CacheStatus::Revalidated | CacheStatus::Stale => CacheCategory::Hit,
            CacheStatus::Miss
            | CacheStatus::Expired
            | CacheStatus::Bypass
            | CacheStatus::Dynamic => CacheCategory::Miss,
            CacheStatus::Error | CacheStatus::Unknown => CacheCategory::Unknown,
        }
    }
}

impl CacheStatus {
    // Parse a cache status value string into the CacheStatus enum
    pub fn from_str(status: &str) -> CacheStatus {
        match status.to_lowercase().as_str() {
            "hit" => CacheStatus::Hit,
            "miss" => CacheStatus::Miss,
            "stale" => CacheStatus::Stale,
            "expired" => CacheStatus::Expired,
            "revalidated" => CacheStatus::Revalidated,
            "bypass" => CacheStatus::Bypass,
            "dynamic" => CacheStatus::Dynamic,
            "error" => CacheStatus::Error,
            _ => CacheStatus::Unknown,
        }
    }

    pub fn _to_category(&self) -> CacheCategory {
        CacheCategory::from_cache_status(self)
    }

    /// Parse cache headers into a CacheStatus enum
    /// This function is not exhaustive and only supports a few cache headers
    pub fn parse_headers(headers: &HeaderMap) -> Option<CacheStatus> {
        for header in CACHE_HEADERS.iter() {
            if let Some(value) = headers.get(*header) {
                if let Ok(value_str) = value.to_str() {
                    return Some(CacheStatus::from_str(value_str));
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    fn headers(name: &'static str, value: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(name, HeaderValue::from_static(value));
        h
    }

    #[test]
    fn parses_known_cache_headers() {
        assert_eq!(
            CacheStatus::parse_headers(&headers("cf-cache-status", "HIT")),
            Some(CacheStatus::Hit)
        );
        assert_eq!(
            CacheStatus::parse_headers(&headers("x-vercel-cache", "stale")),
            Some(CacheStatus::Stale)
        );
        assert_eq!(
            CacheStatus::parse_headers(&headers("x-cache", "weird")),
            Some(CacheStatus::Unknown)
        );
        assert_eq!(CacheStatus::parse_headers(&HeaderMap::new()), None);
    }

    #[test]
    fn groups_statuses_into_categories() {
        assert_eq!(
            CacheCategory::from_cache_status(&CacheStatus::Revalidated),
            CacheCategory::Hit
        );
        assert_eq!(
            CacheCategory::from_cache_status(&CacheStatus::Bypass),
            CacheCategory::Miss
        );
        assert_eq!(
            CacheCategory::from_cache_status(&CacheStatus::Error),
            CacheCategory::Unknown
        );
    }
}
