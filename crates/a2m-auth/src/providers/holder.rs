//! The providers that only hold a stored credential and place it.

use a2m_core::{Failure, Secret, Timestamp};
use a2m_vault::BoxFuture;

use super::{header_safe, part, refused_status};
use crate::encode::base64;
use crate::failure;
use crate::key::AuthEntry;
use crate::material::Material;
use crate::name::ProviderName;
use crate::provider::{AuthProvider, ExpirySign, ProviderNeeds, RawSign, SignInContext, SignedIn};
use crate::sink::{ChallengeScheme, CredentialSink};

/// Which stored credential a holder places, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    ApiKey,
    BearerStatic,
    HttpBasic,
    HttpDigest,
    WindowsIntegrated,
    ClientCertificate,
}

impl Kind {
    pub(crate) const ALL: [Self; 6] = [
        Self::ApiKey,
        Self::BearerStatic,
        Self::HttpBasic,
        Self::HttpDigest,
        Self::WindowsIntegrated,
        Self::ClientCertificate,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::BearerStatic => "bearer_static",
            Self::HttpBasic => "http_basic",
            Self::HttpDigest => "http_digest",
            Self::WindowsIntegrated => "windows_integrated",
            Self::ClientCertificate => "client_certificate",
        }
    }

    /// The settings the provider knows, and the credential roles it needs.
    const fn shape(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Self::ApiKey => (&["header", "prefix"], &["key"]),
            Self::BearerStatic => (&[], &["token"]),
            Self::HttpBasic | Self::HttpDigest => (&["user"], &["password"]),
            Self::WindowsIntegrated => (&["user", "scheme"], &["password"]),
            Self::ClientCertificate => (&[], &["certificate", "key"]),
        }
    }
}

/// A provider that holds a stored credential. Signing in opens the
/// credential; nothing is exchanged, so whether the system accepts it is
/// known only when the system is used.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Holder {
    kind: Kind,
}

impl Holder {
    pub(crate) const fn new(kind: Kind) -> Self {
        Self { kind }
    }

    fn scheme(entry: &AuthEntry) -> Result<ChallengeScheme, Failure> {
        match entry.optional("scheme") {
            None | Some("negotiate") => Ok(ChallengeScheme::Negotiate),
            Some("ntlm") => Ok(ChallengeScheme::Ntlm),
            Some(_) => Err(failure::settings_invalid(
                entry.provider.as_str(),
                "the setting `scheme` is neither `negotiate` nor `ntlm`",
            )),
        }
    }
}

impl AuthProvider for Holder {
    fn name(&self) -> ProviderName {
        ProviderName::fixed(self.kind.name())
    }

    fn needs(&self) -> ProviderNeeds {
        ProviderNeeds {
            person: false,
            silent_renewal: false,
            light_prompt: false,
            movable: true,
            confirms: false,
        }
    }

    fn check(&self, entry: &AuthEntry) -> Result<(), Failure> {
        let name = self.kind.name();
        let (settings, roles) = self.kind.shape();
        entry.only(settings, roles)?;
        for role in roles {
            if !entry.credentials.contains_key(*role) {
                return Err(failure::settings_invalid(
                    name,
                    &format!("no credential is named for the role `{role}`"),
                ));
            }
        }
        match self.kind {
            Kind::ApiKey => {
                let header = entry.required("header")?;
                let token = header
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
                if !token {
                    return Err(failure::settings_invalid(
                        name,
                        "the setting `header` is not a header's name",
                    ));
                }
                if !header_safe(entry.optional("prefix").unwrap_or_default()) {
                    return Err(failure::settings_invalid(
                        name,
                        "the setting `prefix` holds a character a header cannot carry",
                    ));
                }
            }
            Kind::HttpBasic | Kind::HttpDigest | Kind::WindowsIntegrated => {
                let user = entry.required("user")?;
                let basic = self.kind == Kind::HttpBasic;
                if !header_safe(user) || (basic && user.contains(':')) {
                    return Err(failure::settings_invalid(
                        name,
                        "the setting `user` holds a character the scheme cannot carry",
                    ));
                }
                if self.kind == Kind::WindowsIntegrated {
                    Self::scheme(entry)?;
                }
            }
            Kind::BearerStatic | Kind::ClientCertificate => {}
        }
        Ok(())
    }

    fn sign_in<'a>(
        &'a self,
        cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(async move {
            let entry = cx.entry();
            let (settings, roles) = self.kind.shape();
            let mut material = Material::new(self.name());
            for role in roles {
                material = material.with_secret(role, cx.credential(role).await?);
            }
            for setting in settings {
                if let Some(value) = entry.optional(setting) {
                    material = material.with_plain(setting, value);
                }
            }
            Ok(material)
        })
    }

    fn is_signed_in<'a>(
        &'a self,
        _material: &'a Material,
        _now: Timestamp,
    ) -> BoxFuture<'a, SignedIn> {
        Box::pin(async { SignedIn::Yes { until: None } })
    }

    fn detect_expiry(&self, sign: &RawSign) -> Option<ExpirySign> {
        refused_status(sign)
    }

    fn refresh<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<Material, Failure>> {
        Box::pin(async {
            Err(failure::needs_login(
                "a held credential has nothing to renew",
            ))
        })
    }

    fn sign_out<'a>(
        &'a self,
        _material: &'a Material,
        _cx: &'a SignInContext<'a>,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        Box::pin(async { Ok(()) })
    }

    fn inject(&self, material: &Material, sink: &mut dyn CredentialSink) -> Result<(), Failure> {
        let plain = |name: &str| {
            material
                .plain(name)
                .ok_or_else(|| failure::needs_login("its saved credentials are incomplete"))
        };
        match self.kind {
            Kind::ApiKey => {
                let key = part(material, "key")?;
                let prefix = material.plain("prefix").unwrap_or_default();
                let value = Secret::new(format!("{prefix}{}", key.expose()));
                sink.header(plain("header")?, &value);
            }
            Kind::BearerStatic => {
                let token = part(material, "token")?;
                sink.header(
                    "Authorization",
                    &Secret::new(format!("Bearer {}", token.expose())),
                );
            }
            Kind::HttpBasic => {
                let password = part(material, "password")?;
                let pair = Secret::new(format!("{}:{}", plain("user")?, password.expose()));
                let value = Secret::new(format!("Basic {}", base64(pair.expose().as_bytes())));
                sink.header("Authorization", &value);
            }
            Kind::HttpDigest => {
                sink.challenge_credential(
                    ChallengeScheme::Digest,
                    plain("user")?,
                    part(material, "password")?,
                );
            }
            Kind::WindowsIntegrated => {
                let scheme = match material.plain("scheme") {
                    Some("ntlm") => ChallengeScheme::Ntlm,
                    _ => ChallengeScheme::Negotiate,
                };
                sink.challenge_credential(scheme, plain("user")?, part(material, "password")?);
            }
            Kind::ClientCertificate => {
                let certificate = part(material, "certificate")?;
                sink.tls_identity(certificate.expose().as_bytes(), part(material, "key")?);
            }
        }
        Ok(())
    }
}
