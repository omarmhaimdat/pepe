//! The direct path: each worker keeps its own HTTP/1.1 connection and
//! writes its requests to it and reads the responses off it itself.
//!
//! A general client does a great deal for every request that a load
//! generator, sending the same request over and over on a connection it
//! already holds, needs done once. Through reqwest, every request cloned a
//! connector, merged the default headers, checked a connection out of a
//! pool keyed by a hash of the authority, armed a timer, crossed a channel
//! to the connection's task and back, allocated a header table, and had its
//! URL formatted to a string and parsed again for the redirect policy.
//!
//! Here the request is bytes made before the run (`wire::request`), sent
//! with one write. The response is read into the connection's one buffer
//! and parsed where it lies: its headers are ranges of that buffer and its
//! body is counted as it passes through it. Nothing is allocated for a
//! request, and a connection is one task and a buffer the size of what it
//! reads. The numbers are in bench/README.md.
//!
//! What it doesn't do goes to reqwest, as before: proxies, URLs with
//! credentials in them, and redirects (the first redirect a target answers
//! with sends that target to reqwest for the rest of the run). Flows build
//! a new request at every step and stay on reqwest too.

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, HOST, USER_AGENT};
use reqwest::{Method, StatusCode, Version};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{self, ClientConfig};
use tokio_rustls::TlsConnector;

use crate::request::{ConnectTimes, Request};
use crate::response::{root_cause, ErrorKind, Failure};
use crate::wire::{self, Body, Field};

/// How long one address family gets before the other is tried alongside
/// it, when a name resolves to both (as in RFC 8305)
const FAMILY_HEAD_START: Duration = Duration::from_millis(300);

/// The size of a read buffer, which is the most that is read at once.
/// More means fewer reads of a large body; past 128 KB the reads saved no
/// longer show (see bench/README.md).
const READ_BUFFER: usize = 128 * 1024;
/// Read buffers a thread keeps for its connections to borrow
const SPARE_BUFFERS: usize = 2;
/// What is read from the socket at once under TLS. rustls asks for 4 KB
/// at a time, which is four or five reads for one 16 KB record; from a
/// buffer that holds a whole record, it is one.
const TLS_BUFFER: usize = 32 * 1024;
/// A response whose head is longer than this isn't one
const MAX_HEAD: usize = 1024 * 1024;
/// A connection idle this long is looked at before it is written to: the
/// server may have closed it meanwhile
const IDLE: Duration = Duration::from_secs(1);

/// What every request of a run shares on the direct path
pub struct Setup {
    /// `-H` headers, `User-Agent` and `Accept`: what reqwest calls the
    /// client's default headers
    headers: HeaderMap,
    timeout: Duration,
    /// False for `--disable-keepalive`: a new connection for every request
    keepalive: bool,
    follow_redirects: bool,
    /// Accept invalid TLS certificates
    insecure: bool,
    /// Made for the first HTTPS connection; a run over plain HTTP has none
    tls: std::sync::OnceLock<TlsConnector>,
    times: Option<Arc<ConnectTimes>>,
}

impl Setup {
    /// None when the run needs something only reqwest does
    pub fn new(request: &Request, times: Option<&Arc<ConnectTimes>>) -> Option<Self> {
        let settings = &request.settings;
        if settings.proxy.is_some() || proxy_in_environment() {
            return None;
        }
        let mut headers = request.headers.clone();
        // A User-Agent given with -H wins over the default one
        if !headers.contains_key(USER_AGENT) {
            headers.insert(USER_AGENT, settings.user_agent.parse().ok()?);
        }
        if !headers.contains_key(ACCEPT) {
            headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        }
        Some(Self {
            headers,
            timeout: Duration::from_secs(settings.timeout as u64),
            keepalive: !settings.disable_keepalive,
            follow_redirects: !settings.disable_redirects,
            insecure: settings.insecure,
            tls: Default::default(),
            times: times.cloned(),
        })
    }
}

/// reqwest sends through the proxy these name; the direct path has none
fn proxy_in_environment() -> bool {
    [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// The same TLS as reqwest's: rustls with ring, the webpki roots, HTTP/1.1
fn tls_config(insecure: bool) -> ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions");
    let mut config = if insecure {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
            .with_no_client_auth()
    } else {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        builder.with_root_certificates(roots).with_no_client_auth()
    };
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    config
}

/// `-k`: any certificate will do. Signatures are still checked, so the
/// handshake is the work it would be with a certificate that verifies.
#[derive(Debug)]
struct AnyCertificate(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Where connections go: a scheme, a host and a port
#[derive(Debug, PartialEq)]
struct Origin {
    /// The name to verify the certificate against; None for plain HTTP
    tls: Option<ServerName<'static>>,
    /// Without the brackets an IPv6 address has in a URL
    host: String,
    port: u16,
}

/// One target, made ready to send: everything that is the same for each
/// of its requests is built here, once
pub struct Route {
    origin: Arc<Origin>,
    /// Which of a worker's connections this goes out on (see `Lines`)
    line: usize,
    /// The whole request, head and body, as it is written
    bytes: Bytes,
    /// A HEAD: its answer describes a body and has none
    head: bool,
    /// Sending it twice does what sending it once does, so it may go out
    /// again when a kept connection turns out to have been closed
    replayable: bool,
    /// The target answered with a redirect: it is reqwest's from then on
    redirects: AtomicBool,
}

/// The routes of a run's targets, and how many origins they go to
pub struct Routes {
    routes: Vec<Option<Route>>,
    origins: Vec<Arc<Origin>>,
}

impl Routes {
    /// Nothing goes direct
    pub fn none(targets: usize) -> Self {
        Self {
            routes: (0..targets).map(|_| None).collect(),
            origins: Vec::new(),
        }
    }

    /// A route for each target the direct path can send; `headers` are the
    /// target's own, on top of the run's
    pub fn new<'t>(
        setup: &Setup,
        targets: impl Iterator<Item = (&'t Request, &'t HeaderMap)>,
    ) -> Self {
        let mut origins: Vec<Arc<Origin>> = Vec::new();
        let routes = targets
            .map(|(request, headers)| Route::new(setup, request, headers, &mut origins))
            .collect();
        Self { routes, origins }
    }

    /// The route for target `index`, unless it has gone to reqwest
    pub fn get(&self, index: usize) -> Option<&Route> {
        self.routes[index]
            .as_ref()
            .filter(|route| !route.redirects.load(Ordering::Relaxed))
    }

    /// A worker's connections, none open yet
    pub fn lines(&self) -> Lines {
        Lines {
            connections: (0..self.origins.len()).map(|_| None).collect(),
            watch: Watch::new(),
        }
    }
}

impl Route {
    fn new(
        setup: &Setup,
        request: &Request,
        own: &HeaderMap,
        origins: &mut Vec<Arc<Origin>>,
    ) -> Option<Self> {
        let url = &request.url;
        let https = match url.scheme() {
            "http" => false,
            "https" => true,
            _ => return None,
        };
        // reqwest turns user:password@ into an Authorization header
        if !url.username().is_empty() || url.password().is_some() {
            return None;
        }
        let authority = url.host_str()?;
        let host = authority.trim_start_matches('[').trim_end_matches(']');
        let origin = Origin {
            tls: match https {
                true => Some(ServerName::try_from(host.to_string()).ok()?),
                false => None,
            },
            host: host.to_string(),
            port: url.port_or_known_default()?,
        };
        let line = origins
            .iter()
            .position(|known| **known == origin)
            .unwrap_or_else(|| {
                origins.push(Arc::new(origin));
                origins.len() - 1
            });

        // The target's own headers win over the run's, name by name
        let mut headers = own.clone();
        for name in setup.headers.keys() {
            if !own.contains_key(name) {
                for value in setup.headers.get_all(name) {
                    headers.append(name, value.clone());
                }
            }
        }
        if !headers.contains_key(HOST) {
            let value = match url.port() {
                Some(port) => format!("{authority}:{port}"),
                None => authority.to_string(),
            };
            headers.insert(HOST, value.parse().ok()?);
        }
        let mut path = url.path().to_string();
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        let body = request.body.as_deref().unwrap_or_default();
        let method = &request.method;
        Some(Self {
            origin: origins[line].clone(),
            line,
            bytes: wire::request(method, &path, &headers, body),
            head: method == Method::HEAD,
            replayable: [Method::GET, Method::HEAD, Method::OPTIONS, Method::TRACE]
                .contains(method),
            redirects: AtomicBool::new(false),
        })
    }
}

/// A connection's two kinds of stream
enum Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream<BufReader<TcpStream>>>),
}

impl Stream {
    /// Write all of `bytes`
    async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.write_all(bytes).await,
            Self::Tls(stream) => {
                stream.write_all(bytes).await?;
                // rustls holds records back until it is told to send them
                stream.flush().await
            }
        }
    }

    /// Read what has arrived into the room `buf` has left, or arrange to
    /// be woken when something has; 0 at the end of the stream
    fn poll_fill(
        &mut self,
        cx: &mut Context<'_>,
        buf: &mut Vec<u8>,
    ) -> Poll<std::io::Result<usize>> {
        match self {
            Self::Plain(stream) => {
                let reading = stream.read_buf(buf);
                tokio::pin!(reading);
                reading.poll(cx)
            }
            Self::Tls(stream) => {
                let reading = stream.read_buf(buf);
                tokio::pin!(reading);
                reading.poll(cx)
            }
        }
    }
}

thread_local! {
    /// The read buffers of this thread's connections. A connection has no
    /// buffer of its own: it borrows one when its socket has something to
    /// read and gives it back when that has been taken, which is before
    /// it waits again. A thousand connections on a thread read into the
    /// same 128 KB, so a connection costs its task and nothing else, and
    /// what they read is always in the cache.
    static SPARE: std::cell::RefCell<Vec<Vec<u8>>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn borrow_buffer() -> Vec<u8> {
    SPARE
        .with(|spare| spare.borrow_mut().pop())
        .unwrap_or_else(|| Vec::with_capacity(READ_BUFFER))
}

fn return_buffer(mut buf: Vec<u8>) {
    if buf.capacity() == 0 {
        return;
    }
    buf.clear();
    SPARE.with(|spare| {
        let mut spare = spare.borrow_mut();
        if spare.len() < SPARE_BUFFERS {
            spare.push(buf);
        }
    });
}

/// One open connection
struct Connection {
    stream: Stream,
    remote: Option<SocketAddr>,
    /// What was read and not yet taken, in a borrowed buffer; without
    /// capacity when nothing is. Of a response, the head is at its start
    /// and whatever of the body came with it follows.
    buf: Vec<u8>,
    /// How much of `buf` has been taken
    at: usize,
    /// The headers of the response being read, as ranges of `buf`
    fields: Vec<Field>,
    /// When the last request went out on it; None for a new connection
    used: Option<Instant>,
    /// The last response was read to its end and left the connection open
    reusable: bool,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.release();
    }
}

/// Why a request got no response head
enum Broken {
    /// The connection was already closed: nothing of an answer came, so
    /// the server may never have seen the request
    Closed(Failure),
    Failed(Failure),
}

impl Connection {
    /// The headers of the response read last, name and value, in order
    fn headers(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        let buf = &self.buf;
        self.fields
            .iter()
            .map(move |field| (field.name(buf), field.value(buf)))
    }

    /// Give the read buffer back, with whatever is in it
    fn release(&mut self) {
        return_buffer(std::mem::take(&mut self.buf));
        self.at = 0;
    }

    /// The server closed this connection, or sent something nobody asked
    /// for, while it was idle. Costs no system call unless there is
    /// something to read.
    fn gone(&mut self) -> bool {
        self.release();
        let mut buf = borrow_buffer();
        let mut cx = Context::from_waker(Waker::noop());
        let gone = self.stream.poll_fill(&mut cx, &mut buf).is_ready();
        return_buffer(buf);
        gone
    }

    /// Read what has arrived, after what `buf` already holds; 0 at the end
    /// of the stream. A buffer is borrowed for each attempt, and kept
    /// while waiting only if there is something in it.
    async fn fill(&mut self) -> std::io::Result<usize> {
        std::future::poll_fn(|cx| {
            if self.buf.capacity() == 0 {
                self.buf = borrow_buffer();
            }
            let read = self.stream.poll_fill(cx, &mut self.buf);
            if read.is_pending() && self.buf.is_empty() {
                self.release();
            }
            read
        })
        .await
    }

    /// Write `route`'s request and read the head of its response
    async fn exchange(&mut self, route: &Route, now: Instant) -> Result<wire::Head, Broken> {
        self.used = Some(now);
        self.reusable = false;
        self.release();
        if let Err(e) = self.stream.send(&route.bytes).await {
            return Err(Broken::Closed(Failure::other(&e)));
        }
        loop {
            // A head that fills the buffer and still isn't whole
            if self.buf.capacity() != 0 && self.buf.len() == self.buf.capacity() {
                if self.buf.len() >= MAX_HEAD {
                    return Err(Broken::Failed(Failure::said("message head is too large")));
                }
                self.buf.reserve(READ_BUFFER);
            }
            match self.fill().await {
                Ok(0) if self.buf.is_empty() => {
                    return Err(Broken::Closed(Failure::said(
                        "connection closed before message completed",
                    )))
                }
                Ok(0) => {
                    return Err(Broken::Failed(Failure::said(
                        "connection closed before message completed",
                    )))
                }
                Ok(_) => {}
                Err(e) if self.buf.is_empty() => return Err(Broken::Closed(Failure::other(&e))),
                Err(e) => return Err(Broken::Failed(Failure::other(&e))),
            }
            // More than one head may have come: any number of 1xx interim
            // responses, then the one that counts
            loop {
                match wire::head(&self.buf, route.head, &mut self.fields) {
                    Ok(None) => break,
                    Ok(Some(head)) if (100..200).contains(&head.status) && head.status != 101 => {
                        self.buf.drain(..head.len);
                    }
                    Ok(Some(head)) => {
                        self.at = head.len;
                        return Ok(head);
                    }
                    Err(why) => return Err(Broken::Failed(Failure::said(why))),
                }
            }
        }
    }
}

/// A worker's connections, one per origin, and its timeout
pub struct Lines {
    connections: Vec<Option<Connection>>,
    watch: Watch,
}

/// A response's head, with its body still to come. The headers are where
/// they were read, so they are to be looked at before the body is.
pub struct Response<'a> {
    pub status: StatusCode,
    pub version: Version,
    pub remote_addr: Option<SocketAddr>,
    body: Body,
    keep_alive: bool,
    connection: &'a mut Connection,
    watch: &'a mut Watch,
}

impl Response<'_> {
    /// Every header, name and value, in the order they came
    pub fn headers(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.connection.headers()
    }

    /// The headers as a table of their own, for a response that is kept
    pub fn header_map(&self) -> HeaderMap {
        let mut map = HeaderMap::with_capacity(self.connection.fields.len());
        for (name, value) in self.headers() {
            if let (Ok(name), Ok(value)) =
                (HeaderName::from_bytes(name), HeaderValue::from_bytes(value))
            {
                map.append(name, value);
            }
        }
        map
    }

    /// The next piece of the body, as it lies in the connection's buffer;
    /// None at its end
    pub async fn chunk(&mut self) -> Result<Option<&[u8]>, Failure> {
        let piece = loop {
            let unread = self.connection.at < self.connection.buf.len();
            let wants = match &self.body {
                Body::None => false,
                Body::Chunked(chunked) => !chunked.done(),
                Body::Length(_) | Body::UntilClose => true,
            };
            if wants && !unread && self.read().await? == 0 {
                if self.body != Body::UntilClose {
                    return Err(Failure::said("end of file before message length reached"));
                }
                // The close was the end of it
                self.body = Body::None;
                self.keep_alive = false;
            }
            let c = &mut *self.connection;
            let mut ended = false;
            let piece = match &mut self.body {
                Body::None => {
                    // Anything left over was never asked for
                    c.reusable = self.keep_alive && c.at == c.buf.len();
                    if c.reusable {
                        c.release();
                    }
                    return Ok(None);
                }
                Body::Length(left) => {
                    let run = (*left).min((c.buf.len() - c.at) as u64) as usize;
                    *left -= run as u64;
                    ended = *left == 0;
                    c.at += run;
                    c.at - run..c.at
                }
                Body::Chunked(chunked) => {
                    let step = chunked.step(&c.buf[c.at..]).map_err(Failure::said)?;
                    let from = c.at;
                    c.at += step.consumed;
                    ended = chunked.done();
                    from + step.data.start..from + step.data.end
                }
                Body::UntilClose => {
                    let from = c.at;
                    c.at = c.buf.len();
                    from..c.at
                }
            };
            if ended {
                self.body = Body::None;
            }
            if !piece.is_empty() {
                break piece;
            }
        };
        Ok(Some(&self.connection.buf[piece]))
    }

    /// Read more of the body, what was read before having been taken; 0
    /// at the end of the stream
    async fn read(&mut self) -> Result<usize, Failure> {
        let c = &mut *self.connection;
        c.release();
        match self.watch.guard(c.fill()).await {
            Ok(Ok(read)) => Ok(read),
            Ok(Err(e)) => Err(Failure::other(&e)),
            Err(TimedOut) => Err(Failure::timed_out()),
        }
    }
}

/// What became of a request sent directly
pub enum Sent<'a> {
    Answered(Response<'a>),
    /// A redirect, and the run follows them: the target is reqwest's from
    /// now on, this request included
    Redirected,
}

impl Lines {
    /// Send `route`'s request on this worker's connection to its origin,
    /// opening it first if need be. `start` is when the request began,
    /// which is when its timeout counts from.
    pub async fn send<'a>(
        &'a mut self,
        setup: &Setup,
        route: &Route,
        start: Instant,
    ) -> Result<Sent<'a>, Failure> {
        let Lines { connections, watch } = self;
        let line = &mut connections[route.line];
        watch.deadline = (start + setup.timeout).into();
        let head = match watch.guard(send_on(setup, route, line, start)).await {
            Ok(Ok(head)) => head,
            Ok(Err(failure)) => {
                *line = None;
                return Err(failure);
            }
            Err(TimedOut) => {
                *line = None;
                return Err(Failure::timed_out());
            }
        };
        let Ok(status) = StatusCode::from_u16(head.status) else {
            *line = None;
            return Err(Failure::said("invalid HTTP status-code parsed"));
        };
        let location = line.as_ref().is_some_and(|c| {
            c.headers()
                .any(|(name, _)| name.eq_ignore_ascii_case(b"location"))
        });
        // What reqwest's redirect policy follows
        if setup.follow_redirects && location && matches!(head.status, 301 | 302 | 303 | 307 | 308)
        {
            route.redirects.store(true, Ordering::Relaxed);
            // Its body is unread, so the connection can't be used again
            *line = None;
            return Ok(Sent::Redirected);
        }
        let connection = line.as_mut().expect("it answered");
        Ok(Sent::Answered(Response {
            status,
            version: match head.minor {
                0 => Version::HTTP_10,
                _ => Version::HTTP_11,
            },
            remote_addr: connection.remote,
            body: head.body,
            keep_alive: head.keep_alive,
            connection,
            watch,
        }))
    }

    /// Close every connection: the worker has nothing to send for now
    pub fn close(&mut self) {
        self.connections.iter_mut().for_each(|c| *c = None);
    }

    /// The response to `route`'s request has been read, or given up on
    pub fn done(&mut self, setup: &Setup, route: &Route, failed: bool) {
        let line = &mut self.connections[route.line];
        // Kept only if the response ended where it said it would and
        // didn't ask for the connection to be closed
        let keep = !failed && setup.keepalive && line.as_ref().is_some_and(|c| c.reusable);
        if !keep {
            *line = None;
        }
    }
}

/// Send `route`'s request on the worker's connection to its origin,
/// opening one first if there is none, and read the response's head
async fn send_on(
    setup: &Setup,
    route: &Route,
    line: &mut Option<Connection>,
    now: Instant,
) -> Result<wire::Head, Failure> {
    // A connection kept from the last request may have been closed by the
    // server since. One that sat idle is looked at first; one that turns
    // out closed when written to gets its request again on a new
    // connection, once, if the server can't have acted on it.
    let mut again = true;
    loop {
        let idle = |c: &Connection| c.used.is_some_and(|at| now.duration_since(at) >= IDLE);
        if line.as_mut().is_some_and(|c| idle(c) && c.gone()) {
            *line = None;
        }
        if line.is_none() {
            // Boxed: it happens once, and would otherwise be the largest
            // thing in every worker's future
            *line = Some(Box::pin(connect(setup, &route.origin)).await?);
        }
        let connection = line.as_mut().expect("opened above");
        let kept = connection.used.is_some();
        match connection.exchange(route, now).await {
            Ok(head) => return Ok(head),
            Err(Broken::Closed(_)) if kept && again && route.replayable => {
                again = false;
                *line = None;
            }
            Err(Broken::Closed(failure) | Broken::Failed(failure)) => return Err(failure),
        }
    }
}

async fn connect(setup: &Setup, origin: &Origin) -> Result<Connection, Failure> {
    let began = Instant::now();
    let stream = open(origin).await.map_err(|e| Failure::connect(&e))?;
    // A request is one small write: don't wait to fill a segment
    let _ = stream.set_nodelay(true);
    let remote = stream.peer_addr().ok();
    let stream = match &origin.tls {
        None => Stream::Plain(stream),
        Some(name) => Stream::Tls(Box::new(
            setup
                .tls
                .get_or_init(|| TlsConnector::from(Arc::new(tls_config(setup.insecure))))
                .connect(name.clone(), BufReader::with_capacity(TLS_BUFFER, stream))
                .await
                .map_err(|e| Failure::connect(&e))?,
        )),
    };
    if let Some(times) = &setup.times {
        times.record(began.elapsed());
    }
    Ok(Connection {
        stream,
        remote,
        buf: Vec::new(),
        at: 0,
        fields: Vec::new(),
        used: None,
        reusable: false,
    })
}

/// Resolve the origin and connect to the first address that answers
async fn open(origin: &Origin) -> std::io::Result<TcpStream> {
    if let Ok(ip) = origin.host.parse::<IpAddr>() {
        return TcpStream::connect(SocketAddr::new(ip, origin.port)).await;
    }
    let addresses: Vec<SocketAddr> = tokio::net::lookup_host((origin.host.as_str(), origin.port))
        .await?
        .collect();
    let Some(first) = addresses.first() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the name resolved to no address",
        ));
    };
    // The resolver's order says which family to prefer
    let (preferred, other): (Vec<_>, Vec<_>) = addresses
        .iter()
        .partition(|address| address.is_ipv4() == first.is_ipv4());
    if other.is_empty() {
        return first_to_connect(&preferred).await;
    }
    let first_choice = first_to_connect(&preferred);
    tokio::pin!(first_choice);
    match tokio::time::timeout(FAMILY_HEAD_START, &mut first_choice).await {
        Ok(Ok(stream)) => return Ok(stream),
        // Refused, say: no reason to wait before trying the other family
        Ok(Err(_)) => return first_to_connect(&other).await,
        Err(_still_trying) => {}
    }
    let second_choice = first_to_connect(&other);
    tokio::pin!(second_choice);
    // Whichever connects first; if one fails, the other decides
    tokio::select! {
        connected = &mut first_choice => match connected {
            Ok(stream) => Ok(stream),
            Err(_) => second_choice.await,
        },
        connected = &mut second_choice => match connected {
            Ok(stream) => Ok(stream),
            Err(_) => first_choice.await,
        },
    }
}

async fn first_to_connect(addresses: &[SocketAddr]) -> std::io::Result<TcpStream> {
    let mut last = None;
    for address in addresses {
        match TcpStream::connect(address).await {
            Ok(stream) => return Ok(stream),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no address to connect to")))
}

struct TimedOut;

/// A worker's request timeout. reqwest arms a timer for every request and
/// disarms it when the response is in; at a hundred thousand requests a
/// second that is two hundred thousand trips to the timer wheel for timers
/// that never fire. This keeps one timer per worker: a request only notes
/// its deadline, and the timer, when it fires, is moved to the deadline of
/// whichever request is waiting then.
struct Watch {
    timer: Option<Pin<Box<tokio::time::Sleep>>>,
    deadline: tokio::time::Instant,
}

impl Watch {
    fn new() -> Self {
        Self {
            timer: None,
            deadline: tokio::time::Instant::now(),
        }
    }

    /// `future`'s output, unless the deadline comes first
    async fn guard<F: Future>(&mut self, future: F) -> Result<F::Output, TimedOut> {
        tokio::pin!(future);
        std::future::poll_fn(|cx| {
            if let Poll::Ready(output) = future.as_mut().poll(cx) {
                return Poll::Ready(Ok(output));
            }
            let deadline = self.deadline;
            let timer = self
                .timer
                .get_or_insert_with(|| Box::pin(tokio::time::sleep_until(deadline)));
            // Fired for an earlier request's deadline: move it to this one's
            while timer.as_mut().poll(cx).is_ready() {
                if tokio::time::Instant::now() >= deadline {
                    return Poll::Ready(Err(TimedOut));
                }
                timer.as_mut().reset(deadline);
            }
            Poll::Pending
        })
        .await
    }
}

impl Failure {
    fn timed_out() -> Self {
        Self {
            kind: ErrorKind::Timeout,
            // reqwest's words for it
            message: "operation timed out".into(),
        }
    }

    fn connect(error: &(dyn std::error::Error + 'static)) -> Self {
        Self {
            kind: ErrorKind::Connect,
            message: root_cause(error),
        }
    }

    /// A failure in the protocol, in words of its own
    fn said(message: &'static str) -> Self {
        Self {
            kind: ErrorKind::Other,
            message: message.into(),
        }
    }

    fn other(error: &(dyn std::error::Error + 'static)) -> Self {
        Self {
            kind: ErrorKind::Other,
            message: root_cause(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::{self, Plan};
    use crate::request::RequestSettings;
    use crate::response::ResponseStats;
    use std::sync::atomic::AtomicU64;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn settings() -> RequestSettings {
        RequestSettings {
            insecure: false,
            timeout: 5,
            disable_compression: false,
            disable_keepalive: false,
            disable_redirects: false,
            proxy: None,
            user_agent: "pepe/test".into(),
            idle_connections: crate::request::IDLE_CONNECTIONS,
        }
    }

    fn request(url: &str, settings: RequestSettings) -> Request {
        Request::new(url.into(), "GET".into(), None, &[], settings).unwrap()
    }

    /// What the test server does with each request, given its head
    type Handler = fn(&str) -> Reply;

    enum Reply {
        /// Write this and wait for the next request
        Answer(&'static str),
        /// Write this, and close the connection a moment later without
        /// having said so
        AnswerThenClose(&'static str),
        /// Say nothing
        Hang,
    }

    struct Server {
        url: String,
        connections: Arc<AtomicU64>,
        requests: Arc<AtomicU64>,
    }

    /// A keep-alive HTTP/1.1 server that answers as `handler` says
    async fn server(handler: Handler) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let connections = Arc::new(AtomicU64::new(0));
        let requests = Arc::new(AtomicU64::new(0));
        let (opened, served) = (connections.clone(), requests.clone());
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                opened.fetch_add(1, Ordering::Relaxed);
                let served = served.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let mut head = Vec::new();
                    loop {
                        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                            match socket.read(&mut buf).await {
                                Ok(n) if n > 0 => head.extend_from_slice(&buf[..n]),
                                _ => return,
                            }
                        }
                        served.fetch_add(1, Ordering::Relaxed);
                        let text = String::from_utf8_lossy(&head).to_string();
                        head.clear();
                        match handler(&text) {
                            Reply::Answer(answer) => {
                                if socket.write_all(answer.as_bytes()).await.is_err() {
                                    return;
                                }
                            }
                            Reply::AnswerThenClose(answer) => {
                                let _ = socket.write_all(answer.as_bytes()).await;
                                tokio::time::sleep(Duration::from_millis(10)).await;
                                return;
                            }
                            Reply::Hang => std::future::pending::<()>().await,
                        }
                    }
                });
            }
        });
        Server {
            url,
            connections,
            requests,
        }
    }

    const OK: &str = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok";

    async fn run(request: Request, concurrency: usize, count: u64) -> Vec<ResponseStats> {
        let clients = request.build_clients(1).unwrap().0;
        assert!(clients.list[0].direct.is_some(), "the run goes direct");
        let mut load = load::start(clients, request, concurrency, Plan::Count(count), true);
        let mut out = Vec::new();
        while let Some(stat) = load.recv().await {
            out.push(stat);
        }
        out
    }

    fn statuses(results: &[ResponseStats]) -> Vec<Option<u16>> {
        results
            .iter()
            .map(|r| r.status_code.map(|s| s.as_u16()))
            .collect()
    }

    #[tokio::test]
    async fn each_worker_keeps_one_connection() {
        let srv = server(|_| Reply::Answer(OK)).await;
        let results = run(request(&srv.url, settings()), 3, 60).await;
        assert_eq!(statuses(&results), vec![Some(200); 60]);
        assert!(results.iter().all(|r| r.body_bytes == 2));
        assert_eq!(srv.connections.load(Ordering::Relaxed), 3);
        assert_eq!(srv.requests.load(Ordering::Relaxed), 60);
        // What the inspector shows comes through this path too
        let detail = results[0].detail.as_ref().expect("captured");
        assert_eq!(&detail.body[..], b"ok");
        assert_eq!(detail.headers.get("content-length").unwrap(), "2");
        assert_eq!(detail.final_url, format!("{}/", srv.url));
        assert!(detail.remote_addr.is_some());
    }

    #[tokio::test]
    async fn without_keepalive_every_request_opens_a_connection() {
        let srv = server(|_| Reply::Answer(OK)).await;
        let quiet = RequestSettings {
            disable_keepalive: true,
            ..settings()
        };
        let results = run(request(&srv.url, quiet), 2, 10).await;
        assert_eq!(statuses(&results), vec![Some(200); 10]);
        assert_eq!(srv.connections.load(Ordering::Relaxed), 10);
    }

    #[tokio::test]
    async fn a_connection_the_server_closed_is_replaced_not_failed() {
        let srv = server(|_| Reply::AnswerThenClose(OK)).await;
        let request = request(&srv.url, settings());
        let clients = request.build_clients(1).unwrap().0;
        let mut load = load::start(clients, request, 1, Plan::Count(4), false);
        // Paced, so each close has happened before the next request
        load.set_rate(Some(10.0));
        let mut results = Vec::new();
        while let Some(stat) = load.recv().await {
            results.push(stat);
        }
        assert_eq!(statuses(&results), vec![Some(200); 4], "{results:?}");
        assert_eq!(srv.connections.load(Ordering::Relaxed), 4);
    }

    #[tokio::test]
    async fn a_chunked_body_is_counted_in_full() {
        let srv = server(|_| {
            Reply::Answer(
                "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
            )
        })
        .await;
        let results = run(request(&srv.url, settings()), 1, 5).await;
        assert_eq!(statuses(&results), vec![Some(200); 5]);
        assert!(results.iter().all(|r| r.body_bytes == 11));
        assert_eq!(results[0].preview_text().as_deref(), Some("hello world"));
        assert_eq!(srv.connections.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn interim_responses_are_skipped_and_a_close_is_honoured() {
        // A 103 before the answer, and an answer that ends the connection
        let srv = server(|_| {
            Reply::Answer(
                "HTTP/1.1 103 Early Hints\r\nlink: </a.css>\r\n\r\nHTTP/1.1 200 OK\r\nconnection: close\r\ncontent-length: 2\r\n\r\nok",
            )
        })
        .await;
        let results = run(request(&srv.url, settings()), 1, 4).await;
        assert_eq!(statuses(&results), vec![Some(200); 4]);
        assert!(results.iter().all(|r| r.body_bytes == 2));
        // Told to close, the worker opened a connection for each request
        assert_eq!(srv.connections.load(Ordering::Relaxed), 4);
    }

    #[tokio::test]
    async fn a_body_that_ends_with_the_connection_is_counted_to_its_end() {
        let srv = server(|_| Reply::AnswerThenClose("HTTP/1.1 200 OK\r\n\r\nall of it")).await;
        let results = run(request(&srv.url, settings()), 1, 3).await;
        assert_eq!(statuses(&results), vec![Some(200); 3]);
        assert!(results.iter().all(|r| r.body_bytes == 9), "{results:?}");
        assert_eq!(srv.connections.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn a_body_cut_short_and_a_head_that_is_not_http_are_failures() {
        let srv = server(|_| {
            Reply::AnswerThenClose("HTTP/1.1 200 OK\r\ncontent-length: 50\r\n\r\nshort")
        })
        .await;
        let results = run(request(&srv.url, settings()), 1, 2).await;
        assert_eq!(
            results[0].error_message.as_deref(),
            Some("end of file before message length reached")
        );
        let srv = server(|_| Reply::Answer("220 smtp ready\r\n\r\n")).await;
        let results = run(request(&srv.url, settings()), 1, 2).await;
        assert_eq!(statuses(&results), vec![None; 2]);
        assert_eq!(
            results[0].error_message.as_deref(),
            Some("invalid HTTP version parsed")
        );
    }

    #[tokio::test]
    async fn a_large_body_is_read_through_a_buffer_that_grows() {
        static BIG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let big = BIG.get_or_init(|| {
            format!(
                "HTTP/1.1 200 OK\r\nx-request-id: big-1\r\ncontent-length: 300000\r\n\r\n{}",
                "x".repeat(300_000)
            )
        });
        let srv = server(|_| Reply::Answer(BIG.get().expect("set below"))).await;
        let _ = big;
        let results = run(request(&srv.url, settings()), 2, 6).await;
        assert_eq!(statuses(&results), vec![Some(200); 6]);
        assert!(results.iter().all(|r| r.body_bytes == 300_000));
        // The headers were read before the body took the buffer over
        assert!(results.iter().all(|r| r
            .request_id
            .as_ref()
            .is_some_and(|(_, id)| &**id == "big-1")));
        assert_eq!(srv.connections.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn a_head_request_reads_no_body() {
        let srv = server(|_| Reply::Answer("HTTP/1.1 200 OK\r\ncontent-length: 500\r\n\r\n")).await;
        let head = Request::new(srv.url.clone(), "HEAD".into(), None, &[], settings()).unwrap();
        let results = run(head, 1, 5).await;
        assert_eq!(statuses(&results), vec![Some(200); 5]);
        assert!(results.iter().all(|r| r.body_bytes == 0));
        assert_eq!(srv.connections.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_redirect_is_followed_by_handing_the_target_to_reqwest() {
        let srv = server(|head| {
            if head.starts_with("GET /there ") {
                Reply::Answer(OK)
            } else {
                Reply::Answer("HTTP/1.1 302 Found\r\nlocation: /there\r\ncontent-length: 0\r\n\r\n")
            }
        })
        .await;
        let results = run(request(&srv.url, settings()), 2, 12).await;
        assert_eq!(statuses(&results), vec![Some(200); 12]);

        // Told not to follow them, the redirect is the answer
        let stay = RequestSettings {
            disable_redirects: true,
            ..settings()
        };
        let results = run(request(&srv.url, stay), 2, 6).await;
        assert_eq!(statuses(&results), vec![Some(302); 6]);
    }

    #[tokio::test]
    async fn a_request_that_gets_no_answer_times_out() {
        let srv = server(|_| Reply::Hang).await;
        let brief = RequestSettings {
            timeout: 1,
            ..settings()
        };
        let began = Instant::now();
        let results = run(request(&srv.url, brief), 2, 2).await;
        assert!(began.elapsed() < Duration::from_secs(3));
        assert!(results.iter().all(|r| r.error == Some(ErrorKind::Timeout)));
        assert_eq!(
            results[0].error_message.as_deref(),
            Some("operation timed out")
        );
    }

    #[tokio::test]
    async fn a_refused_connection_is_a_connect_error_in_the_systems_words() {
        // A port nothing listens on: bound, read, and let go
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let results = run(request(&url, settings()), 1, 2).await;
        assert!(results.iter().all(|r| r.error == Some(ErrorKind::Connect)));
        let said = results[0].error_message.as_deref().unwrap();
        assert!(said.contains("refused"), "{said}");
    }

    #[test]
    fn a_route_is_the_request_reqwest_would_send() {
        let mut origins = Vec::new();
        let headers = ["Accept: text/x".to_string(), "X-A: 1".to_string()];
        let request = Request::new(
            "http://example.test:8080/a/b?x=1&y=%20z#frag".into(),
            "POST".into(),
            Some(b"hi".to_vec()),
            &headers,
            settings(),
        )
        .unwrap();
        let setup = Setup::new(&request, None).unwrap();
        let mut own = HeaderMap::new();
        own.insert("x-a", HeaderValue::from_static("2"));
        let route = Route::new(&setup, &request, &own, &mut origins).unwrap();
        let sent = String::from_utf8_lossy(&route.bytes).to_string();
        let (head, body) = sent.split_once("\r\n\r\n").unwrap();
        let mut lines: Vec<&str> = head.split("\r\n").collect();
        assert_eq!(lines.remove(0), "POST /a/b?x=1&y=%20z HTTP/1.1");
        lines.sort_unstable();
        // The run's Accept replaces the default; the target's own header
        // replaces the run's
        assert_eq!(
            lines,
            [
                "accept: text/x",
                "content-length: 2",
                "host: example.test:8080",
                "user-agent: pepe/test",
                "x-a: 2",
            ]
        );
        assert_eq!(body, "hi");
        assert!(!route.replayable && !route.head);
        assert_eq!(
            (origins[0].host.as_str(), origins[0].port),
            ("example.test", 8080)
        );
        assert!(origins[0].tls.is_none());

        // The default port stays out of Host, and one origin is one line
        let secure = self::request("https://example.test/", settings());
        let again = self::request("https://example.test:443/other", settings());
        let first = Route::new(&setup, &secure, &HeaderMap::new(), &mut origins).unwrap();
        let second = Route::new(&setup, &again, &HeaderMap::new(), &mut origins).unwrap();
        let sent = String::from_utf8_lossy(&first.bytes).to_string();
        assert!(sent.starts_with("GET / HTTP/1.1\r\n"), "{sent}");
        assert!(sent.contains("\r\nhost: example.test\r\n"), "{sent}");
        assert!(first.replayable);
        assert_eq!((first.line, second.line, origins.len()), (1, 1, 2));
        assert!(origins[1].tls.is_some());

        // IPv6: brackets in Host, none in the address to connect to
        let v6 = self::request("http://[::1]:9000/", settings());
        let route = Route::new(&setup, &v6, &HeaderMap::new(), &mut origins).unwrap();
        let sent = String::from_utf8_lossy(&route.bytes).to_string();
        assert!(sent.contains("\r\nhost: [::1]:9000\r\n"), "{sent}");
        assert_eq!(route.origin.host, "::1");
    }

    #[test]
    fn what_only_reqwest_does_is_left_to_it() {
        let mut origins = Vec::new();
        let plain = request("http://example.test/", settings());
        let setup = Setup::new(&plain, None).unwrap();
        let with_login = request("http://user:secret@example.test/", settings());
        assert!(Route::new(&setup, &with_login, &HeaderMap::new(), &mut origins).is_none());

        let proxied = RequestSettings {
            proxy: Some("http://127.0.0.1:3128".into()),
            ..settings()
        };
        assert!(Setup::new(&request("http://example.test/", proxied), None).is_none());
    }
}
