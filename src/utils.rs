use std::{num::NonZeroUsize, thread::available_parallelism};

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

/// CPU time this thread has used so far, where the platform can say
#[cfg(unix)]
pub fn thread_cpu_time() -> Option<std::time::Duration> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime only writes the timespec it's given
    let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } == 0;
    ok.then(|| std::time::Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32))
}

#[cfg(not(unix))]
pub fn thread_cpu_time() -> Option<std::time::Duration> {
    None
}

/// Resolve `host` and time it: (time until the lookup answered, time to
/// walk the addresses)
pub async fn resolve_dns(
    host: &str,
) -> Result<(std::time::Duration, std::time::Duration), PepeError> {
    let start = std::time::Instant::now();
    let addrs = tokio::net::lookup_host(format!("{}:0", host))
        .await
        .map_err(PepeError::IoError)?;
    let dns_lookup_time = start.elapsed();

    let start = std::time::Instant::now();
    let _ = addrs.collect::<Vec<_>>();
    let dns_resolution_time = start.elapsed();

    Ok((dns_lookup_time, dns_resolution_time))
}
