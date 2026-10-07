//! HTTP/1.1 as it goes over a connection: a request as the bytes to write,
//! a response's head parsed where it was read, and its body's framing.
//!
//! This is the part of the direct path (see `direct`) that knows the
//! protocol and nothing about sockets, so that all of it can be tested on
//! plain bytes. It keeps no copy of anything: a header is two ranges of the
//! buffer the response was read into, and a body is handed on as slices of
//! that buffer.

use std::ops::Range;

use bytes::Bytes;
use reqwest::header::{HeaderMap, CONTENT_LENGTH, TRANSFER_ENCODING};
use reqwest::Method;

/// Headers parsed on the stack; a response with more is parsed again with
/// room for `MAX_FIELDS`
const FIELDS: usize = 32;
const MAX_FIELDS: usize = 256;

/// The request as it is written: the head, and the body after it. Built
/// once for a target, so sending it is one write of bytes that are already
/// there.
pub fn request(method: &Method, path: &str, headers: &HeaderMap, body: &[u8]) -> Bytes {
    let mut out = Vec::with_capacity(256 + body.len());
    out.extend_from_slice(method.as_str().as_bytes());
    out.push(b' ');
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(b" HTTP/1.1\r\n");
    for (name, value) in headers {
        out.extend_from_slice(name.as_str().as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    // A body says how long it is, unless a header already says how it ends
    let framed = headers.contains_key(CONTENT_LENGTH) || headers.contains_key(TRANSFER_ENCODING);
    if !body.is_empty() && !framed {
        out.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    Bytes::from(out)
}

/// One header of a response: where its name and its value are in the
/// buffer the head was read into
#[derive(Debug, Clone, Copy)]
pub struct Field {
    name: (u32, u32),
    value: (u32, u32),
}

impl Field {
    /// The name, in whatever case the server wrote it
    pub fn name<'b>(&self, buf: &'b [u8]) -> &'b [u8] {
        &buf[self.name.0 as usize..self.name.1 as usize]
    }

    pub fn value<'b>(&self, buf: &'b [u8]) -> &'b [u8] {
        &buf[self.value.0 as usize..self.value.1 as usize]
    }
}

/// How a response's body ends
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// There is none: a HEAD's answer, a 204, a 304, `Content-Length: 0`
    None,
    /// This many bytes are left of it
    Length(u64),
    Chunked(Chunked),
    /// When the server closes the connection
    UntilClose,
}

/// A response's head
#[derive(Debug, PartialEq)]
pub struct Head {
    pub status: u16,
    /// The 1 of HTTP/1.1, or the 0 of HTTP/1.0
    pub minor: u8,
    /// Bytes of the buffer the head took; the body starts after them
    pub len: usize,
    pub body: Body,
    /// The connection can carry another request once the body is read
    pub keep_alive: bool,
}

/// Parse the head of a response from the start of `buf`, leaving its
/// headers in `fields`. None when the head isn't all there yet.
/// `to_head`: the request was a HEAD, whose answer has headers that
/// describe a body and no body.
pub fn head(
    buf: &[u8],
    to_head: bool,
    fields: &mut Vec<Field>,
) -> Result<Option<Head>, &'static str> {
    let mut few = [httparse::EMPTY_HEADER; FIELDS];
    let mut response = httparse::Response::new(&mut few);
    match response.parse(buf) {
        Ok(httparse::Status::Complete(len)) => read_head(buf, &response, len, to_head, fields),
        Ok(httparse::Status::Partial) => Ok(None),
        Err(httparse::Error::TooManyHeaders) => {
            let mut many = vec![httparse::EMPTY_HEADER; MAX_FIELDS];
            let mut response = httparse::Response::new(&mut many);
            match response.parse(buf) {
                Ok(httparse::Status::Complete(len)) => {
                    read_head(buf, &response, len, to_head, fields)
                }
                Ok(httparse::Status::Partial) => Ok(None),
                Err(e) => Err(parse_error(e)),
            }
        }
        Err(e) => Err(parse_error(e)),
    }
}

/// hyper's words for these, which are what pepe's reports have said so far
fn parse_error(error: httparse::Error) -> &'static str {
    match error {
        httparse::Error::Version => "invalid HTTP version parsed",
        httparse::Error::Status => "invalid HTTP status-code parsed",
        httparse::Error::TooManyHeaders => "message head is too large",
        _ => "invalid HTTP header parsed",
    }
}

fn read_head(
    buf: &[u8],
    response: &httparse::Response<'_, '_>,
    len: usize,
    to_head: bool,
    fields: &mut Vec<Field>,
) -> Result<Option<Head>, &'static str> {
    let status = response.code.unwrap_or(0);
    let minor = response.version.unwrap_or(1);
    let start = buf.as_ptr() as usize;
    let within = |part: &[u8]| {
        let from = (part.as_ptr() as usize - start) as u32;
        (from, from + part.len() as u32)
    };
    fields.clear();
    let mut length = None;
    let mut chunked = false;
    let mut close = false;
    let mut keep = false;
    for header in response.headers.iter() {
        fields.push(Field {
            name: within(header.name.as_bytes()),
            value: within(header.value),
        });
        // All three names are of different lengths, which is the cheap test
        match header.name.len() {
            14 if header.name.eq_ignore_ascii_case("content-length") => {
                let value = std::str::from_utf8(header.value)
                    .ok()
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .ok_or("invalid content-length parsed")?;
                if length.is_some_and(|other| other != value) {
                    return Err("invalid content-length parsed");
                }
                length = Some(value);
            }
            17 if header.name.eq_ignore_ascii_case("transfer-encoding") => {
                // The last coding says how the body is framed
                chunked = header
                    .value
                    .rsplit(|b| *b == b',')
                    .next()
                    .is_some_and(|last| last.trim_ascii().eq_ignore_ascii_case(b"chunked"));
            }
            10 if header.name.eq_ignore_ascii_case("connection") => {
                for token in header.value.split(|b| *b == b',') {
                    let token = token.trim_ascii();
                    close |= token.eq_ignore_ascii_case(b"close");
                    keep |= token.eq_ignore_ascii_case(b"keep-alive");
                }
            }
            _ => {}
        }
    }
    let body = if to_head || status < 200 || status == 204 || status == 304 {
        Body::None
    } else if chunked {
        Body::Chunked(Chunked::default())
    } else {
        match length {
            Some(0) => Body::None,
            Some(length) => Body::Length(length),
            None => Body::UntilClose,
        }
    };
    let keep_alive = match minor {
        0 => keep && !close,
        _ => !close,
    } && body != Body::UntilClose
        // A protocol switch: what follows isn't HTTP
        && status != 101;
    Ok(Some(Head {
        status,
        minor,
        len,
        body,
        keep_alive,
    }))
}

/// Where the decoder is in a chunked body
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum At {
    /// In the hex digits of a chunk's size
    #[default]
    Size,
    /// After the size, skipping an extension up to the line's end
    Extension,
    /// In a chunk's data, with `left` bytes of it to come
    Data,
    /// After a chunk's data, at its CR LF
    DataEnd,
    /// After the last chunk: trailer lines, up to an empty one
    Trailer,
    Done,
}

/// Decodes `Transfer-Encoding: chunked`, a piece of input at a time
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chunked {
    at: At,
    /// Bytes of the current chunk still to come
    left: u64,
    /// The size line has a digit, or the trailer line anything at all
    seen: bool,
}

/// What `Chunked::step` made of its input
#[derive(Debug, PartialEq)]
pub struct Step {
    /// Bytes of the input it is done with
    pub consumed: usize,
    /// The body's bytes among them, as a range of the input; may be empty
    pub data: Range<usize>,
}

impl Chunked {
    pub fn done(&self) -> bool {
        self.at == At::Done
    }

    /// Take the framing off the front of `input` and stop at the end of
    /// the first run of data, or of the input, or of the body
    pub fn step(&mut self, input: &[u8]) -> Result<Step, &'static str> {
        let mut i = 0;
        while i < input.len() {
            let byte = input[i];
            match self.at {
                At::Size => {
                    i += 1;
                    match byte {
                        b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' => {
                            let digit = (byte as char).to_digit(16).unwrap_or(0) as u64;
                            self.left = self
                                .left
                                .checked_mul(16)
                                .and_then(|n| n.checked_add(digit))
                                .ok_or("invalid chunk size")?;
                            self.seen = true;
                        }
                        b'\n' | b';' | b'\r' | b' ' | b'\t' if !self.seen => {
                            return Err("invalid chunk size")
                        }
                        b'\n' => self.sized(),
                        b';' | b'\r' | b' ' | b'\t' => self.at = At::Extension,
                        _ => return Err("invalid chunk size"),
                    }
                }
                At::Extension => {
                    i += 1;
                    if byte == b'\n' {
                        self.sized();
                    }
                }
                At::Data => {
                    let run = (self.left.min((input.len() - i) as u64)) as usize;
                    self.left -= run as u64;
                    if self.left == 0 {
                        self.at = At::DataEnd;
                    }
                    return Ok(Step {
                        consumed: i + run,
                        data: i..i + run,
                    });
                }
                At::DataEnd => {
                    i += 1;
                    match byte {
                        b'\r' => {}
                        b'\n' => {
                            self.at = At::Size;
                            self.seen = false;
                        }
                        _ => return Err("invalid chunk"),
                    }
                }
                At::Trailer => {
                    i += 1;
                    match byte {
                        b'\r' => {}
                        // An empty line ends the trailers, and the body
                        b'\n' if !self.seen => {
                            self.at = At::Done;
                            break;
                        }
                        b'\n' => self.seen = false,
                        _ => self.seen = true,
                    }
                }
                At::Done => break,
            }
        }
        Ok(Step {
            consumed: i,
            data: i..i,
        })
    }

    /// The size line is over
    fn sized(&mut self) {
        self.seen = false;
        self.at = if self.left == 0 {
            At::Trailer
        } else {
            At::Data
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderName, HeaderValue};

    fn parsed(text: &str) -> (Head, Vec<(String, String)>) {
        let mut fields = Vec::new();
        let head = head(text.as_bytes(), false, &mut fields).unwrap().unwrap();
        let named = fields
            .iter()
            .map(|f| {
                let value = String::from_utf8_lossy(f.value(text.as_bytes())).to_string();
                (
                    String::from_utf8_lossy(f.name(text.as_bytes())).to_string(),
                    value,
                )
            })
            .collect();
        (head, named)
    }

    #[test]
    fn a_request_is_its_line_its_headers_and_its_body() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", HeaderValue::from_static("pepe/test"));
        headers.append("x-a", HeaderValue::from_static("1"));
        headers.append("x-a", HeaderValue::from_static("2"));
        headers.insert("host", HeaderValue::from_static("example.test"));
        let get = request(&Method::GET, "/a?b=1", &headers, b"");
        assert_eq!(
            &get[..],
            b"GET /a?b=1 HTTP/1.1\r\nuser-agent: pepe/test\r\nx-a: 1\r\nx-a: 2\r\nhost: example.test\r\n\r\n"
        );
        let post = request(&Method::POST, "/", &HeaderMap::new(), b"hi");
        assert_eq!(&post[..], b"POST / HTTP/1.1\r\ncontent-length: 2\r\n\r\nhi");
        // A length the caller gave is the one that's sent
        let mut own = HeaderMap::new();
        own.insert(
            HeaderName::from_static("content-length"),
            HeaderValue::from_static("2"),
        );
        let post = request(&Method::POST, "/", &own, b"hi");
        assert_eq!(&post[..], b"POST / HTTP/1.1\r\ncontent-length: 2\r\n\r\nhi");
    }

    #[test]
    fn a_head_says_where_the_body_starts_and_how_it_ends() {
        let text = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello";
        let (head, fields) = parsed(text);
        assert_eq!((head.status, head.minor), (200, 1));
        assert_eq!(&text[head.len..], "hello");
        assert_eq!(head.body, Body::Length(5));
        assert!(head.keep_alive);
        assert_eq!(
            fields,
            [
                ("Content-Type".to_string(), "text/plain".to_string()),
                ("Content-Length".to_string(), "5".to_string())
            ]
        );

        let (head, _) = parsed("HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n");
        assert_eq!(head.body, Body::Chunked(Chunked::default()));
        // Chunked wins over a length, as RFC 9112 has it
        let (head, _) =
            parsed("HTTP/1.1 200 OK\r\ncontent-length: 3\r\ntransfer-encoding: chunked\r\n\r\n");
        assert_eq!(head.body, Body::Chunked(Chunked::default()));

        let (head, _) = parsed("HTTP/1.1 200 OK\r\n\r\n");
        assert_eq!((head.body, head.keep_alive), (Body::UntilClose, false));
        let (head, _) = parsed("HTTP/1.1 204 No Content\r\n\r\n");
        assert_eq!((head.body, head.keep_alive), (Body::None, true));
        let (head, _) = parsed("HTTP/1.1 304 Not Modified\r\ncontent-length: 9\r\n\r\n");
        assert_eq!(head.body, Body::None);
        let (head, _) = parsed("HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
        assert_eq!(head.body, Body::None);
    }

    #[test]
    fn a_head_request_has_no_body_whatever_the_headers_say() {
        let mut fields = Vec::new();
        let text = b"HTTP/1.1 200 OK\r\ncontent-length: 500\r\n\r\n";
        let head = head(text, true, &mut fields).unwrap().unwrap();
        assert_eq!((head.body, head.keep_alive), (Body::None, true));
    }

    #[test]
    fn the_connection_is_kept_unless_the_response_says_otherwise() {
        let keep = |text: &str| parsed(text).0.keep_alive;
        assert!(keep("HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n"));
        assert!(!keep(
            "HTTP/1.1 200 OK\r\nConnection: Close\r\ncontent-length: 0\r\n\r\n"
        ));
        assert!(!keep(
            "HTTP/1.1 200 OK\r\nconnection: foo, close\r\ncontent-length: 0\r\n\r\n"
        ));
        // HTTP/1.0 closes unless it says it won't
        assert!(!keep("HTTP/1.0 200 OK\r\ncontent-length: 0\r\n\r\n"));
        assert!(keep(
            "HTTP/1.0 200 OK\r\nConnection: keep-alive\r\ncontent-length: 0\r\n\r\n"
        ));
        assert!(!keep(
            "HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\n\r\n"
        ));
    }

    #[test]
    fn a_head_that_is_not_all_there_or_not_http_says_so() {
        let mut fields = Vec::new();
        assert_eq!(head(b"", false, &mut fields), Ok(None));
        assert_eq!(
            head(b"HTTP/1.1 200 OK\r\ncontent-le", false, &mut fields),
            Ok(None)
        );
        assert_eq!(
            head(b"SMTP ready\r\n\r\n", false, &mut fields),
            Err("invalid HTTP version parsed")
        );
        assert_eq!(
            head(
                b"HTTP/1.1 200 OK\r\ncontent-length: five\r\n\r\n",
                false,
                &mut fields
            ),
            Err("invalid content-length parsed")
        );
        assert_eq!(
            head(
                b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\ncontent-length: 6\r\n\r\n",
                false,
                &mut fields
            ),
            Err("invalid content-length parsed")
        );
    }

    #[test]
    fn more_headers_than_fit_on_the_stack_are_still_read() {
        let mut text = String::from("HTTP/1.1 200 OK\r\n");
        for i in 0..100 {
            text.push_str(&format!("x-{i}: {i}\r\n"));
        }
        text.push_str("content-length: 1\r\n\r\n");
        let (head, fields) = parsed(&text);
        assert_eq!((fields.len(), head.body), (101, Body::Length(1)));
        assert_eq!(fields[99], ("x-99".to_string(), "99".to_string()));
    }

    /// Decode `input` handed over in pieces of `piece` bytes
    fn decode(input: &[u8], piece: usize) -> Result<(Vec<u8>, bool, usize), &'static str> {
        let mut chunked = Chunked::default();
        let mut out = Vec::new();
        let mut used = 0;
        for part in input.chunks(piece) {
            let mut at = 0;
            while at < part.len() && !chunked.done() {
                let step = chunked.step(&part[at..])?;
                out.extend_from_slice(&part[at..][step.data]);
                at += step.consumed;
            }
            used += at;
        }
        Ok((out, chunked.done(), used))
    }

    #[test]
    fn chunks_are_decoded_however_the_bytes_arrive() {
        let body =
            b"5\r\nhello\r\n6;ext=1\r\n world\r\nA\r\n0123456789\r\n0\r\nx-trailer: 1\r\n\r\n";
        for piece in [1, 2, 3, 7, 64] {
            let (out, done, used) = decode(body, piece).unwrap();
            assert_eq!(out, b"hello world0123456789", "pieces of {piece}");
            assert!(done);
            assert_eq!(used, body.len());
        }
        // Without trailers, and with bare line feeds
        assert_eq!(decode(b"2\nok\n0\n\n", 3).unwrap().0, b"ok");
        // Not finished: the last chunk hasn't come
        assert!(!decode(b"5\r\nhel", 2).unwrap().1);
        // What follows the body is left alone
        let (out, done, used) = decode(b"1\r\na\r\n0\r\n\r\nHTTP/1.1 200", 64).unwrap();
        assert_eq!((out.as_slice(), done, used), (&b"a"[..], true, 11));
    }

    #[test]
    fn a_chunk_size_that_is_not_one_is_an_error() {
        assert_eq!(decode(b"zz\r\n", 4), Err("invalid chunk size"));
        assert_eq!(decode(b"\r\n", 4), Err("invalid chunk size"));
        assert_eq!(
            decode(b"ffffffffffffffffff\r\n", 64),
            Err("invalid chunk size")
        );
        assert_eq!(decode(b"1\r\nab", 64), Err("invalid chunk"));
    }
}
