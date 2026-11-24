//! A small relay between the engine and a test stand, so that a test can
//! hold back the answer to a write after the stand stored it, and kill the
//! caller while it waits: the one fault the stand's own control interface
//! cannot make; and change the reads that follow a write, so that a
//! session can end, or an edge can block, between a write and its
//! read-back. It names no real system.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// What the relay does to a read it changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Alteration {
    /// The read goes to the stand without its `Authorization` header, so
    /// that the stand's own guard refuses it, as it refuses a session that
    /// has ended.
    StripAuthorization,
    /// The read goes to this path of the stand instead, such as the
    /// stand's `/edge/blocked`.
    Divert(String),
}

/// Which reads the relay changes, and from when.
#[derive(Debug, Default)]
struct ReadRule {
    alteration: Option<Alteration>,
    /// Whether the rule applies now; until then it waits for a write to
    /// pass.
    armed: bool,
}

/// A relay on its own port, stopped when dropped.
pub struct Proxy {
    base: String,
    hold: Arc<AtomicBool>,
    stored: Arc<Notify>,
    reads: Arc<Mutex<ReadRule>>,
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
        let hold = Arc::new(AtomicBool::new(false));
        let stored = Arc::new(Notify::new());
        let reads = Arc::new(Mutex::new(ReadRule::default()));
        let (mode, told, rule) = (Arc::clone(&hold), Arc::clone(&stored), Arc::clone(&reads));
        let server = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                tokio::spawn(relay(
                    client,
                    upstream.clone(),
                    Arc::clone(&mode),
                    Arc::clone(&told),
                    Arc::clone(&rule),
                ));
            }
        });
        Self {
            base: format!("http://{address}"),
            hold,
            stored,
            reads,
            server,
        }
    }

    /// The relay's address, to write where the stand's would be.
    pub fn url(&self) -> &str {
        &self.base
    }

    /// The next write is carried to the stand, which stores it and
    /// answers; the answer is then held back for ever. Once; writes after
    /// it pass.
    pub fn hold_next_write(&self) {
        self.hold.store(true, Ordering::SeqCst);
    }

    /// Waits until the stand has answered a write whose answer the relay
    /// then held.
    pub async fn stored(&self) {
        self.stored.notified().await;
    }

    /// Every read is changed as `alteration` says, from now, or once the
    /// next write has passed when `after_a_write`; until
    /// [`Proxy::pass_reads`].
    pub fn alter_reads(&self, alteration: Alteration, after_a_write: bool) {
        *self.reads.lock().unwrap() = ReadRule {
            alteration: Some(alteration),
            armed: !after_a_write,
        };
    }

    /// Reads pass unchanged again.
    pub fn pass_reads(&self) {
        *self.reads.lock().unwrap() = ReadRule::default();
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
    hold: Arc<AtomicBool>,
    stored: Arc<Notify>,
    reads: Arc<Mutex<ReadRule>>,
) {
    let Some((head, body)) = read_request(&mut client).await else {
        return;
    };
    let is_write = !(head.starts_with("GET ") || head.starts_with("HEAD "));
    let alteration = {
        let rule = reads.lock().unwrap();
        if !is_write && rule.armed {
            rule.alteration.clone()
        } else {
            None
        }
    };
    // One request for each connection, so the answer ends where the
    // stand closes.
    let mut lines: Vec<String> = head
        .trim_end()
        .lines()
        .filter(|line| !line.to_ascii_lowercase().starts_with("connection:"))
        .filter(|line| {
            alteration != Some(Alteration::StripAuthorization)
                || !line.to_ascii_lowercase().starts_with("authorization:")
        })
        .map(str::to_owned)
        .collect();
    if let (Some(Alteration::Divert(path)), Some(first)) = (&alteration, lines.first_mut()) {
        let method = first.split(' ').next().unwrap_or("GET").to_owned();
        *first = format!("{method} {path} HTTP/1.1");
    }
    lines.push("connection: close".to_owned());
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
    if is_write {
        let mut rule = reads.lock().unwrap();
        if rule.alteration.is_some() {
            rule.armed = true;
        }
    }
    if is_write && hold.swap(false, Ordering::SeqCst) {
        stored.notify_one();
        // Holds the caller's connection open, silent, until the relay is
        // dropped or the caller goes away.
        let mut rest = [0u8; 64];
        while matches!(client.read(&mut rest).await, Ok(read) if read > 0) {}
    } else {
        let _ = client.write_all(&answer).await;
        let _ = client.shutdown().await;
    }
}
