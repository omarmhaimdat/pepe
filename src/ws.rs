//! The little of WebSocket (RFC 6455) a client needs to hold Slack's
//! Socket Mode connection: the handshake, text messages each way, pings
//! answered, a close said. Over the same rustls as the rest of pepe, so no
//! client library comes in for it. `ws://` too, for tests.

use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// The most a message may be: Socket Mode's are a few kilobytes
const MOST: usize = 16 * 1024 * 1024;

pub struct Socket {
    io: Box<dyn Io>,
    /// Read and not yet taken
    buf: Vec<u8>,
}

/// Random bytes from the TLS provider's generator
fn random<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    let provider = tokio_rustls::rustls::crypto::ring::default_provider();
    let _ = provider.secure_random.fill(&mut out);
    out
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(ABC[(n >> shift) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Open `url` (wss:// or ws://) and upgrade it
pub async fn connect(url: &str) -> Result<Socket, String> {
    let url = reqwest::Url::parse(url).map_err(|e| format!("{url}: {e}"))?;
    let tls = match url.scheme() {
        "wss" => true,
        "ws" => false,
        other => {
            return Err(format!(
                "a WebSocket URL is wss:// or ws://, not {other}://"
            ))
        }
    };
    let host = url
        .host_str()
        .ok_or("the WebSocket URL has no host")?
        .to_string();
    let port = url.port().unwrap_or(if tls { 443 } else { 80 });
    let tcp = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| format!("couldn't reach {host}: {e}"))?;
    let _ = tcp.set_nodelay(true);
    let mut io: Box<dyn Io> = if tls {
        let mut config = crate::direct::tls_config(false);
        config.alpn_protocols.clear();
        let name = ServerName::try_from(host.clone()).map_err(|e| e.to_string())?;
        let stream = TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await
            .map_err(|e| format!("TLS with {host}: {e}"))?;
        Box::new(stream)
    } else {
        Box::new(tcp)
    };
    let path = match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_string(),
    };
    let key = base64(&random::<16>());
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    io.write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    // The answer's head, and whatever came after it
    let mut buf = Vec::new();
    let end = loop {
        let mut chunk = [0u8; 4096];
        let n = io.read(&mut chunk).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("the server closed before upgrading".into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        if buf.len() > 64 * 1024 {
            return Err("the server's answer has no end".into());
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let status = head.lines().next().unwrap_or("");
    if !status.contains(" 101") {
        return Err(format!("the server didn't upgrade: {status}"));
    }
    Ok(Socket {
        io,
        buf: buf[end + 4..].to_vec(),
    })
}

impl Socket {
    async fn fill(&mut self, want: usize) -> Result<(), String> {
        while self.buf.len() < want {
            let mut chunk = [0u8; 8192];
            let n = self.io.read(&mut chunk).await.map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("the connection closed".into());
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
        Ok(())
    }

    /// One frame: whether it ends a message, its opcode, its payload
    async fn frame(&mut self) -> Result<(bool, u8, Vec<u8>), String> {
        self.fill(2).await?;
        let fin = self.buf[0] & 0x80 != 0;
        let opcode = self.buf[0] & 0x0f;
        let masked = self.buf[1] & 0x80 != 0;
        let mut len = u64::from(self.buf[1] & 0x7f);
        let mut at = 2;
        if len == 126 {
            self.fill(4).await?;
            len = u64::from(u16::from_be_bytes([self.buf[2], self.buf[3]]));
            at = 4;
        } else if len == 127 {
            self.fill(10).await?;
            len = u64::from_be_bytes(self.buf[2..10].try_into().expect("eight bytes"));
            at = 10;
        }
        if len as usize > MOST {
            return Err("a message over 16 MB".into());
        }
        let mask_at = at;
        if masked {
            at += 4;
        }
        let total = at + len as usize;
        self.fill(total).await?;
        let mut payload = self.buf[at..total].to_vec();
        if masked {
            let key: [u8; 4] = self.buf[mask_at..mask_at + 4]
                .try_into()
                .expect("four bytes");
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= key[i % 4];
            }
        }
        self.buf.drain(..total);
        Ok((fin, opcode, payload))
    }

    async fn write(&mut self, opcode: u8, payload: &[u8]) -> Result<(), String> {
        let mut out = vec![0x80 | opcode];
        // A client masks what it sends
        match payload.len() {
            n if n < 126 => out.push(0x80 | n as u8),
            n if n <= u16::MAX as usize => {
                out.push(0x80 | 126);
                out.extend_from_slice(&(n as u16).to_be_bytes());
            }
            n => {
                out.push(0x80 | 127);
                out.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        let key = random::<4>();
        out.extend_from_slice(&key);
        out.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
        self.io.write_all(&out).await.map_err(|e| e.to_string())?;
        self.io.flush().await.map_err(|e| e.to_string())
    }

    /// The next text message; None when the server closes. Pings are
    /// answered on the way.
    pub async fn recv(&mut self) -> Result<Option<String>, String> {
        let mut message: Vec<u8> = Vec::new();
        loop {
            let (fin, opcode, payload) = self.frame().await?;
            match opcode {
                0x0..=0x2 => {
                    message.extend_from_slice(&payload);
                    if message.len() > MOST {
                        return Err("a message over 16 MB".into());
                    }
                    if fin {
                        return Ok(Some(String::from_utf8_lossy(&message).into_owned()));
                    }
                }
                0x8 => {
                    let _ = self.write(0x8, &payload[..payload.len().min(2)]).await;
                    return Ok(None);
                }
                0x9 => self.write(0xA, &payload).await?,
                _ => {}
            }
        }
    }

    pub async fn send(&mut self, text: &str) -> Result<(), String> {
        self.write(0x1, text.as_bytes()).await
    }

    pub async fn close(&mut self) {
        let _ = self.write(0x8, &1000u16.to_be_bytes()).await;
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn base64_as_the_rfc_has_it() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    /// A server's side of one connection: the upgrade, then frames
    pub struct Server {
        pub io: TcpStream,
    }

    impl Server {
        pub async fn accept(listener: &TcpListener) -> Server {
            let (mut io, _) = listener.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                let mut b = [0u8; 1];
                io.read_exact(&mut b).await.unwrap();
                head.push(b[0]);
            }
            io.write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n")
                .await
                .unwrap();
            Server { io }
        }

        /// A text frame, unmasked, in `parts` fragments
        pub async fn send(&mut self, text: &str, parts: usize) {
            let bytes = text.as_bytes();
            let size = bytes.len().div_ceil(parts.max(1)).max(1);
            let chunks: Vec<&[u8]> = bytes.chunks(size).collect();
            for (i, chunk) in chunks.iter().enumerate() {
                let fin = if i + 1 == chunks.len() { 0x80 } else { 0 };
                let opcode = if i == 0 { 0x1 } else { 0x0 };
                let mut out = vec![fin | opcode];
                if chunk.len() < 126 {
                    out.push(chunk.len() as u8);
                } else {
                    out.push(126);
                    out.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
                }
                out.extend_from_slice(chunk);
                self.io.write_all(&out).await.unwrap();
            }
        }

        pub async fn ping(&mut self) {
            self.io.write_all(&[0x89, 2, b'h', b'i']).await.unwrap();
        }

        /// The next frame from the client, unmasked: its opcode and text
        pub async fn recv(&mut self) -> (u8, String) {
            let mut h = [0u8; 2];
            self.io.read_exact(&mut h).await.unwrap();
            assert!(h[1] & 0x80 != 0, "a client masks its frames");
            let mut len = usize::from(h[1] & 0x7f);
            if len == 126 {
                let mut l = [0u8; 2];
                self.io.read_exact(&mut l).await.unwrap();
                len = usize::from(u16::from_be_bytes(l));
            }
            let mut key = [0u8; 4];
            self.io.read_exact(&mut key).await.unwrap();
            let mut payload = vec![0u8; len];
            self.io.read_exact(&mut payload).await.unwrap();
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= key[i % 4];
            }
            (h[0] & 0x0f, String::from_utf8_lossy(&payload).into_owned())
        }
    }

    #[tokio::test]
    async fn messages_both_ways_fragments_and_pings() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/link?ticket=1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut s = Server::accept(&listener).await;
            s.send(r#"{"type":"hello"}"#, 1).await;
            s.ping().await;
            let long = "x".repeat(300);
            s.send(&long, 3).await;
            let (op, pong) = s.recv().await;
            assert_eq!((op, pong.as_str()), (0xA, "hi"), "a ping is answered");
            let (op, text) = s.recv().await;
            assert_eq!((op, text.as_str()), (0x1, r#"{"envelope_id":"1"}"#));
            s.io.write_all(&[0x88, 0]).await.unwrap();
        });
        let mut socket = connect(&url).await.unwrap();
        assert_eq!(socket.recv().await.unwrap().unwrap(), r#"{"type":"hello"}"#);
        assert_eq!(
            socket.recv().await.unwrap().unwrap().len(),
            300,
            "fragments joined"
        );
        socket.send(r#"{"envelope_id":"1"}"#).await.unwrap();
        assert_eq!(socket.recv().await.unwrap(), None, "closed");
        server.await.unwrap();
    }
}
