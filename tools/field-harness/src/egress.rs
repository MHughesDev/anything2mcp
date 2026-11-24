//! The harness's closed-network check (decision 0310, point 5): before it
//! serves, it tries one public address, and it refuses to serve when that
//! address answers. It fails closed: only a clear refusal to route, or no
//! answer in time, counts as closed. Any other outcome, including a refused
//! connection (something answered) and an error nobody expected, counts as
//! reached.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;

/// The public address tried: a resolver that answers anywhere there is a route.
pub const PUBLIC_ADDRESS: &str = "1.1.1.1:443";

/// How long the attempt waits before it counts as unanswered.
pub const WAIT: Duration = Duration::from_secs(5);

/// A way to open a connection, so that a test gives its own.
pub trait Connector: Send + Sync {
    /// Tries to connect to `address`.
    fn connect(
        &self,
        address: SocketAddr,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + '_>>;
}

/// The operating system's own connection.
pub struct Tcp;

impl Connector for Tcp {
    fn connect(
        &self,
        address: SocketAddr,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + '_>> {
        Box::pin(async move { tokio::net::TcpStream::connect(address).await.map(drop) })
    }
}

/// What the check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Egress {
    /// Nothing was reached: the system said there is no route, or nothing
    /// answered in time.
    Closed,
    /// Something answered, or the outcome is not one that proves a closed
    /// network.
    Reached(String),
}

/// Whether the outcome of a connection proves that no route leads out.
fn proves_no_route(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NetworkUnreachable
            | io::ErrorKind::HostUnreachable
            | io::ErrorKind::TimedOut
    )
}

/// Tries `address` through `connector`, waiting at most `wait`.
pub async fn check(connector: &dyn Connector, address: SocketAddr, wait: Duration) -> Egress {
    match tokio::time::timeout(wait, connector.connect(address)).await {
        // No answer in time.
        Err(_) => Egress::Closed,
        Ok(Ok(())) => Egress::Reached(format!("{address} accepted a connection")),
        Ok(Err(error)) if proves_no_route(&error) => Egress::Closed,
        Ok(Err(error)) => Egress::Reached(format!(
            "{address} was tried and the outcome does not show a closed network ({})",
            error.kind()
        )),
    }
}

/// Parses [`PUBLIC_ADDRESS`].
pub fn public_address() -> Option<SocketAddr> {
    PUBLIC_ADDRESS.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests: a helper that cannot do its work stops the test"
    )]
    mod held {
        use super::*;

        struct Fixed(fn() -> io::Result<()>);

        impl Connector for Fixed {
            fn connect(
                &self,
                _address: SocketAddr,
            ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + '_>> {
                let outcome = (self.0)();
                Box::pin(async move { outcome })
            }
        }

        struct Silent;

        impl Connector for Silent {
            fn connect(
                &self,
                _address: SocketAddr,
            ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + '_>> {
                Box::pin(std::future::pending())
            }
        }

        fn address() -> SocketAddr {
            public_address().unwrap()
        }

        #[tokio::test]
        async fn a_connection_that_is_made_is_reached() {
            let found = check(&Fixed(|| Ok(())), address(), WAIT).await;
            assert!(matches!(found, Egress::Reached(_)), "{found:?}");
        }

        #[tokio::test]
        async fn a_refused_connection_is_reached_because_something_answered() {
            let refused = || Err(io::ErrorKind::ConnectionRefused.into());
            let found = check(&Fixed(refused), address(), WAIT).await;
            assert!(matches!(found, Egress::Reached(_)), "{found:?}");
        }

        #[tokio::test]
        async fn an_error_nobody_expected_is_reached() {
            let outcomes: [fn() -> io::Result<()>; 4] = [
                || Err(io::ErrorKind::PermissionDenied.into()),
                || Err(io::ErrorKind::ConnectionReset.into()),
                || Err(io::ErrorKind::AddrNotAvailable.into()),
                || Err(io::Error::other("something else")),
            ];
            for outcome in outcomes {
                let found = check(&Fixed(outcome), address(), WAIT).await;
                assert!(matches!(found, Egress::Reached(_)), "{found:?}");
            }
        }

        #[tokio::test]
        async fn only_a_refusal_to_route_or_a_timeout_is_closed() {
            let outcomes: [fn() -> io::Result<()>; 3] = [
                || Err(io::ErrorKind::NetworkUnreachable.into()),
                || Err(io::ErrorKind::HostUnreachable.into()),
                || Err(io::ErrorKind::TimedOut.into()),
            ];
            for outcome in outcomes {
                assert_eq!(
                    check(&Fixed(outcome), address(), WAIT).await,
                    Egress::Closed
                );
            }
        }

        #[tokio::test]
        async fn no_answer_in_time_is_closed() {
            let short = Duration::from_millis(50);
            assert_eq!(check(&Silent, address(), short).await, Egress::Closed);
        }
    }
}
