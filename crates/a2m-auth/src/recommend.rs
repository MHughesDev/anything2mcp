//! The fixed rules that turn sign-in findings into a recommended mode and
//! provider for `auth.detect` (decisions 0085, 0321, and 0367).
//!
//! The rules are a table, never a model (rule 1): one row for each sign a
//! discoverer names. A sign no row names is a question for a person, never
//! a guess (rule 7). What comes out is evidence for a person to confirm; it
//! is marked [`INFERRED`] and is never a settled method, and it holds no
//! credential and no text the system itself supplied.

use a2m_core::{Digest, Finding};
use serde_json::{Value as Json, json};

/// The kind of finding the rules read.
pub const SIGN_IN_METHOD: &str = "sign_in_method";

/// The evidence level a recommendation carries, always: a person confirms
/// it by signing in.
pub const INFERRED: &str = "inferred";

/// One sign of how a target signs in, as a discoverer names it in a
/// `sign_in_method` finding's `sign_in`. The names are the discoverers':
/// the web pack's four (`SIGN_IN_SIGNS` of `a2m-pack-web-probe`), the one a
/// move to an identity page with a `SAMLRequest` form post gives, and the
/// ones the HTTP pack reads from a description's security schemes. A test
/// of the product holds the web pack's list inside this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SignInSign {
    /// A page with a password field, on the target's own origin.
    PasswordForm,
    /// A page that asks for a one-time code.
    CodeStep,
    /// A move to an identity page on another origin.
    OtherOrigin,
    /// The move carried a `SAMLRequest` in a form post.
    SamlPost,
    /// An answer of status 401 that names no scheme.
    Challenge,
    /// A key placed in a header, a query, or a cookie.
    ApiKey,
    /// `Basic`.
    HttpBasic,
    /// `Digest`.
    HttpDigest,
    /// A bearer token held as it is.
    BearerToken,
    /// `Negotiate` or `NTLM`.
    Integrated,
    /// OAuth with a client's own credentials.
    ClientCredentials,
    /// OAuth with a person's consent, or OpenID Connect.
    AuthorizationCode,
    /// A client certificate.
    ClientCertificate,
}

impl SignInSign {
    /// Every sign, in the order the rules are tried and shown.
    pub const ALL: [Self; 13] = [
        Self::PasswordForm,
        Self::CodeStep,
        Self::OtherOrigin,
        Self::SamlPost,
        Self::Challenge,
        Self::ApiKey,
        Self::HttpBasic,
        Self::HttpDigest,
        Self::BearerToken,
        Self::Integrated,
        Self::ClientCredentials,
        Self::AuthorizationCode,
        Self::ClientCertificate,
    ];

    /// The name a finding holds.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::PasswordForm => "password_form",
            Self::CodeStep => "code_step",
            Self::OtherOrigin => "other_origin",
            Self::SamlPost => "saml_post",
            Self::Challenge => "challenge",
            Self::ApiKey => "api_key",
            Self::HttpBasic => "http_basic",
            Self::HttpDigest => "http_digest",
            Self::BearerToken => "bearer_token",
            Self::Integrated => "integrated",
            Self::ClientCredentials => "client_credentials",
            Self::AuthorizationCode => "authorization_code",
            Self::ClientCertificate => "client_certificate",
        }
    }

    /// The sign a name stands for, if it is one.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sign| sign.name() == name)
    }

    /// The row of the table for this sign: the mode, the provider, other
    /// providers that suit it, and the reason, all fixed text.
    const fn row(self) -> Row {
        match self {
            Self::PasswordForm => Row::Chosen {
                mode: "managed_session",
                provider: "form_login",
                also: &["window_login"],
                reason: "The start page asks for a password on the target's own origin. `form_login` signs in on that page with a stored credential; `window_login` leaves the typing to a person in a window of the engine's own browser.",
            },
            Self::CodeStep => Row::Chosen {
                mode: "managed_session",
                provider: "window_login",
                also: &[],
                reason: "The page asks for a one-time code. The engine never answers a second factor (decision 0321), so a person signs in, in a window of the engine's own browser.",
            },
            Self::OtherOrigin => Row::Chosen {
                mode: "managed_session",
                provider: "sso_oidc",
                also: &["window_login"],
                reason: "The start page moves to an identity page on another origin. OpenID Connect and SAML look alike from outside, so this is reported as `sso_oidc` unless a `SAMLRequest` form post was seen.",
            },
            Self::SamlPost => Row::Chosen {
                mode: "managed_session",
                provider: "sso_saml",
                also: &["window_login"],
                reason: "The move to the identity page carried a `SAMLRequest` in a form post.",
            },
            Self::Challenge => Row::Question(
                "The surface answered with status 401 and no scheme the rules know. Say which scheme it uses (Basic, Digest, a bearer token, or another), or declare it in `auth.yaml`.",
            ),
            Self::ApiKey => Row::Chosen {
                mode: "custom",
                provider: "api_key",
                also: &[],
                reason: "The description declares an API key. A dedicated key, stored as a vault reference, is placed in the header, query, or cookie the description names.",
            },
            Self::HttpBasic => Row::Chosen {
                mode: "custom",
                provider: "http_basic",
                also: &[],
                reason: "The description declares HTTP Basic. A dedicated account's name and password are stored as vault references.",
            },
            Self::HttpDigest => Row::Chosen {
                mode: "custom",
                provider: "http_digest",
                also: &[],
                reason: "The description declares HTTP Digest. A dedicated account's name and password are stored as vault references.",
            },
            Self::BearerToken => Row::Chosen {
                mode: "custom",
                provider: "bearer_static",
                also: &[],
                reason: "The description declares a bearer token. A token issued for this use is stored as a vault reference.",
            },
            Self::Integrated => Row::Chosen {
                mode: "custom",
                provider: "windows_integrated",
                also: &[],
                reason: "The description declares Negotiate or NTLM. The driver answers the challenge with a dedicated account.",
            },
            Self::ClientCredentials => Row::Chosen {
                mode: "custom",
                provider: "oauth_client_credentials",
                also: &[],
                reason: "The description declares OAuth with a client's own credentials. The client identifier and secret are stored as vault references, and the token endpoint is declared as a destination.",
            },
            Self::AuthorizationCode => Row::Chosen {
                mode: "managed_session",
                provider: "oauth_authorization_code",
                also: &[],
                reason: "The description declares OAuth with a person's consent, or OpenID Connect. A person agrees through a link they open, and the tokens carry their own rights.",
            },
            Self::ClientCertificate => Row::Chosen {
                mode: "custom",
                provider: "client_certificate",
                also: &[],
                reason: "The description declares mutual TLS. A dedicated certificate and key are stored as vault references.",
            },
        }
    }
}

/// One row of the table.
enum Row {
    Chosen {
        mode: &'static str,
        provider: &'static str,
        also: &'static [&'static str],
        reason: &'static str,
    },
    Question(&'static str),
}

/// The question put when a finding names a sign no row holds.
const UNRECOGNIZED: &str = "A discoverer named a sign in method these rules have no row for. Say how the target signs in, or declare it in `auth.yaml`.";

/// The question put when no finding says whether a second factor follows.
const SECOND_FACTOR: &str = "Whether a second factor, a passkey, or a link sent by mail follows the sign-in does not show before one is sent. A person says which it is.";

/// One recommended way of signing in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The sign that led to it.
    pub sign: SignInSign,
    /// The mode of `auth.yaml`.
    pub mode: &'static str,
    /// The provider of `auth.yaml`.
    pub provider: &'static str,
    /// Other providers that also suit the sign.
    pub also: &'static [&'static str],
    /// Why, in fixed words.
    pub reason: &'static str,
}

/// What the rules recommend for a set of findings. It is evidence for a
/// person to confirm, marked `inferred`; a drafter keeps what the target
/// declares and uses this only where nothing is declared (decision 0303).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Recommendation {
    /// One choice for each sign found, in the order of the table.
    pub choices: Vec<Choice>,
    /// What a person has to say, as questions.
    pub questions: Vec<&'static str>,
    /// What is not known, by name.
    pub unknown: Vec<&'static str>,
    /// The findings the recommendation rests on.
    pub from: Vec<Digest>,
}

impl Recommendation {
    /// The first choice, when there is one.
    #[must_use]
    pub fn first(&self) -> Option<&Choice> {
        self.choices.first()
    }

    /// The recommendation as the answer of `auth.detect` shows it.
    #[must_use]
    pub fn to_json(&self) -> Json {
        let first = self.first();
        json!({
            "level": INFERRED,
            "mode": first.map(|choice| choice.mode),
            "provider": first.map(|choice| choice.provider),
            "choices": self.choices.iter().map(|choice| json!({
                "sign": choice.sign.name(),
                "mode": choice.mode,
                "provider": choice.provider,
                "also": choice.also,
                "reason": choice.reason,
            })).collect::<Vec<_>>(),
            "questions": self.questions,
            "unknown": self.unknown,
            "from": self.from.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "confirm": "Evidence for a person to confirm by signing in; what the target's own files declare is kept as declared.",
        })
    }
}

/// The fixed rules: findings in, a recommendation out. Findings of another
/// kind are ignored. The same findings in any order give the same answer.
#[must_use]
pub fn recommend(findings: &[Finding]) -> Recommendation {
    let mut signs = std::collections::BTreeSet::new();
    let mut second_factor_asked = false;
    let mut unrecognized = false;
    let mut from = Vec::new();
    for finding in findings {
        if finding.kind().as_str() != SIGN_IN_METHOD {
            continue;
        }
        let Ok(found) = serde_json::from_str::<Json>(finding.found()) else {
            unrecognized = true;
            continue;
        };
        from.push(finding.id());
        match found["sign_in"].as_str().and_then(SignInSign::parse) {
            Some(sign) => {
                signs.insert(sign);
            }
            None => unrecognized = true,
        }
        if found["second_factor"].as_str() == Some("asked") {
            second_factor_asked = true;
        }
    }
    from.sort();
    from.dedup();
    let mut out = Recommendation {
        from,
        ..Recommendation::default()
    };
    for sign in signs {
        match sign.row() {
            Row::Chosen {
                mode,
                provider,
                also,
                reason,
            } => out.choices.push(Choice {
                sign,
                mode,
                provider,
                also,
                reason,
            }),
            Row::Question(question) => out.questions.push(question),
        }
    }
    if unrecognized {
        out.questions.push(UNRECOGNIZED);
        out.unknown.push("method");
    }
    if out.choices.is_empty() {
        out.unknown.push("identity_mode");
        if !unrecognized && out.questions.is_empty() {
            out.unknown.push("method");
        }
    }
    if !second_factor_asked && out.choices.iter().any(|choice| is_web(choice.sign)) {
        out.unknown.push("second_factor");
        out.questions.push(SECOND_FACTOR);
    }
    out
}

const fn is_web(sign: SignInSign) -> bool {
    matches!(
        sign,
        SignInSign::PasswordForm | SignInSign::OtherOrigin | SignInSign::SamlPost
    )
}
