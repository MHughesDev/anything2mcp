//! A small relay between the engine and a test stand, so that a test can
//! lose the answer to a write after the stand stored it: the fault the
//! stand's own control interface cannot make. It names no real system.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// What the relay does with the next write it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnWrite {
    /// Relays the answer.
    Pass,
    /// Lets the stand store the write and answer, then closes the caller's
    /// connection without the answer. Once; then it passes again.
    LoseAnswer,
    /// Lets the stand store the write and answer, then keeps the caller
    /// waiting for ever. Once; then it passes again.
    HoldAnswer,
}

/// A relay on its own port, stopped when dropped.
pub struct Proxy {
    base: String,
    on_write: Arc<Mutex<OnWrite>>,
    stored: Arc<Notify>,
    server: JoinHandle<()>,
}

impl Proxy {
    /// Starts a relay to the stand at `upstream`, an address such as
    /// `http://127.0.0.1:4010`.
    pub async fn start(upstream: &str) -> Self {
        let upstream = upstream
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_owned();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let on_write = Arc::new(Mutex::new(OnWrite::Pass));
        let stored = Arc::new(Notify::new());
        let (mode, told) = (Arc::clone(&on_write), Arc::clone(&stored));
        let server = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                tokio::spawn(relay(
                    client,
                    upstream.clone(),
                    Arc::clone(&mode),
                    Arc::clone(&told),
                ));
            }
        });
        Self {
            base: format!("http://{address}"),
            on_write,
            stored,
            server,
        }
    }

    /// The relay's address, to write where the stand's would be.
    pub fn url(&self) -> &str {
        &self.base
    }

    /// Sets what happens to the next write.
    pub fn on_write(&self, what: OnWrite) {
        *self.on_write.lock().unwrap() = what;
    }

    /// Waits until the stand has answered a write whose answer the relay
    /// then lost or held.
    pub async fn stored(&self) {
        self.stored.notified().await;
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Reads one request: its head, and a body of the length the head states.
async fn read_request(client: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = find(&bytes, b"\r\n\r\n") {
            break at + 4;
        }
        let read = client.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8(bytes[..head_end].to_vec()).ok()?;
    let length: usize = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0);
    let mut body = bytes[head_end..].to_vec();
    while body.len() < length {
        let read = client.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some((head, body))
}

async fn relay(
    mut client: TcpStream,
    upstream: String,
    on_write: Arc<Mutex<OnWrite>>,
    stored: Arc<Notify>,
) {
    let Some((head, body)) = read_request(&mut client).await else {
        return;
    };
    let is_write = !(head.starts_with("GET ") || head.starts_with("HEAD "));
    // One request for each connection, so the answer ends where the
    // stand closes.
    let mut lines: Vec<&str> = head
        .trim_end()
        .lines()
        .filter(|line| !line.to_ascii_lowercase().starts_with("connection:"))
        .collect();
    lines.push("connection: close");
    let mut request = lines.join("\r\n").into_bytes();
    request.extend_from_slice(b"\r\n\r\n");
    request.extend_from_slice(&body);

    let Ok(mut stand) = TcpStream::connect(&upstream).await else {
        return;
    };
    if stand.write_all(&request).await.is_err() {
        return;
    }
    let mut answer = Vec::new();
    if stand.read_to_end(&mut answer).await.is_err() {
        return;
    }
    let what = if is_write {
        std::mem::replace(&mut *on_write.lock().unwrap(), OnWrite::Pass)
    } else {
        OnWrite::Pass
    };
    match what {
        OnWrite::Pass => {
            let _ = client.write_all(&answer).await;
            let _ = client.shutdown().await;
        }
        OnWrite::LoseAnswer => {
            stored.notify_one();
            drop(client);
        }
        OnWrite::HoldAnswer => {
            stored.notify_one();
            // Holds the caller's connection open, silent, until the relay
            // is dropped or the caller goes away.
            let mut rest = [0u8; 64];
            while matches!(client.read(&mut rest).await, Ok(read) if read > 0) {}
        }
    }
}
