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
        const NAMES: [(&str, CacheStatus); 8] = [
            ("hit", CacheStatus::Hit),
            ("miss", CacheStatus::Miss),
            ("stale", CacheStatus::Stale),
            ("expired", CacheStatus::Expired),
            ("revalidated", CacheStatus::Revalidated),
            ("bypass", CacheStatus::Bypass),
            ("dynamic", CacheStatus::Dynamic),
            ("error", CacheStatus::Error),
        ];
        NAMES
            .iter()
            .find(|(name, _)| status.eq_ignore_ascii_case(name))
            .map_or(CacheStatus::Unknown, |(_, found)| found.clone())
    }

    pub fn _to_category(&self) -> CacheCategory {
        CacheCategory::from_cache_status(self)
    }

    /// Parse cache headers into a CacheStatus enum
    /// This function is not exhaustive and only supports a few cache headers
    pub fn parse_headers<'h>(
        headers: impl Iterator<Item = (&'h [u8], &'h [u8])>,
    ) -> Option<CacheStatus> {
        // One pass over the response's few headers, instead of a lookup
        // for each name on the list: this runs for every response. Most
        // headers are ruled out by their length alone.
        const LENGTHS: u64 = crate::utils::lengths(&CACHE_HEADERS);
        let mut best: Option<(usize, &str)> = None;
        for (name, value) in headers {
            if LENGTHS & (1 << name.len().min(63)) == 0 {
                continue;
            }
            let Some(rank) = CACHE_HEADERS
                .iter()
                .position(|h| h.as_bytes().eq_ignore_ascii_case(name))
            else {
                continue;
            };
            if best.is_some_and(|(found, _)| found <= rank) {
                continue;
            }
            if let Ok(value) = std::str::from_utf8(value) {
                best = Some((rank, value.trim()));
            }
        }
        best.map(|(_, value)| CacheStatus::from_str(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn headers(name: &'static str, value: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(name, HeaderValue::from_static(value));
        h
    }

    #[test]
    fn parses_known_cache_headers() {
        assert_eq!(
            CacheStatus::parse_headers(crate::response::fields(&headers("cf-cache-status", "HIT"))),
            Some(CacheStatus::Hit)
        );
        assert_eq!(
            CacheStatus::parse_headers(crate::response::fields(&headers(
                "x-vercel-cache",
                "stale"
            ))),
            Some(CacheStatus::Stale)
        );
        assert_eq!(
            CacheStatus::parse_headers(crate::response::fields(&headers("x-cache", "weird"))),
            Some(CacheStatus::Unknown)
        );
        assert_eq!(CacheStatus::parse_headers(std::iter::empty()), None);
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
