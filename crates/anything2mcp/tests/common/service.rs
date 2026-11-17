//! A small signed-in service that runs inside a test: it answers
//! `GET /account` only to a caller that sends the bearer token it was
//! started with, and it echoes that token in its answer, so that a test
//! can see whether a secret the system itself returns is kept from the
//! caller. It names no real system.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// The service on its own port, stopped when dropped.
pub struct Service {
    base: String,
    seen: Arc<Mutex<Vec<String>>>,
    server: JoinHandle<()>,
}

impl Service {
    /// Starts the service. It accepts `authorization: Bearer <token>`.
    pub async fn start(token: &str) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let expected = format!("authorization: Bearer {token}");
        let echo = token.to_owned();
        let server = tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let log = Arc::clone(&log);
                let (expected, echo) = (expected.clone(), echo.clone());
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let mut chunk = [0_u8; 4096];
                    while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                        match stream.read(&mut chunk).await {
                            Ok(read) if read > 0 => bytes.extend_from_slice(&chunk[..read]),
                            _ => return,
                        }
                    }
                    let head = String::from_utf8_lossy(&bytes).into_owned();
                    let mut lines = head.lines();
                    let first = lines.next().unwrap_or_default().to_owned();
                    let signed_in = lines.take_while(|line| !line.is_empty()).any(|line| {
                        let (name, value) = line.split_once(':').unwrap_or((line, ""));
                        format!("{}: {}", name.to_ascii_lowercase(), value.trim()) == expected
                    });
                    log.lock()
                        .unwrap()
                        .push(format!("{first} signed_in={signed_in}"));
                    let (status, body) = match (first.as_str(), signed_in) {
                        ("GET /account HTTP/1.1", true) => (
                            "200 OK",
                            format!(r#"{{"name":"service","echo":"{echo}"}}"#),
                        ),
                        ("GET /account HTTP/1.1", false) => {
                            ("401 Unauthorized", r#"{"error":"unauthorized"}"#.to_owned())
                        }
                        _ => ("404 Not Found", r#"{"error":"not_found"}"#.to_owned()),
                    };
                    let answer = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(answer.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self { base, seen, server }
    }

    /// The service's address.
    pub fn url(&self) -> &str {
        &self.base
    }

    /// Every request the service saw: its first line, and whether it
    /// carried the token.
    pub fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.server.abort();
    }
}
