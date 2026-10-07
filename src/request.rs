use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT};
use reqwest::{Method, Proxy, Url};

use crate::direct;
use crate::load::{Senders, Threads};
use crate::metrics::Histogram;
use crate::response::Failure;
use crate::PepeError;

/// How long the connections of a run took to open: TCP and TLS together,
/// which is how the client opens them. Fed by `TimedConnector`, a layer
/// around the client's connector, so only the requests that opened a
/// connection count, and a keep-alive run sees it once per connection.
#[derive(Debug, Default)]
pub struct ConnectTimes {
    hist: Mutex<Histogram>,
}

impl ConnectTimes {
    pub fn record(&self, took: Duration) {
        if let Ok(mut hist) = self.hist.lock() {
            hist.record(took.as_micros() as u64);
        }
    }

    /// The distribution so far
    pub fn histogram(&self) -> Histogram {
        self.hist.lock().map(|h| h.clone()).unwrap_or_default()
    }
}

/// Times whatever service it wraps: given to reqwest as a connector layer
#[derive(Clone)]
struct TimeConnects(Arc<ConnectTimes>);

impl<S> tower_layer::Layer<S> for TimeConnects {
    type Service = TimedConnector<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TimedConnector {
            inner,
            times: self.0.clone(),
        }
    }
}

#[derive(Clone)]
pub struct TimedConnector<S> {
    inner: S,
    times: Arc<ConnectTimes>,
}

impl<S, R> tower_service::Service<R> for TimedConnector<S>
where
    S: tower_service::Service<R>,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: R) -> Self::Future {
        let start = Instant::now();
        let connecting = self.inner.call(request);
        let times = self.times.clone();
        Box::pin(async move {
            let connected = connecting.await;
            // A failed connect is counted among the errors, not here
            if connected.is_ok() {
                times.record(start.elapsed());
            }
            connected
        })
    }
}

/// Idle connections a client keeps per host. A worker's connection is idle
/// only for the moment between its requests, so a few is plenty. The cap
/// matters because whenever a request finds no idle connection the pool
/// races a new one against waiting for one to be returned, and keeps the
/// loser as a spare: without it, a run that started a thousand requests at
/// once held two thousand connections for its whole length.
///
/// A paced run (`--rate`) is the exception: its workers wait between
/// requests, so their connections are idle most of the time, and a cap
/// this low would close and reopen them in waves. It keeps up to its
/// concurrency instead (see `RequestSettings::idle_connections`).
pub const IDLE_CONNECTIONS: usize = 4;

#[derive(Debug, Clone)]
pub struct RequestSettings {
    pub timeout: u32,
    /// Accept invalid TLS certificates
    pub insecure: bool,
    pub disable_compression: bool,
    pub disable_keepalive: bool,
    pub disable_redirects: bool,
    pub proxy: Option<String>,
    pub user_agent: String,
    /// Idle connections the pool keeps per host: `IDLE_CONNECTIONS`, or
    /// the concurrency when the run is paced
    pub idle_connections: usize,
}

/// What a load shard sends with: the direct path's setup when the run can
/// use it (see `direct`), and a reqwest client for what that leaves out
pub struct Sender {
    pub direct: Option<Arc<direct::Setup>>,
    /// Built when it is first needed. A run that goes direct and meets no
    /// redirect never needs it, and is spared its connection pool, its
    /// TLS setup and its resolver.
    client: std::sync::OnceLock<reqwest::Client>,
    request: Arc<Request>,
    times: Option<Arc<ConnectTimes>>,
}

impl Sender {
    fn new(request: &Arc<Request>, times: Option<&Arc<ConnectTimes>>) -> Result<Self, PepeError> {
        let direct = direct::Setup::new(request, times).map(Arc::new);
        let sender = Self {
            client: Default::default(),
            request: request.clone(),
            times: times.cloned(),
            direct,
        };
        // With no direct path every request is reqwest's, so what is wrong
        // with the client is better said before the run than during it
        if sender.direct.is_none() {
            sender.build()?;
        }
        Ok(sender)
    }

    fn build(&self) -> Result<&reqwest::Client, PepeError> {
        if self.client.get().is_none() {
            let client = self.request.build_client_with(self.times.as_ref())?;
            let _ = self.client.set(client);
        }
        Ok(self.client.get().expect("set above"))
    }

    /// The reqwest client, built now if this is the first request for it
    pub fn client(&self) -> Result<&reqwest::Client, Failure> {
        self.build().map_err(|e| Failure {
            kind: crate::response::ErrorKind::Other,
            message: e.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    /// Parsed once here, as is the method, instead of on every request
    pub url: Url,
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
        body: Option<Vec<u8>>,
        headers: &[String],
        settings: RequestSettings,
    ) -> Result<Self, PepeError> {
        let mut header_map = HeaderMap::new();
        for header in headers {
            let (name, value) = parse_header(header).map_err(PepeError::HeaderParseError)?;
            // append, not insert: repeated headers (e.g. several Cookie) are kept
            header_map.append(name, value);
        }

        let url = Url::parse(&url).map_err(|e| PepeError::InvalidUrl(format!("{url:?}: {e}")))?;

        Ok(Self {
            url,
            method: Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET),
            body: body.map(Bytes::from),
            headers: header_map,
            settings,
        })
    }

    /// One sender per load shard (see `load::shards`), each with its own
    /// connections, all timing them into the returned `ConnectTimes`
    pub fn build_clients(
        &self,
        threads: impl Into<Threads>,
    ) -> Result<(Senders, Arc<ConnectTimes>), PepeError> {
        let threads = threads.into();
        let times = Arc::new(ConnectTimes::default());
        let request = Arc::new(self.clone());
        let list = (0..threads.most.max(1))
            .map(|_| Sender::new(&request, Some(&times)))
            .collect::<Result<_, PepeError>>()?;
        let senders = Senders {
            list,
            start: threads.start,
        };
        Ok((senders, times))
    }

    /// A sender for a single request, timing nothing
    pub fn build_client(&self) -> Result<Sender, PepeError> {
        Sender::new(&Arc::new(self.clone()), None)
    }

    fn build_client_with(
        &self,
        times: Option<&Arc<ConnectTimes>>,
    ) -> Result<reqwest::Client, PepeError> {
        let mut request_headers = self.headers.clone();
        // A User-Agent given with -H wins over the default one
        if !request_headers.contains_key(USER_AGENT) {
            request_headers.insert(
                USER_AGENT,
                self.settings
                    .user_agent
                    .parse::<HeaderValue>()
                    .map_err(|e| PepeError::HeaderParseError(e.to_string()))?,
            );
        }

        let mut client_builder = reqwest::Client::builder()
            .default_headers(request_headers)
            .timeout(std::time::Duration::from_secs(self.settings.timeout as u64))
            .pool_max_idle_per_host(self.settings.idle_connections);

        if let Some(proxy_url) = &self.settings.proxy {
            let proxy =
                Proxy::all(proxy_url).map_err(|e| PepeError::HeaderParseError(e.to_string()))?;
            client_builder = client_builder.proxy(proxy);
        }

        if self.settings.insecure {
            client_builder = client_builder.danger_accept_invalid_certs(true);
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

        if let Some(times) = times {
            client_builder = client_builder.connector_layer(TimeConnects(times.clone()));
        }

        client_builder.build().map_err(PepeError::RequestError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> RequestSettings {
        RequestSettings {
            insecure: false,
            timeout: 5,
            disable_compression: false,
            disable_keepalive: false,
            disable_redirects: false,
            proxy: None,
            user_agent: "pepe/test".into(),
            idle_connections: IDLE_CONNECTIONS,
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

    /// A stand-in for the connector: ready at once, answers after a delay
    #[derive(Clone)]
    struct Slow(Duration);

    impl tower_service::Service<&'static str> for Slow {
        type Response = &'static str;
        type Error = std::convert::Infallible;
        type Future = Pin<Box<dyn Future<Output = Result<&'static str, Self::Error>> + Send>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: &'static str) -> Self::Future {
            let delay = self.0;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                Ok("connected")
            })
        }
    }

    #[tokio::test]
    async fn connections_are_timed_through_the_layer() {
        use tower_layer::Layer;
        use tower_service::Service;
        let times = Arc::new(ConnectTimes::default());
        let mut connector = TimeConnects(times.clone()).layer(Slow(Duration::from_millis(20)));
        for _ in 0..3 {
            assert_eq!(connector.call("example.com").await.unwrap(), "connected");
        }
        let hist = times.histogram();
        assert_eq!(hist.count(), 3);
        assert!(hist.percentile(50.0) >= 20_000, "{}", hist.percentile(50.0));
    }

    #[test]
    fn keeps_repeated_headers() {
        let headers = vec!["Cookie: a=1".to_string(), "Cookie: b=2".to_string()];
        let request =
            Request::new("http://x".into(), "GET".into(), None, &headers, settings()).unwrap();
        assert_eq!(request.headers.get_all("cookie").iter().count(), 2);
    }
}
