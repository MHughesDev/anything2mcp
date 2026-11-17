//! The auth provider contract: sign in, check, recognize a sign of expiry,
//! renew, sign out, and place credentials into a driver's client.
//!
//! A provider is called only by the keeper's worker and by a lease's
//! `apply`. Nothing else may hold one for use.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use a2m_core::{Failure, FailureClass, FailureCode, Secret, Timestamp};
use a2m_vault::{BoxFuture, RandomSource, Vault};

use crate::credentials::Credentials;
use crate::failure;
use crate::key::{AuthEntry, SessionRef};
use crate::material::Material;
use crate::name::ProviderName;
use crate::person::{AskPerson, PersonAnswer, PersonAsk};
use crate::providers::totp;
use crate::sink::CredentialSink;

/// Two providers were given the same name.
pub const AUTH_PROVIDER_DUPLICATE: FailureCode = FailureCode::fixed("auth_provider_duplicate");

/// A rung of the renewal ladder (spec section 215). The keeper climbs it in
/// order and never repeats a rung within one renewal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rung {
    /// Renew without anyone: refresh the token, or exchange the stored
    /// credential again.
    Silent,
    /// The person approves a prompt or enters a code.
    LightPrompt,
    /// A full sign-in.
    FullSignIn,
}

impl Rung {
    /// The rung as the session timeline writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::LightPrompt => "light_prompt",
            Self::FullSignIn => "full_sign_in",
        }
    }
}

/// What a provider needs and can do. The keeper plans the renewal ladder
/// from it and never asks a provider for what it says it cannot do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderNeeds {
    /// Signing in needs the session's own person. False for a provider that
    /// works from stored credentials alone.
    pub person: bool,
    /// `refresh` can renew at the silent rung.
    pub silent_renewal: bool,
    /// `refresh` can renew at the light-prompt rung.
    pub light_prompt: bool,
    /// The material may be saved, and restored on another runner. False for
    /// a session bound to its device.
    pub movable: bool,
    /// A successful `sign_in` or `refresh` means the other side accepted the
    /// credential. False for a provider that only holds a value: whether it
    /// is accepted is known only when the system is used.
    pub confirms: bool,
}

/// The provider's own view of whether its material still signs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedIn {
    /// As far as the provider can tell, yes; until the moment given, when
    /// the credentials say when they run out.
    Yes {
        /// When the credentials themselves run out.
        until: Option<Timestamp>,
    },
    /// No: the credentials have run out.
    No,
}

/// Something a driver observed that may mean the session has ended. The
/// driver does not judge it; the session's provider does, through the lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawSign {
    /// The system answered with a status that refuses the caller, and
    /// perhaps a challenge that says what it wants instead.
    Status {
        /// The status, as the system's protocol numbers it.
        code: u16,
        /// The challenge that came with it, if any. Never a credential.
        challenge: Option<String>,
    },
    /// The system sent the client somewhere else to sign in.
    Redirect {
        /// The host it was sent to.
        host: String,
    },
    /// The credentials' own expiry has passed.
    PastExpiry,
}

/// A sign that a session has ended, as a driver reports it to the keeper
/// through the engine. The driver stops its step with `session_expired`; it
/// never renews.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpirySign {
    /// The system refused a credential it accepted before.
    Refused,
    /// The credential's own expiry has passed.
    PastExpiry,
    /// The system asked the client to sign in again.
    SignInRequested,
    /// The identity provider demands fresh claims from a person.
    FreshClaimsDemanded,
    /// A signal the target's auth file declares was seen.
    Declared,
}

impl ExpirySign {
    /// The sign as the session timeline writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Refused => "sign_refused",
            Self::PastExpiry => "sign_past_expiry",
            Self::SignInRequested => "sign_sign_in_requested",
            Self::FreshClaimsDemanded => "sign_fresh_claims_demanded",
            Self::Declared => "sign_declared",
        }
    }
}

/// What a provider is given to sign in, renew, or sign out: the session,
/// its auth file entry, the stored credentials by role, the time, random
/// bytes, and the way to the session's own person.
///
/// It gives out secrets only as [`Secret`], and every one it gives out is
/// registered with the redactor first.
pub struct SignInContext<'a> {
    pub(crate) session: &'a SessionRef,
    pub(crate) entry: &'a AuthEntry,
    pub(crate) credentials: &'a dyn Credentials,
    pub(crate) vault: &'a Vault,
    pub(crate) person: &'a dyn AskPerson,
    pub(crate) random: &'a dyn RandomSource,
    pub(crate) now: Timestamp,
    pub(crate) rung: Rung,
}

impl SignInContext<'_> {
    /// The session being signed in or renewed.
    #[must_use]
    pub fn session(&self) -> &SessionRef {
        self.session
    }

    /// The session's auth file entry.
    #[must_use]
    pub fn entry(&self) -> &AuthEntry {
        self.entry
    }

    /// The time of day, from the keeper's clock.
    #[must_use]
    pub fn now(&self) -> Timestamp {
        self.now
    }

    /// The rung of the renewal ladder this call is for. A sign-in is always
    /// [`Rung::FullSignIn`].
    #[must_use]
    pub fn rung(&self) -> Rung {
        self.rung
    }

    /// Whether the entry names a stored credential for `role`.
    #[must_use]
    pub fn has_credential(&self, role: &str) -> bool {
        self.entry.credentials.contains_key(role)
    }

    /// The stored credential the entry names for `role`, opened.
    ///
    /// # Errors
    ///
    /// `input`/`auth_settings_invalid` when the entry names none for the
    /// role; `input`/`credential_missing` when nothing is stored under the
    /// reference; the vault's failures when it cannot be opened.
    pub async fn credential(&self, role: &str) -> Result<Secret, Failure> {
        let reference = self.entry.credentials.get(role).ok_or_else(|| {
            failure::settings_invalid(
                self.entry.provider.as_str(),
                &format!("no credential is named for the role `{role}`"),
            )
        })?;
        let secret = self
            .credentials
            .open(&self.session.key, reference)
            .await?;
        self.vault.register(&secret);
        Ok(secret)
    }

    /// Registers a secret the provider made or received, such as a token an
    /// exchange returned or a header value it built. Call it before the
    /// secret goes anywhere else.
    pub fn register(&self, secret: &Secret) {
        self.vault.register(secret);
    }

    /// Fills `out` with random bytes, for a state value or a proof key.
    ///
    /// # Errors
    ///
    /// `busy`/`randomness_unavailable`.
    pub fn random(&self, out: &mut [u8]) -> Result<(), Failure> {
        self.random.fill(out)
    }

    /// Asks the session's own person, and waits for the answer. Whatever
    /// comes back is registered with the redactor.
    pub async fn ask_person(&self, ask: PersonAsk) -> PersonAnswer {
        let answer = self.person.ask(self.session, ask).await;
        if let PersonAnswer::Done(returned) = &answer {
            for value in returned.values() {
                self.vault.register(value);
            }
        }
        answer
    }

    /// Where a person's browser is sent back to after a link, if the
    /// surface can receive it.
    #[must_use]
    pub fn return_address(&self) -> Option<String> {
        self.person.return_address()
    }

    /// The one-time code for this moment, computed from the stored secret of
    /// the role `totp_secret` and the settings `totp_digits`, `totp_period`,
    /// and `totp_algorithm`. A provider that signs in a dedicated account
    /// enters it where the system asks for a code.
    ///
    /// # Errors
    ///
    /// As [`SignInContext::credential`]; `input`/`auth_settings_invalid`
    /// when a setting or the stored secret is not usable.
    pub async fn one_time_code(&self) -> Result<Secret, Failure> {
        let settings = totp::Settings::from_entry(self.entry)?;
        let secret = self.credential(totp::ROLE).await?;
        let code = totp::code(&secret, self.now, &settings)
            .map_err(|what| failure::settings_invalid(self.entry.provider.as_str(), what))?;
        self.vault.register(&code);
        Ok(code)
    }
}

impl fmt::Debug for SignInContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignInContext")
            .field("session", &self.session.session)
            .field("provider", &self.entry.provider.as_str())
            .field("rung", &self.rung)
            .finish_non_exhaustive()
    }
}

/// An auth provider: how to sign in to one kind of thing, check that the
/// sign-in holds, recognize a sign of expiry, renew, sign out, and place the
/// credentials into a driver's client.
///
/// A public contract. A provider that only holds or exchanges a credential
/// lives in this crate; one that needs a surface of the system lives in the
/// pack that owns the surface, and reaches that surface through handles the
/// pack gave it when it made the provider.
///
/// What an implementation must hold to:
///
/// - It is called only by the keeper. It never calls itself on a timer and
///   never renews unasked.
/// - A secret exists only as [`Secret`] inside [`Material`]. Every secret it
///   makes or receives is passed to [`SignInContext::register`] before it
///   goes anywhere else. No secret appears in a [`Failure`], a log, or a
///   `Debug`.
/// - A refusal of a stored credential is `session_expired`/`credential_refused`;
///   a demand for a person is `session_expired`/`needs_login` or `needs_mfa`.
///   The keeper reads those codes to climb the ladder, and tries nothing
///   twice.
/// - A challenge or a block met while signing in is a stop, `edge_blocked`,
///   never something to get past.
/// - A person is reached only through [`SignInContext::ask_person`]: a link
///   they open themself, or a code they enter. Nothing drives their browser.
pub trait AuthProvider: Send + Sync {
    /// The provider's name, as `auth.yaml` writes it.
    fn name(&self) -> ProviderName;

    /// What it needs and can do.
    fn needs(&self) -> ProviderNeeds;

    /// Checks an auth file entry without signing in: every setting it needs
    /// is there, and nothing it does not know.
    ///
    /// # Errors
    ///
    /// `input`/`auth_settings_invalid`.
    fn check(&self, entry: &AuthEntry) -> Result<(), Failure>;

    /// Signs in and returns the session's credentials.
    fn sign_in<'a>(&'a self, cx: &'a SignInContext<'a>)
    -> BoxFuture<'a, Result<Material, Failure>>;

    /// The provider's own view of whether `material` still signs in at
    /// `now`. It does not reach the system; the target's probe does that.
    fn is_signed_in<'a>(&'a self, material: &'a Material, now: Timestamp)
    -> BoxFuture<'a, SignedIn>;

    /// Whether something a driver observed is a sign that the session has
    /// ended. `None` when it is not one this provider recognizes.
    fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign>;

    /// Renews at the rung `cx.rung()`, from the credentials held, and
    /// returns the new ones. Called only for a rung [`AuthProvider::needs`]
    /// says it has.
    fn refresh<'a>(
        &'a self,
        material: &'a Material,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>>;

    /// Ends the sign-in on the other side, where there is a way to.
    fn sign_out<'a>(
        &'a self,
        material: &'a Material,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<(), Failure>>;

    /// Places the credentials into a driver's client. It returns nothing
    /// that holds one.
    ///
    /// # Errors
    ///
    /// `session_expired`/`needs_login` when the material lacks a part the
    /// provider needs.
    fn inject(&self, material: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure>;
}

/// The auth providers handed to the keeper, by name. `a2m-provider` collects
/// them from the packs; [`ProviderSet::neutral`] holds the ones built here.
#[derive(Clone, Default)]
pub struct ProviderSet {
    by_name: BTreeMap<ProviderName, Arc<dyn AuthProvider>>,
}

impl ProviderSet {
    /// No providers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The neutral providers this crate builds: the ones that only hold or
    /// exchange a credential.
    #[must_use]
    pub fn neutral() -> Self {
        let mut set = Self::new();
        for provider in crate::providers::neutral() {
            set.by_name.insert(provider.name(), provider);
        }
        set
    }

    /// Adds one provider.
    ///
    /// # Errors
    ///
    /// `input`/`auth_provider_duplicate` when the name is already taken.
    /// Which of two providers fills a name is never decided by order.
    pub fn with(mut self, provider: Arc<dyn AuthProvider>) -> Result<Self, Failure> {
        let name = provider.name();
        if self.by_name.contains_key(&name) {
            return Err(Failure::new(
                FailureClass::Input,
                AUTH_PROVIDER_DUPLICATE,
                format!("Two auth providers are named `{name}`."),
                "Enable only one provider of that name.",
            ));
        }
        self.by_name.insert(name, provider);
        Ok(self)
    }

    /// The provider of a name.
    ///
    /// # Errors
    ///
    /// `input`/`auth_provider_unknown`.
    pub fn get(&self, name: &ProviderName) -> Result<Arc<dyn AuthProvider>, Failure> {
        self.by_name
            .get(name)
            .cloned()
            .ok_or_else(|| failure::provider_unknown(name.as_str()))
    }

    /// The names of every provider, in order.
    #[must_use]
    pub fn names(&self) -> Vec<ProviderName> {
        self.by_name.keys().cloned().collect()
    }
}

impl fmt::Debug for ProviderSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.by_name.keys().map(ProviderName::as_str))
            .finish()
    }
}
