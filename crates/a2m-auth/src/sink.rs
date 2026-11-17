//! Where a session's credentials are placed: the driver's own client.

use a2m_core::Secret;

/// A scheme in which the other side sends a challenge and the client answers
/// it with a credential. The provider holds the credential; the driver
/// carries out the scheme on its connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChallengeScheme {
    /// Digest access authentication.
    Digest,
    /// NTLM.
    Ntlm,
    /// Negotiate: Kerberos, falling back to NTLM where the two sides agree.
    Negotiate,
}

impl ChallengeScheme {
    /// The scheme's name as an auth file writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Digest => "digest",
            Self::Ntlm => "ntlm",
            Self::Negotiate => "negotiate",
        }
    }
}

/// The driver's client, as the place credentials are put. A pack implements
/// it over whatever its driver sends with: a request being built, a
/// connection, the environment of a program it is about to start.
///
/// Credentials go in and never come back: no method returns a value, and an
/// implementation must not keep one longer than the request or connection it
/// was placed into, nor write one to a log, a trace, or a capture. Every
/// secret passed here has already been registered with the redactor.
pub trait CredentialSink: Send {
    /// A named value sent with each request: a header, or what stands for
    /// one on this kind of connection.
    fn header(&mut self, name: &str, value: &Secret);

    /// A cookie for one origin.
    fn cookie(&mut self, origin: &str, name: &str, value: &Secret);

    /// The client's own certificate and its private key, for a connection
    /// that asks the client to prove who it is.
    fn tls_identity(&mut self, certificate: &[u8], key: &Secret);

    /// The credential for a challenge-and-response scheme, which the driver
    /// then carries out.
    fn challenge_credential(&mut self, scheme: ChallengeScheme, user: &str, secret: &Secret);
}
