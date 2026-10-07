//! Target server for the benchmarks in bench/README.md.
//!
//! It answers every request from memory as fast as hyper can on every core,
//! so that the load generator under test, not the server, is the bottleneck.
//!
//! ```text
//! cargo run --release --manifest-path bench/server/Cargo.toml
//!     http on 127.0.0.1:8089, https on 127.0.0.1:8090
//! ... -- --addr 0.0.0.0:9000 --tls-addr 0.0.0.0:9443
//! ... -- --tls-addr ""        no HTTPS listener
//! ... -- --latency 5ms        every answer takes at least this long
//! ... -- --threads 4          worker threads (default: every core)
//! ```
//!
//! The HTTPS listener uses a self-signed certificate made at startup, so
//! clients need to skip verification (pepe -k, oha --insecure).
//!
//! Paths:
//!
//! ```text
//! /            16-byte body
//! /1k /16k /256k   bodies of that size
//! /json        a small JSON object
//! /slow?ms=20  sleeps before answering
//! /status/503  answers with that status
//! /timed       like /, with Server-Timing and X-Request-Id headers
//! /count       requests answered so far, to check a client's own count
//! ```

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::header::{HeaderValue, CONTENT_TYPE};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

const SMALL: &[u8] = b"hello from pepe\n";
const JSON: &[u8] = br#"{"id":42,"name":"pepe","ok":true,"tags":["load","test"]}"#;
const TEXT: HeaderValue = HeaderValue::from_static("text/plain");

/// Everything a response is made from, built once
struct Bodies {
    k1: Bytes,
    k16: Bytes,
    k256: Bytes,
    /// Added to every answer (`--latency`)
    latency: Duration,
    /// Requests answered, for `/count` and `/timed`'s ids
    served: AtomicU64,
}

fn sized(bytes: usize) -> Bytes {
    let mut body = vec![b'x'; bytes];
    body.push(b'\n');
    Bytes::from(body)
}

fn text(body: Bytes) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(body));
    response.headers_mut().insert(CONTENT_TYPE, TEXT);
    response
}

async fn answer(
    bodies: Arc<Bodies>,
    request: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let served = bodies.served.fetch_add(1, Ordering::Relaxed) + 1;
    if !bodies.latency.is_zero() {
        tokio::time::sleep(bodies.latency).await;
    }
    let path = request.uri().path();
    let response = match path {
        "/" => text(Bytes::from_static(SMALL)),
        "/1k" => text(bodies.k1.clone()),
        "/16k" => text(bodies.k16.clone()),
        "/256k" => text(bodies.k256.clone()),
        "/json" => {
            let mut response = Response::new(Full::new(Bytes::from_static(JSON)));
            let headers = response.headers_mut();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
            headers.insert("x-cache", HeaderValue::from_static("HIT"));
            response
        }
        "/slow" => {
            let ms = request
                .uri()
                .query()
                .and_then(|q| q.split('&').find_map(|pair| pair.strip_prefix("ms=")))
                .and_then(|ms| ms.parse::<u64>().ok())
                .filter(|ms| *ms > 0)
                .unwrap_or(10);
            tokio::time::sleep(Duration::from_millis(ms)).await;
            text(Bytes::from_static(SMALL))
        }
        // What a traced backend sends: a few Server-Timing segments and the
        // request's id, to measure what reading them costs pepe
        "/timed" => {
            let mut response = text(Bytes::from_static(SMALL));
            let headers = response.headers_mut();
            headers.insert(
                "server-timing",
                HeaderValue::from_static(
                    "db;dur=2.4;desc=\"primary\", app;dur=0.7, cache;desc=HIT",
                ),
            );
            let id = format!("req-{served:012}");
            headers.insert("x-request-id", HeaderValue::from_str(&id).expect("ascii"));
            response
        }
        "/count" => text(Bytes::from(format!("{}\n", served - 1))),
        _ => match path.strip_prefix("/status/") {
            Some(code) => {
                let mut response = text(Bytes::from_static(SMALL));
                *response.status_mut() = code
                    .parse::<u16>()
                    .ok()
                    .and_then(|code| StatusCode::from_u16(code).ok())
                    .unwrap_or(StatusCode::BAD_REQUEST);
                response
            }
            None => text(Bytes::from_static(SMALL)),
        },
    };
    Ok(response)
}

/// One connection, plain or TLS, until the client closes it
async fn serve<S>(stream: S, bodies: Arc<Bodies>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let service = service_fn(move |request| answer(bodies.clone(), request));
    // A client that goes away mid-request is the client's business
    let _ = http1::Builder::new()
        .pipeline_flush(true)
        .serve_connection(TokioIo::new(stream), service)
        .await;
}

fn tune(stream: &TcpStream) {
    let _ = stream.set_nodelay(true);
}

async fn listen(addr: SocketAddr, bodies: Arc<Bodies>, tls: Option<TlsAcceptor>) {
    let listener = match TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("can't listen on {addr}: {e}");
            std::process::exit(1);
        }
    };
    let scheme = if tls.is_some() { "https" } else { "http" };
    println!("bench server on {scheme}://{addr}");
    loop {
        // Out of file descriptors, say: wait for some to be closed
        let Ok((stream, _)) = listener.accept().await else {
            tokio::time::sleep(Duration::from_millis(5)).await;
            continue;
        };
        tune(&stream);
        let bodies = bodies.clone();
        match tls.clone() {
            None => drop(tokio::spawn(serve(stream, bodies))),
            Some(tls) => drop(tokio::spawn(async move {
                if let Ok(stream) = tls.accept(stream).await {
                    serve(stream, bodies).await;
                }
            })),
        }
    }
}

/// A certificate for 127.0.0.1 and localhost, made now and never stored
fn self_signed() -> TlsAcceptor {
    let names = ["localhost", "127.0.0.1", "::1"].map(String::from).to_vec();
    let made = rcgen::generate_simple_self_signed(names).expect("make a certificate");
    let cert = CertificateDer::from(made.cert.der().to_vec());
    let key = PrivateKeyDer::try_from(made.key_pair.serialize_der()).expect("a PKCS#8 key");
    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .expect("a usable certificate");
    // HTTP/1.1 only, like the plain listener: the load generators under
    // test speak HTTP/1.1
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    TlsAcceptor::from(Arc::new(config))
}

/// "5ms", "2s", or a bare number of milliseconds
fn duration(text: &str) -> Option<Duration> {
    let (number, unit) = match text.find(|c: char| !c.is_ascii_digit() && c != '.') {
        Some(at) => text.split_at(at),
        None => (text, "ms"),
    };
    let number: f64 = number.parse().ok()?;
    let seconds = match unit {
        "us" | "µs" => number / 1e6,
        "ms" => number / 1e3,
        "s" => number,
        _ => return None,
    };
    Some(Duration::from_secs_f64(seconds))
}

fn usage(problem: &str) -> ! {
    eprintln!("{problem}\nusage: bench-server [--addr HOST:PORT] [--tls-addr HOST:PORT|\"\"] [--latency 5ms] [--threads N]");
    std::process::exit(2);
}

fn main() {
    let mut addr = "127.0.0.1:8089".to_string();
    let mut tls_addr = "127.0.0.1:8090".to_string();
    let mut latency = Duration::ZERO;
    let mut threads = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| usage(&format!("{flag} needs a value")))
        };
        // One dash is accepted too, as the Go server this replaced took
        match flag.trim_start_matches('-') {
            "addr" => addr = value(),
            "tls-addr" => tls_addr = value(),
            "latency" => {
                let text = value();
                latency =
                    duration(&text).unwrap_or_else(|| usage(&format!("bad latency {text:?}")));
            }
            "threads" => threads = value().parse::<usize>().ok().filter(|n| *n > 0),
            _ => usage(&format!("unknown flag {flag}")),
        }
    }
    let parse = |text: &str| -> SocketAddr {
        // ":9000" means every interface, as it did for the Go server
        let full = if text.starts_with(':') {
            format!("0.0.0.0{text}")
        } else {
            text.to_string()
        };
        full.parse()
            .unwrap_or_else(|_| usage(&format!("bad address {text:?}")))
    };

    let bodies = Arc::new(Bodies {
        k1: sized(1024),
        k16: sized(16 * 1024),
        k256: sized(256 * 1024),
        latency,
        served: AtomicU64::new(0),
    });
    let mut runtime = tokio::runtime::Builder::new_multi_thread();
    runtime.enable_all();
    if let Some(threads) = threads {
        runtime.worker_threads(threads);
    }
    runtime
        .build()
        .expect("build a tokio runtime")
        .block_on(async {
            let mut listeners = Vec::new();
            if !tls_addr.is_empty() {
                let tls = Some(self_signed());
                listeners.push(tokio::spawn(listen(parse(&tls_addr), bodies.clone(), tls)));
            }
            listeners.push(tokio::spawn(listen(parse(&addr), bodies, None)));
            for listener in listeners {
                let _ = listener.await;
            }
        });
}
