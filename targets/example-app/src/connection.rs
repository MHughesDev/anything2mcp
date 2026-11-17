//! A listener whose connections a request can cut, so that the fault "lost after
//! commit" can close a connection after the write is stored and before any answer
//! is sent (case 17).
//!
//! Each accepted connection carries a [`ConnectionHandle`]. The fault middleware
//! finds it in the request's extensions and cuts the connection: from then on the
//! connection refuses to write, so the answer the server produces is never sent and
//! the server closes the socket.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};

/// A handle on one connection, through which a request can cut it.
#[derive(Clone, Debug)]
pub(crate) struct ConnectionHandle {
    cut: Arc<AtomicBool>,
}

impl ConnectionHandle {
    /// Makes sure nothing more is ever written on this connection.
    pub(crate) fn cut(&self) {
        self.cut.store(true, Ordering::SeqCst);
    }
}

impl Connected<IncomingStream<'_, CuttableListener>> for ConnectionHandle {
    fn connect_info(stream: IncomingStream<'_, CuttableListener>) -> Self {
        stream.remote_addr().clone()
    }
}

/// A TCP listener whose connections can be cut.
pub(crate) struct CuttableListener(pub(crate) TcpListener);

impl Listener for CuttableListener {
    type Io = CuttableStream;
    type Addr = ConnectionHandle;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (stream, _) = Listener::accept(&mut self.0).await;
        let cut = Arc::new(AtomicBool::new(false));
        (
            CuttableStream {
                stream,
                cut: Arc::clone(&cut),
            },
            ConnectionHandle { cut },
        )
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        // The handle of a connection that does not exist; nothing reads it.
        let _: SocketAddr = self.0.local_addr()?;
        Ok(ConnectionHandle {
            cut: Arc::new(AtomicBool::new(false)),
        })
    }
}

/// A TCP stream that refuses every write once its connection has been cut.
pub(crate) struct CuttableStream {
    stream: TcpStream,
    cut: Arc<AtomicBool>,
}

impl CuttableStream {
    fn refuse_if_cut(&self) -> Result<(), io::Error> {
        if self.cut.load(Ordering::SeqCst) {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "the connection was cut by a fault",
            ))
        } else {
            Ok(())
        }
    }
}

impl AsyncRead for CuttableStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.cut.load(Ordering::SeqCst) {
            // Read as the end of the stream, so the server lets go of it.
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for CuttableStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Err(e) = self.refuse_if_cut() {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        if let Err(e) = self.refuse_if_cut() {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.stream).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.stream.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Err(e) = self.refuse_if_cut() {
            return Poll::Ready(Err(e));
        }
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
