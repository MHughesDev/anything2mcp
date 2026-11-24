//! The application served over TLS, behind the feature `tls`.
//!
//! A [`TestAuthority`] is made when a test runs, and never written to disk
//! or committed: a committed certificate would expire. It signs the
//! server's certificate in one of the shapes a test needs, a good one and
//! two made wrong on purpose. A test trusts the authority through the
//! engine's own setting for further roots, by the PEM text [`TestAuthority::pem`]
//! gives; a second authority that nobody trusts stands for an unknown one.
//!
//! The TLS provider is ring, stated in each configuration; nothing here
//! installs one for the process.

use std::fmt;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, date_time_ymd,
};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream;

use crate::connection::{ConnectionHandle, CuttableListener, CuttableStream};

/// The longest a TLS handshake may take before the connection is dropped.
const HANDSHAKE_LIMIT: Duration = Duration::from_secs(10);

/// Why a certificate or a TLS configuration could not be made.
#[derive(Debug)]
pub struct TlsError(String);

impl fmt::Display for TlsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the test stand's TLS could not be set up: {}", self.0)
    }
}

impl std::error::Error for TlsError {}

fn err(what: &str) -> impl Fn(rcgen::Error) -> TlsError + '_ {
    move |e| TlsError(format!("{what}: {e}"))
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The certificate the server shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerCertificate {
    /// For `127.0.0.1` and `localhost`, valid from 2000 to 4000.
    Good,
    /// For `other.example.test` only.
    WrongName,
    /// For `127.0.0.1`, valid in the year 2000 only.
    Expired,
}

/// A certificate authority made for one test run, trusted by nobody until a
/// test names its certificate.
pub struct TestAuthority {
    issuer: Issuer<'static, KeyPair>,
    der: CertificateDer<'static>,
    pem: String,
}

impl fmt::Debug for TestAuthority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestAuthority").finish_non_exhaustive()
    }
}

impl TestAuthority {
    /// A new authority with a fresh key, named `name`.
    ///
    /// # Errors
    ///
    /// Returns an error if its key or certificate could not be made.
    pub fn new(name: &str) -> Result<Self, TlsError> {
        let mut params =
            CertificateParams::new(Vec::<String>::new()).map_err(err("the authority's names"))?;
        params.distinguished_name.push(DnType::CommonName, name);
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params.not_before = date_time_ymd(2000, 1, 1);
        params.not_after = date_time_ymd(4000, 1, 1);
        let key = KeyPair::generate().map_err(err("the authority's key"))?;
        let certificate = params
            .self_signed(&key)
            .map_err(err("the authority's certificate"))?;
        Ok(Self {
            der: certificate.der().clone(),
            pem: certificate.pem(),
            issuer: Issuer::new(params, key),
        })
    }

    /// The authority's certificate as a PEM file holds it.
    #[must_use]
    pub fn pem(&self) -> &str {
        &self.pem
    }

    /// The authority's certificate.
    #[must_use]
    pub fn der(&self) -> CertificateDer<'static> {
        self.der.clone()
    }

    /// A server configuration that shows a certificate of the shape asked
    /// for, signed by this authority. TLS 1.3 and 1.2.
    ///
    /// # Errors
    ///
    /// Returns an error if the certificate or the configuration could not
    /// be made.
    pub fn server(&self, shape: ServerCertificate) -> Result<Arc<ServerConfig>, TlsError> {
        let names: Vec<String> = match shape {
            ServerCertificate::Good => vec!["127.0.0.1".into(), "localhost".into()],
            ServerCertificate::WrongName => vec!["other.example.test".into()],
            ServerCertificate::Expired => vec!["127.0.0.1".into()],
        };
        let mut params = CertificateParams::new(names).map_err(err("the server's names"))?;
        params
            .distinguished_name
            .push(DnType::CommonName, "example-app");
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        let (from, to) = match shape {
            ServerCertificate::Expired => (date_time_ymd(2000, 1, 1), date_time_ymd(2000, 12, 31)),
            _ => (date_time_ymd(2000, 1, 1), date_time_ymd(4000, 1, 1)),
        };
        params.not_before = from;
        params.not_after = to;
        let key = KeyPair::generate().map_err(err("the server's key"))?;
        let certificate = params
            .signed_by(&key, &self.issuer)
            .map_err(err("the server's certificate"))?;
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let mut config = ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .map_err(|e| TlsError(format!("the server's versions: {e}")))?
            .with_no_client_auth()
            .with_single_cert(vec![certificate.der().clone(), self.der()], key)
            .map_err(|e| TlsError(format!("the server's configuration: {e}")))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

/// A listener that completes a TLS handshake on each connection before it
/// hands the connection on. A connection whose handshake fails is dropped,
/// and the next one is taken. Its connections can be cut, as plain ones can.
pub(crate) struct TlsListener {
    inner: CuttableListener,
    acceptor: TlsAcceptor,
}

impl Listener for TlsListener {
    type Io = TlsStream<CuttableStream>;
    type Addr = ConnectionHandle;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            let (stream, handle) = Listener::accept(&mut self.inner).await;
            if let Ok(Ok(stream)) =
                tokio::time::timeout(HANDSHAKE_LIMIT, self.acceptor.accept(stream)).await
            {
                return (stream, handle);
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

impl Connected<IncomingStream<'_, TlsListener>> for ConnectionHandle {
    fn connect_info(stream: IncomingStream<'_, TlsListener>) -> Self {
        stream.remote_addr().clone()
    }
}

/// Serves one new application on two listeners: `plain` over HTTP and
/// `secure` over TLS with `config`, until the task is stopped.
///
/// # Errors
///
/// Returns the error that stopped either server.
pub async fn serve_both(
    plain: TcpListener,
    secure: TcpListener,
    config: Arc<ServerConfig>,
) -> io::Result<()> {
    let router = crate::router();
    let over_tls = TlsListener {
        inner: CuttableListener(secure),
        acceptor: TlsAcceptor::from(config),
    };
    let plain = axum::serve(
        CuttableListener(plain),
        router
            .clone()
            .into_make_service_with_connect_info::<ConnectionHandle>(),
    );
    let secure = axum::serve(
        over_tls,
        router.into_make_service_with_connect_info::<ConnectionHandle>(),
    );
    tokio::try_join!(async { plain.await }, async { secure.await }).map(|_| ())
}

/// A TLS front for another stand: it completes a TLS handshake with
/// `config` on each connection and copies the bytes to and from `upstream`,
/// a plain loopback address, so that a stand that speaks only plain HTTP can
/// be reached at an `https` address. Stopped when dropped.
#[derive(Debug)]
pub struct TlsFront {
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl TlsFront {
    /// Starts a front on a free local port.
    ///
    /// # Errors
    ///
    /// Returns an error if no port could be bound.
    pub async fn start(
        config: Arc<ServerConfig>,
        upstream: std::net::SocketAddr,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(config);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let Ok(Ok(mut secure)) =
                        tokio::time::timeout(HANDSHAKE_LIMIT, acceptor.accept(stream)).await
                    else {
                        return;
                    };
                    let Ok(mut plain) = tokio::net::TcpStream::connect(upstream).await else {
                        return;
                    };
                    let _ = tokio::io::copy_bidirectional(&mut secure, &mut plain).await;
                });
            }
        });
        Ok(Self { address, task })
    }

    /// The front's address, as `127.0.0.1:<port>`.
    #[must_use]
    pub fn address(&self) -> std::net::SocketAddr {
        self.address
    }
}

impl Drop for TlsFront {
    fn drop(&mut self) {
        self.task.abort();
    }
}
