use std::{num::NonZeroUsize, thread::available_parallelism};

use hyper::Uri;
use std::str::FromStr;

use crate::PepeError;

/// Get the number of available cores
/// If the number of cores is not available, return 8
pub fn num_of_cores() -> u32 {
    available_parallelism()
        .unwrap_or(NonZeroUsize::new(8).unwrap())
        .get() as u32
}

/// Get the version of the application
/// This is the version from Cargo.toml
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Get the default user agent string
/// This is the user agent string used by pepe by default
/// It includes the version of the application
/// e.g. pepe/0.1.0
pub fn default_user_agent() -> String {
    format!("pepe/{}", version())
}

/// Resolve the DNS for a given URL
/// This function takes a URL string and returns a tuple of two durations
/// The first duration is the time taken to lookup the DNS
/// The second duration is the time taken to resolve the DNS
/// If the DNS lookup fails, an error is returned
/// # Arguments
/// * `url` - A string slice that holds the URL
/// # Returns
/// A Result containing a tuple of two durations or an error
pub async fn resolve_dns(
    url: &str,
) -> Result<(std::time::Duration, std::time::Duration), PepeError> {
    let uri = Uri::from_str(url).map_err(|e| PepeError::UrlParseError(e))?;
    let host = uri.host().ok_or_else(|| PepeError::HostParseError)?;

    let start = std::time::Instant::now();
    let addrs = match tokio::net::lookup_host(format!("{}:0", host)).await {
        Ok(addrs) => addrs,
        Err(e) => {
            // eprintln!("DNS lookup failed for host {}: {}", host, e);
            return Err(PepeError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                e,
            )));
        }
    };
    let dns_lookup_time = start.elapsed();

    let start = std::time::Instant::now();
    let _ = addrs.collect::<Vec<_>>();
    let dns_resolution_time = start.elapsed();

    Ok((dns_lookup_time, dns_resolution_time))
}
