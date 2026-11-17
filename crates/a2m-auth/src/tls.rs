//! The one way an HTTP client of the product is made, and what it trusts.
//!
//! Every outbound HTTP client of the session keeper and of the packs is made
//! by [`client_builder`]. With the feature `tls` on, the builder carries a
//! ready TLS configuration that states its cryptography provider, ring, so
//! the HTTP client never asks for a provider installed for the whole process
//! and nothing here installs one. A crate that makes its own client in a
//! build with TLS on would stop when it is made; the source test in this
//! crate's tests holds the three product crates to this function.
//!
//! What a client trusts is a [`Trust`]: the roots a person named, checked
//! first, then the operating system's own verifier, or the bundled roots on
//! a machine that has none. TLS 1.2 is the lowest version spoken. Nothing
//! here, or anywhere, switches certificate checking off (decision 0157).
//!
//! The roots a person names come from the installation's setting
//! `network.trust_roots`, read when the engine starts, as the bytes of PEM
//! files the product crate read through the roots of files a person
//! allowed. No tool input and no target file reaches them.

use std::sync::OnceLock;

use a2m_core::{Failure, FailureClass, FailureCode};

/// `input`: the TLS configuration of this machine could not be made.
pub const TLS_SETUP_FAILED: FailureCode = FailureCode::fixed("tls_setup_failed");
/// `input`: a file of further trust roots holds no certificate, or one that
/// cannot be read.
pub const TRUST_ROOTS_UNREADABLE: FailureCode = FailureCode::fixed("trust_roots_unreadable");
/// `unknown_variant`: the certificate the system showed does not lead to an
/// authority this machine trusts.
pub const CERTIFICATE_UNTRUSTED: FailureCode = FailureCode::fixed("certificate_untrusted");
/// `unknown_variant`: the certificate is for another name than the address.
pub const CERTIFICATE_NAME_MISMATCH: FailureCode = FailureCode::fixed("certificate_name_mismatch");
/// `unknown_variant`: the certificate's time has run out.
pub const CERTIFICATE_EXPIRED: FailureCode = FailureCode::fixed("certificate_expired");
/// `unknown_variant`: the certificate was refused for another reason, or the
/// operating system's verifier refused it without saying which.
pub const CERTIFICATE_NOT_VERIFIED: FailureCode = FailureCode::fixed("certificate_not_verified");
/// `unknown_variant`: the TLS handshake failed for a reason that is not the
/// certificate: no version or cipher in common, or the far end ended it.
pub const TLS_HANDSHAKE_FAILED: FailureCode = FailureCode::fixed("tls_handshake_failed");

/// Every code of this module with its one class.
pub const CODES: &[(FailureClass, &str)] = &[
    (FailureClass::Input, "tls_setup_failed"),
    (FailureClass::Input, "trust_roots_unreadable"),
    (FailureClass::UnknownVariant, "certificate_untrusted"),
    (FailureClass::UnknownVariant, "certificate_name_mismatch"),
    (FailureClass::UnknownVariant, "certificate_expired"),
    (FailureClass::UnknownVariant, "certificate_not_verified"),
    (FailureClass::UnknownVariant, "tls_handshake_failed"),
];

/// Whether this build can make HTTPS requests.
#[must_use]
pub const fn enabled() -> bool {
    cfg!(feature = "tls")
}

/// Where the roots a [`Trust`] checks a certificate against come from,
/// beside the roots a person named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootsSource {
    /// The operating system's own certificate verifier.
    Platform,
    /// The roots bundled with the engine: the machine has none of its own.
    Bundled,
    /// None: this build makes no HTTPS requests.
    NotBuilt,
}

impl RootsSource {
    /// The name, as `doctor` and the logs show it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Bundled => "bundled",
            Self::NotBuilt => "not_built",
        }
    }
}

/// What an HTTP client trusts. Made once, when the engine starts, and shared
/// by every client; cloning it shares it.
#[derive(Clone)]
pub struct Trust {
    #[cfg(feature = "tls")]
    config: std::sync::Arc<rustls::ClientConfig>,
    source: RootsSource,
    named: usize,
}

impl std::fmt::Debug for Trust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Trust")
            .field("source", &self.source)
            .field("named", &self.named)
            .finish_non_exhaustive()
    }
}

impl Trust {
    /// The operating system's roots, and no others.
    ///
    /// # Errors
    ///
    /// `input`/`tls_setup_failed` when no TLS configuration can be made.
    pub fn system() -> Result<Self, Failure> {
        Self::with_named_roots(&[])
    }

    /// The operating system's roots and the certificates in `pem_files`:
    /// the bytes of each file a person named in `network.trust_roots`.
    ///
    /// # Errors
    ///
    /// `input`/`trust_roots_unreadable` for a file that holds no certificate
    /// or one that cannot be read; `input`/`tls_not_enabled` for named roots
    /// in a build without the feature `tls`; `input`/`tls_setup_failed` when
    /// no TLS configuration can be made.
    pub fn with_named_roots(pem_files: &[&[u8]]) -> Result<Self, Failure> {
        #[cfg(feature = "tls")]
        {
            built::trust(pem_files)
        }
        #[cfg(not(feature = "tls"))]
        {
            if pem_files.is_empty() {
                Ok(Self {
                    source: RootsSource::NotBuilt,
                    named: 0,
                })
            } else {
                Err(crate::failure::tls_not_enabled())
            }
        }
    }

    /// Where the roots beside the named ones come from.
    #[must_use]
    pub fn roots_source(&self) -> RootsSource {
        self.source
    }

    /// How many certificates a person named.
    #[must_use]
    pub fn named_roots(&self) -> usize {
        self.named
    }
}

static INSTALLED: OnceLock<Trust> = OnceLock::new();

/// Sets what every client of this process trusts. The product crate calls
/// it once, when the engine starts, before any client is made.
///
/// # Errors
///
/// `input`/`tls_setup_failed` when a trust was set already, or a client was
/// made already with the operating system's roots.
pub fn install(trust: Trust) -> Result<(), Failure> {
    INSTALLED.set(trust).map_err(|_| {
        setup_failed(
            "what the engine trusts was set already, when it started or by a client made before",
        )
    })
}

/// What clients of this process trust: what [`install`] set, or the
/// operating system's roots when nothing was set.
///
/// # Errors
///
/// `input`/`tls_setup_failed` when no TLS configuration can be made.
pub fn installed() -> Result<Trust, Failure> {
    if let Some(trust) = INSTALLED.get() {
        return Ok(trust.clone());
    }
    let trust = Trust::system()?;
    Ok(INSTALLED.get_or_init(|| trust).clone())
}

/// A client builder that trusts `trust`. Every HTTP client of the product
/// is made from one of these; with the feature `tls` on, it carries its own
/// TLS configuration, which states its provider, and an `https` request
/// needs nothing installed for the process.
pub fn client_builder(trust: &Trust) -> reqwest::ClientBuilder {
    #[cfg(feature = "tls")]
    {
        reqwest::Client::builder()
            .tls_backend_preconfigured(rustls::ClientConfig::clone(&trust.config))
    }
    #[cfg(not(feature = "tls"))]
    {
        let _ = trust;
        reqwest::Client::builder()
    }
}

/// The named failure for a request that failed while its connection was
/// made, when TLS is why: the certificate, or the handshake. `what` names
/// the far end, as "the identity service" or "the surface `api`". `None`
/// when the error is not a TLS one, or when the request was not stopped
/// while its connection was made: a TLS error after a request was sent is
/// a lost connection, and the caller says so.
#[must_use]
pub fn refusal(error: &reqwest::Error, what: &str) -> Option<Failure> {
    #[cfg(feature = "tls")]
    {
        if !error.is_connect() {
            return None;
        }
        built::found(error).map(|found| built::named(found, what))
    }
    #[cfg(not(feature = "tls"))]
    {
        let _ = (error, what);
        None
    }
}

fn setup_failed(why: &str) -> Failure {
    Failure::new(
        FailureClass::Input,
        TLS_SETUP_FAILED,
        format!("The TLS configuration of this engine could not be made: {why}."),
        "Run `anything2mcp doctor`, and report it if it says nothing more.",
    )
}

#[cfg(feature = "tls")]
mod built {
    use std::sync::Arc;

    use a2m_core::{Failure, FailureClass};
    use rustls::client::WebPkiServerVerifier;
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::WebPkiSupportedAlgorithms;
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{CertificateError, DigitallySignedStruct, RootCertStore, SignatureScheme};

    use super::{
        CERTIFICATE_EXPIRED, CERTIFICATE_NAME_MISMATCH, CERTIFICATE_NOT_VERIFIED,
        CERTIFICATE_UNTRUSTED, RootsSource, TLS_HANDSHAKE_FAILED, TRUST_ROOTS_UNREADABLE, Trust,
        setup_failed,
    };

    /// The most links of an error's chain that are looked through.
    const DEEPEST: usize = 16;

    /// Checks a certificate against the named roots first. A certificate
    /// that leads to one of them is judged there, by the same rules on every
    /// operating system; one that leads to none goes to the system's
    /// verifier. Neither is ever skipped.
    #[derive(Debug)]
    struct Verifier {
        named: Option<Arc<WebPkiServerVerifier>>,
        system: Arc<dyn ServerCertVerifier>,
        algorithms: WebPkiSupportedAlgorithms,
    }

    impl ServerCertVerifier for Verifier {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            intermediates: &[CertificateDer<'_>],
            server_name: &ServerName<'_>,
            ocsp_response: &[u8],
            now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            if let Some(named) = &self.named {
                match named.verify_server_cert(
                    end_entity,
                    intermediates,
                    server_name,
                    ocsp_response,
                    now,
                ) {
                    Err(rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer)) => {}
                    judged => return judged,
                }
            }
            self.system.verify_server_cert(
                end_entity,
                intermediates,
                server_name,
                ocsp_response,
                now,
            )
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.algorithms.supported_schemes()
        }
    }

    fn unreadable(why: &str) -> Failure {
        Failure::new(
            FailureClass::Input,
            TRUST_ROOTS_UNREADABLE,
            format!("A file named in `network.trust_roots` cannot be used: {why}."),
            "Name a PEM file that holds the certificate of the authority, and start the engine again.",
        )
    }

    pub(super) fn trust(pem_files: &[&[u8]]) -> Result<Trust, Failure> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());

        let mut named = RootCertStore::empty();
        for bytes in pem_files {
            let mut found = 0_usize;
            for certificate in CertificateDer::pem_slice_iter(bytes) {
                let certificate =
                    certificate.map_err(|_| unreadable("it is not a readable PEM file"))?;
                named
                    .add(certificate)
                    .map_err(|_| unreadable("a certificate in it cannot be read"))?;
                found += 1;
            }
            if found == 0 {
                return Err(unreadable("it holds no certificate"));
            }
        }
        let count = named.len();
        let named = if named.is_empty() {
            None
        } else {
            Some(
                WebPkiServerVerifier::builder_with_provider(Arc::new(named), Arc::clone(&provider))
                    .build()
                    .map_err(|_| unreadable("its certificates cannot be used as roots"))?,
            )
        };

        // The operating system's verifier, or, on a machine with no roots
        // of its own (a minimal container), the bundled ones.
        let (system, source): (Arc<dyn ServerCertVerifier>, RootsSource) =
            match rustls_platform_verifier::Verifier::new(Arc::clone(&provider)) {
                Ok(verifier) => (Arc::new(verifier), RootsSource::Platform),
                Err(_) => {
                    let bundled = RootCertStore {
                        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                    };
                    let verifier = WebPkiServerVerifier::builder_with_provider(
                        Arc::new(bundled),
                        Arc::clone(&provider),
                    )
                    .build()
                    .map_err(|_| setup_failed("the bundled roots cannot be used"))?;
                    (verifier, RootsSource::Bundled)
                }
            };

        let verifier = Verifier {
            named,
            system,
            algorithms: provider.signature_verification_algorithms,
        };
        // TLS 1.3 and 1.2, and nothing older: rustls speaks nothing older.
        let mut config = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .map_err(|_| setup_failed("the provider offers neither TLS 1.3 nor TLS 1.2"))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        // The HTTP client of this build speaks HTTP/1.1 only.
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Trust {
            config: Arc::new(config),
            source,
            named: count,
        })
    }

    /// The TLS error somewhere in an error's chain. An I/O error carries
    /// the one it wraps in its own field, which its `source` skips.
    pub(super) fn found(error: &reqwest::Error) -> Option<&rustls::Error> {
        fn walk<'a>(
            error: &'a (dyn std::error::Error + 'static),
            depth: usize,
        ) -> Option<&'a rustls::Error> {
            let mut next = Some(error);
            let mut depth = depth;
            while let Some(current) = next {
                if depth >= DEEPEST {
                    return None;
                }
                depth += 1;
                if let Some(found) = current.downcast_ref::<rustls::Error>() {
                    return Some(found);
                }
                if let Some(inner) = current
                    .downcast_ref::<std::io::Error>()
                    .and_then(std::io::Error::get_ref)
                    && let Some(found) = walk(inner, depth)
                {
                    return Some(found);
                }
                next = current.source();
            }
            None
        }
        walk(error, 0)
    }

    /// The failure a TLS error names. Nothing the far end wrote goes into it.
    pub(super) fn named(error: &rustls::Error, what: &str) -> Failure {
        let (code, why, next) = match error {
            rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer) => (
                CERTIFICATE_UNTRUSTED,
                "its certificate does not lead to an authority this machine trusts",
                "If the system uses an authority of your organization, name that authority's certificate in the setting `network.trust_roots`; otherwise do not connect to it.",
            ),
            rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            ) => (
                CERTIFICATE_NAME_MISMATCH,
                "its certificate is for another name than the address",
                "Check the address in the target's files: the certificate names another host.",
            ),
            rustls::Error::InvalidCertificate(
                CertificateError::Expired | CertificateError::ExpiredContext { .. },
            ) => (
                CERTIFICATE_EXPIRED,
                "its certificate has expired",
                "The system's operator must renew its certificate; check this machine's clock as well.",
            ),
            rustls::Error::InvalidCertificate(_) | rustls::Error::NoCertificatesPresented => (
                CERTIFICATE_NOT_VERIFIED,
                "its certificate was refused",
                "Check the system's certificate with its operator.",
            ),
            _ => (
                TLS_HANDSHAKE_FAILED,
                "the TLS handshake failed",
                "Check that the system offers TLS 1.2 or 1.3 at this address.",
            ),
        };
        Failure::new(
            FailureClass::UnknownVariant,
            code,
            format!("No connection was made to {what}: {why}. Nothing was sent."),
            next,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_are_as_declared() {
        for (class, code) in CODES {
            assert!(FailureCode::new(code).is_ok(), "{code}");
            assert_eq!(
                *class,
                if code.starts_with("certificate_") || *code == "tls_handshake_failed" {
                    FailureClass::UnknownVariant
                } else {
                    FailureClass::Input
                }
            );
        }
    }

    #[test]
    fn the_system_trust_is_made_and_a_client_built_from_it() {
        let trust = Trust::system().unwrap();
        assert_eq!(trust.named_roots(), 0);
        if enabled() {
            assert_ne!(trust.roots_source(), RootsSource::NotBuilt);
        } else {
            assert_eq!(trust.roots_source(), RootsSource::NotBuilt);
        }
        // With TLS on and no provider installed for the process, a client
        // is still made: its configuration states its own.
        assert!(client_builder(&trust).build().is_ok());
        assert!(client_builder(&installed().unwrap()).build().is_ok());
    }

    #[test]
    fn a_named_root_file_with_no_certificate_is_refused_by_name() {
        let failure = Trust::with_named_roots(&[b"not a certificate"]).unwrap_err();
        if enabled() {
            assert_eq!(failure.code(), &TRUST_ROOTS_UNREADABLE);
        } else {
            assert_eq!(failure.code(), &crate::failure::TLS_NOT_ENABLED);
        }
        assert_eq!(failure.class(), FailureClass::Input);
    }

    #[cfg(feature = "tls")]
    #[test]
    fn each_tls_error_is_named_by_what_went_wrong() {
        use rustls::CertificateError as C;
        let cases = [
            (
                rustls::Error::InvalidCertificate(C::UnknownIssuer),
                &CERTIFICATE_UNTRUSTED,
            ),
            (
                rustls::Error::InvalidCertificate(C::NotValidForName),
                &CERTIFICATE_NAME_MISMATCH,
            ),
            (
                rustls::Error::InvalidCertificate(C::Expired),
                &CERTIFICATE_EXPIRED,
            ),
            (
                rustls::Error::InvalidCertificate(C::Revoked),
                &CERTIFICATE_NOT_VERIFIED,
            ),
            (
                rustls::Error::General("PLANTED words of the far end".to_owned()),
                &TLS_HANDSHAKE_FAILED,
            ),
        ];
        for (error, code) in cases {
            let failure = built::named(&error, "the surface `api`");
            assert_eq!(failure.code(), code);
            assert_eq!(failure.class(), FailureClass::UnknownVariant);
            assert!(!failure.message().contains("PLANTED"));
        }
    }
}
