//! A relay that gives one part of a test stand an origin of its own: a
//! request for `/x` at the relay is sent to the stand as `/<prefix>/x`, and
//! the stand's answer comes back as it is. A system that lives at the root
//! of its own host is what the web packs' drafting places a surface by;
//! the stand serves its signed-in parts under a path. Nothing in a request
//! or an answer is read or kept but the request line. It names no real
//! system.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// The most bytes a request's head may have.
const HEAD_LONGEST: usize = 64 * 1024;

/// A relay on its own port, stopped when dropped.
pub struct Prefixed {
    base: String,
    server: JoinHandle<()>,
}

impl Prefixed {
    /// Starts a relay to the stand at `upstream`, such as
    /// `http://127.0.0.1:4010`, under `prefix`, such as `/secure/bearer`.
    pub async fn start(upstream: &str, prefix: &str) -> Self {
        let upstream = upstream
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_owned();
        let prefix = prefix.trim_end_matches('/').to_owned();
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                tokio::spawn(relay(client, upstream.clone(), prefix.clone()));
            }
        });
        Self {
            base: format!("http://{address}"),
            server,
        }
    }

    /// The relay's own address, with no path.
    pub fn base(&self) -> &str {
        &self.base
    }
}

impl Drop for Prefixed {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// One request, and its answer. The request is sent on with its path under
/// the prefix and the connection marked to close after it, so that the
/// stand answers it alone; everything else passes through as it is.
async fn relay(mut client: TcpStream, upstream: String, prefix: String) {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > HEAD_LONGEST || client.read(&mut byte).await.unwrap_or(0) == 0 {
            return;
        }
        head.push(byte[0]);
    }
    let Ok(text) = String::from_utf8(head) else {
        return;
    };
    let mut lines: Vec<String> = text
        .trim_end_matches("\r\n")
        .split("\r\n")
        .map(str::to_owned)
        .collect();
    let Some(first) = lines.first_mut() else {
        return;
    };
    let mut parts = first.splitn(3, ' ');
    let (Some(method), Some(path), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return;
    };
    *first = format!("{method} {prefix}{path} {version}");
    lines.retain(|line| !line.to_ascii_lowercase().starts_with("connection:"));
    lines.push("connection: close".to_owned());
    let head = format!("{}\r\n\r\n", lines.join("\r\n"));
    let Ok(mut stand) = TcpStream::connect(&upstream).await else {
        return;
    };
    if stand.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut stand).await;
}
