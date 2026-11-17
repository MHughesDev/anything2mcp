//! The harness: starts a device for a test, on a port the system chooses, and
//! stops it when the test ends.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

use crate::control::{Control, Shared};
use crate::device::DeviceConfig;
use crate::protocol::{HEADER_BYTES, Header};

/// The device could not be started: no local port could be bound.
#[derive(Debug)]
pub struct StartError(std::io::Error);

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the simulated device could not be started: {}", self.0)
    }
}

impl std::error::Error for StartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// One running simulated device, on its own port, stopped when dropped.
#[derive(Debug)]
pub struct TestDevice {
    address: SocketAddr,
    unit: u8,
    control: Control,
    server: JoinHandle<()>,
}

impl TestDevice {
    /// Starts a device in its starting state on a free port of the loopback address.
    ///
    /// # Errors
    ///
    /// Returns an error if no port could be bound.
    pub async fn start() -> Result<Self, StartError> {
        Self::start_with(DeviceConfig::default()).await
    }

    /// Starts a device with the given unit identifier, firmware, and pump ramp.
    ///
    /// # Errors
    ///
    /// Returns an error if no port could be bound.
    pub async fn start_with(config: DeviceConfig) -> Result<Self, StartError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(StartError)?;
        let address = listener.local_addr().map_err(StartError)?;
        let shared = Shared::new(config);
        let control = Control::new(Arc::clone(&shared));
        let server = tokio::spawn(serve(listener, shared));
        Ok(Self {
            address,
            unit: config.unit,
            control,
            server,
        })
    }

    /// The address the device listens on.
    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    /// The port the device listens on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.address.port()
    }

    /// The unit identifier the device answers for.
    #[must_use]
    pub const fn unit(&self) -> u8 {
        self.unit
    }

    /// The control interface: ground truth, and the faults.
    #[must_use]
    pub const fn control(&self) -> &Control {
        &self.control
    }
}

impl Drop for TestDevice {
    fn drop(&mut self) {
        // Stopping the server task drops its links with it.
        self.server.abort();
    }
}

/// Accepts links until the task is stopped. Each link is its own task, and
/// all of them stop with this one.
async fn serve(listener: TcpListener, shared: Arc<Shared>) {
    let mut links = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    links.spawn(serve_link(Arc::clone(&shared), stream));
                }
                // A link that failed while being accepted is the client's to
                // notice; the device keeps listening.
                Err(_) => tokio::task::yield_now().await,
            },
            Some(_) = links.join_next() => {}
        }
    }
}

enum Arrived {
    Frame {
        header: Header,
        pdu: Vec<u8>,
        raw: Vec<u8>,
    },
    BadHeader {
        header: Header,
        raw: Vec<u8>,
    },
}

async fn read_frame(stream: &mut TcpStream) -> std::io::Result<Arrived> {
    let mut head = [0u8; HEADER_BYTES];
    stream.read_exact(&mut head).await?;
    let header = Header::parse(&head);
    let Some(length) = header.pdu_bytes() else {
        return Ok(Arrived::BadHeader {
            header,
            raw: head.to_vec(),
        });
    };
    let mut pdu = vec![0u8; length];
    stream.read_exact(&mut pdu).await?;
    let mut raw = head.to_vec();
    raw.extend(&pdu);
    Ok(Arrived::Frame { header, pdu, raw })
}

/// One link: one request at a time, each answered before the next is read.
async fn serve_link(shared: Arc<Shared>, mut stream: TcpStream) {
    // Subscribe before asking whether the device is up, so that a restart
    // between the two is seen by one or the other.
    let mut cut = shared.cut.subscribe();
    let Some(link) = shared.change(|inner| inner.link_opened()) else {
        return;
    };
    // Replies are small and each is wanted at once.
    let _ = stream.set_nodelay(true);
    loop {
        let arrived = tokio::select! {
            biased;
            _ = cut.changed() => break,
            arrived = read_frame(&mut stream) => arrived,
        };
        let plan = match arrived {
            Ok(Arrived::Frame { header, pdu, raw }) => {
                shared.change(|inner| inner.handle(link, &header, &pdu, raw))
            }
            Ok(Arrived::BadHeader { header, raw }) => {
                shared.change(|inner| inner.bad_header(link, &header, raw));
                break;
            }
            // The client closed the link, or it failed.
            Err(_) => break,
        };
        if let Some(ms) = plan.delay_ms {
            // This wait is the fault itself: a slow reply.
            tokio::select! {
                _ = cut.changed() => break,
                () = tokio::time::sleep(Duration::from_millis(ms)) => {}
            }
        }
        if let Some(reply) = plan.reply
            && stream.write_all(&reply).await.is_err()
        {
            break;
        }
        if plan.close || !shared.powered() {
            break;
        }
    }
    // The device learns the link is gone before the client can see it close.
    shared.change(|inner| inner.link_closed());
    drop(stream);
}
