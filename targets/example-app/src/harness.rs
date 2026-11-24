//! The ground-truth harness: starts the application for a test and asks it,
//! through the control interface, what it actually stored.
//!
//! A test never proves a write using Anything2MCP's own read path alone. It asks here.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::auth::{AuthReport, AuthSettings, Credential};
use crate::details::StoredDelivery;
use crate::faults::Fault;
use crate::forgery::NotesReport;
use crate::store::{StoredItem, StoredOrder, WriteRecord};

/// The one account's username. Made up.
pub const USERNAME: &str = "example-user";
/// The one account's password. Made up.
pub const PASSWORD: &str = "secret-for-tests-password";
/// The one-time code asked for when the second factor is on. Fixed, and made up.
pub const SECOND_FACTOR_CODE: &str = "246810";
/// The bearer token the routes under `/secure/bearer` take. Made up.
pub const BEARER_TOKEN: &str = "example-token-for-tests-1";
/// The API key the routes under `/secure/key` take, in [`API_KEY_HEADER`]. Made up.
pub const API_KEY: &str = "example-key-for-tests-1";
/// The header that carries the API key.
pub const API_KEY_HEADER: &str = "x-api-key";
/// The cookie that carries a session.
pub const SESSION_COOKIE: &str = "example_session";
/// The cookie that carries a sign-in waiting for its one-time code.
pub const PENDING_COOKIE: &str = "example_pending";
/// Failed sign-ins in a row after which the account is locked until a reset.
pub const LOCKOUT_AFTER: u64 = 5;

/// Why the harness could not do what a test asked.
#[derive(Debug)]
pub enum HarnessError {
    /// The application could not be started.
    Start(std::io::Error),
    /// A request to the control interface failed.
    Request(reqwest::Error),
    /// The control interface answered with a status that is not a success.
    Status(u16),
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(e) => write!(f, "the test application could not be started: {e}"),
            Self::Request(e) => write!(f, "a request to the control interface failed: {e}"),
            Self::Status(status) => {
                write!(f, "the control interface answered with status {status}")
            }
        }
    }
}

impl std::error::Error for HarnessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Start(e) => Some(e),
            Self::Request(e) => Some(e),
            Self::Status(_) => None,
        }
    }
}

impl From<reqwest::Error> for HarnessError {
    fn from(e: reqwest::Error) -> Self {
        Self::Request(e)
    }
}

#[derive(Deserialize)]
struct Stored<T> {
    stored: Option<T>,
}

#[derive(Deserialize)]
struct Writes {
    writes: Vec<WriteRecord>,
}

#[derive(Deserialize)]
struct DeliveryWrites {
    writes: u64,
}

#[derive(Deserialize)]
struct Seen {
    seen: Vec<String>,
}

#[derive(Deserialize)]
struct CaseEvents {
    events: Vec<String>,
}

#[derive(Deserialize)]
struct TrapWrites {
    writes: BTreeMap<String, u64>,
}

#[derive(Deserialize)]
struct Faults {
    faults: Vec<Fault>,
}

/// One running test application, on its own port, stopped when dropped.
#[derive(Debug)]
pub struct TestApp {
    base: String,
    /// The identity provider's address, when it was started with
    /// [`TestApp::start_with_identity_provider`].
    provider: Option<String>,
    /// The `https` address of the same application, when it was started
    /// with [`TestApp::start_tls`].
    #[cfg(feature = "tls")]
    secure: Option<String>,
    http: reqwest::Client,
    server: JoinHandle<()>,
}

/// A client builder for the harness's own requests, and for a test that
/// acts as a user over plain HTTP. Wherever `reqwest` has TLS, its TLS
/// configuration states its provider and trusts nothing, so that a client is
/// made without a provider installed for the process. That holds whatever
/// switched TLS on in `reqwest`: this stand's feature `tls`, or another
/// crate of the same build, such as `a2m-auth` with its own `tls`, while
/// this stand's is off.
pub fn client_builder() -> reqwest::ClientBuilder {
    // With no configuration, reqwest is handed something it does not know,
    // and `build` returns an error instead of stopping the process.
    let builder = match plain_client_config() {
        Some(config) => reqwest::Client::builder().tls_backend_preconfigured(config),
        None => reqwest::Client::builder().tls_backend_preconfigured(()),
    };
    // No client of the workspace takes a proxy from the environment
    // (decision 0197): the harness asks the application on this machine.
    builder.no_proxy()
}

/// The stand-in for `reqwest::ClientBuilder::tls_backend_preconfigured` in
/// a build whose `reqwest` has no TLS, where there is nothing to configure.
/// A method of the type itself is always chosen before a method of a trait,
/// so where `reqwest` has TLS the call in [`client_builder`] is `reqwest`'s
/// own, which states the provider, and this one is never called. Which of
/// the two it is follows the features the build unified in `reqwest`, which
/// this stand's own features cannot see.
#[allow(
    dead_code,
    reason = "used only in a build whose reqwest has no TLS; see above"
)]
trait NoTlsInReqwest {
    fn tls_backend_preconfigured(self, tls: impl std::any::Any) -> Self;
}

impl NoTlsInReqwest for reqwest::ClientBuilder {
    fn tls_backend_preconfigured(self, _tls: impl std::any::Any) -> Self {
        self
    }
}

/// The harness's own client configuration: ring, stated, and no roots. The
/// harness speaks plain HTTP only. `None` if ring offered no version, which
/// it always does.
fn plain_client_config() -> Option<rustls::ClientConfig> {
    rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
    .ok()
    .map(|builder| {
        builder
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth()
    })
}

impl TestApp {
    /// Starts a new application with the seed data on a free local port.
    ///
    /// # Errors
    ///
    /// Returns an error if no port could be bound.
    pub async fn start() -> Result<Self, HarnessError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(HarnessError::Start)?;
        let address = listener.local_addr().map_err(HarnessError::Start)?;
        let server = tokio::spawn(async move {
            // The task is aborted when the `TestApp` is dropped; an error here
            // shows up in the test as a failed request.
            let _ = crate::serve(listener).await;
        });
        Ok(Self {
            base: format!("http://{address}"),
            provider: None,
            #[cfg(feature = "tls")]
            secure: None,
            http: client_builder().build()?,
            server,
        })
    }

    /// Starts a new application and an imaginary identity provider beside it,
    /// on a second free local port: the application at [`TestApp::url`], under
    /// the name `127.0.0.1`, and the provider at [`TestApp::provider_url`],
    /// under the name `localhost`. They share one state, so that a code the
    /// provider issues is the one the application takes back.
    ///
    /// # Errors
    ///
    /// Returns an error if a port could not be bound.
    pub async fn start_with_identity_provider() -> Result<Self, HarnessError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(HarnessError::Start)?;
        let provider = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(HarnessError::Start)?;
        let address = listener.local_addr().map_err(HarnessError::Start)?;
        let provider_port = provider.local_addr().map_err(HarnessError::Start)?.port();
        let base = format!("http://{address}");
        let provider_base = format!("http://localhost:{provider_port}");
        let state = crate::AppState::default();
        {
            let mut inner = state.lock().await;
            inner.auth.sso.application = Some(base.clone());
            inner.auth.sso.provider = Some(provider_base.clone());
        }
        let shared = state.clone();
        let server = tokio::spawn(async move {
            let _ = tokio::join!(
                crate::serve_over(listener, state),
                crate::serve_provider(provider, shared)
            );
        });
        Ok(Self {
            base,
            provider: Some(provider_base),
            #[cfg(feature = "tls")]
            secure: None,
            http: client_builder().build()?,
            server,
        })
    }

    /// The address of a path at the identity provider, for an application
    /// started with [`TestApp::start_with_identity_provider`].
    #[must_use]
    pub fn provider_url(&self, path: &str) -> Option<String> {
        self.provider.as_ref().map(|base| format!("{base}{path}"))
    }

    /// Starts a new application that is served on two free local ports: over
    /// TLS, with `server`'s certificate, at [`TestApp::secure_url`], and over
    /// plain HTTP at [`TestApp::url`], where the harness asks what it stored.
    /// Both serve one application with one store.
    ///
    /// # Errors
    ///
    /// Returns an error if a port could not be bound.
    #[cfg(feature = "tls")]
    pub async fn start_tls(
        server: std::sync::Arc<rustls::ServerConfig>,
    ) -> Result<Self, HarnessError> {
        let plain = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(HarnessError::Start)?;
        let secure = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(HarnessError::Start)?;
        let address = plain.local_addr().map_err(HarnessError::Start)?;
        let secure_address = secure.local_addr().map_err(HarnessError::Start)?;
        let task = tokio::spawn(async move {
            let _ = crate::tls::serve_both(plain, secure, server).await;
        });
        Ok(Self {
            base: format!("http://{address}"),
            provider: None,
            secure: Some(format!("https://{secure_address}")),
            http: client_builder().build()?,
            server: task,
        })
    }

    /// The `https` address of a path, for an application started with
    /// [`TestApp::start_tls`]; the plain address for one that was not.
    #[cfg(feature = "tls")]
    #[must_use]
    pub fn secure_url(&self, path: &str) -> String {
        format!("{}{path}", self.secure.as_deref().unwrap_or(&self.base))
    }

    /// The address of a path in this application, such as `/api/orders`.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// An HTTP client, for a test that acts as a user of the application.
    #[must_use]
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Restores the seed data and clears every fault, counter, and setting.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn reset(&self) -> Result<(), HarnessError> {
        self.command(self.http.post(self.url("/__control/reset")))
            .await
    }

    /// Exactly what the application has stored for a line item, if anything.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn stored_item(
        &self,
        order_id: &str,
        item_code: &str,
    ) -> Result<Option<StoredItem>, HarnessError> {
        let path = format!("/__control/orders/{order_id}/items/{item_code}");
        Ok(self.ask::<Stored<StoredItem>>(&path).await?.stored)
    }

    /// Exactly what the application has stored for an order, if anything.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn stored_order(&self, order_id: &str) -> Result<Option<StoredOrder>, HarnessError> {
        let path = format!("/__control/orders/{order_id}");
        Ok(self.ask::<Stored<StoredOrder>>(&path).await?.stored)
    }

    /// Every write since the last reset, oldest first: which request changed which record.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn writes(&self) -> Result<Vec<WriteRecord>, HarnessError> {
        Ok(self.ask::<Writes>("/__control/writes").await?.writes)
    }

    /// How many times one line item has been written since the last reset.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn write_count(
        &self,
        order_id: &str,
        item_code: &str,
    ) -> Result<usize, HarnessError> {
        let writes = self.writes().await?;
        Ok(writes
            .iter()
            .filter(|w| w.order_id == order_id && w.item_code == item_code)
            .count())
    }

    /// Switches a fault on.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn set_fault(&self, fault: &Fault) -> Result<(), HarnessError> {
        self.command(self.http.post(self.url("/__control/faults")).json(fault))
            .await
    }

    /// The faults that are switched on now.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn faults(&self) -> Result<Vec<Fault>, HarnessError> {
        Ok(self.ask::<Faults>("/__control/faults").await?.faults)
    }

    /// Switches every fault off.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn clear_faults(&self) -> Result<(), HarnessError> {
        self.command(self.http.delete(self.url("/__control/faults")))
            .await
    }

    /// Sets how long the slow endpoint waits, in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn set_slow_ms(&self, ms: u64) -> Result<(), HarnessError> {
        let body = serde_json::json!({"slow_ms": ms});
        self.command(self.http.post(self.url("/__control/settings")).json(&body))
            .await
    }

    /// Sets how a shipment's optional `carrier` is answered from now on
    /// (case 35), until the next reset.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn set_carrier(
        &self,
        answer: crate::answers::CarrierAnswer,
    ) -> Result<(), HarnessError> {
        self.command(
            self.http
                .post(self.url("/__control/shipment"))
                .json(&answer),
        )
        .await
    }

    /// Serves the description as its revision (case 38), or as its first
    /// text, from now on, until the next reset.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn revise_description(&self, revised: bool) -> Result<(), HarnessError> {
        let body = serde_json::json!({"revised": revised});
        self.command(
            self.http
                .post(self.url("/__control/description"))
                .json(&body),
        )
        .await
    }

    /// Changes one page as a release would (package 9.1), from now on,
    /// until the next reset or a change to `None`.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn change_screen(
        &self,
        page: crate::releases::ChangedPage,
        change: crate::releases::ScreenChange,
    ) -> Result<(), HarnessError> {
        let release = crate::releases::ScreenRelease { page, change };
        self.command(
            self.http
                .post(self.url("/__control/screens"))
                .json(&release),
        )
        .await
    }

    /// Exactly what is stored for an order's delivery details, if anything (case 33).
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn stored_delivery(
        &self,
        order_id: &str,
    ) -> Result<Option<StoredDelivery>, HarnessError> {
        let path = format!("/__control/orders/{order_id}/delivery");
        Ok(self.ask::<Stored<StoredDelivery>>(&path).await?.stored)
    }

    /// How many times an order's delivery details were written since the last reset.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn delivery_write_count(&self, order_id: &str) -> Result<u64, HarnessError> {
        let path = format!("/__control/orders/{order_id}/delivery");
        Ok(self.ask::<DeliveryWrites>(&path).await?.writes)
    }

    /// What the application counted about signing in since the last reset.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn auth_report(&self) -> Result<AuthReport, HarnessError> {
        self.ask("/__control/auth").await
    }

    /// Sets the sign-in modes: the one-time code step, and one session per account.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn set_auth_settings(&self, settings: &AuthSettings) -> Result<(), HarnessError> {
        self.command(
            self.http
                .post(self.url("/__control/auth/settings"))
                .json(settings),
        )
        .await
    }

    /// Ends every usable session as if it had timed out.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn expire_sessions(&self) -> Result<(), HarnessError> {
        self.command(self.http.post(self.url("/__control/auth/sessions/expire")))
            .await
    }

    /// Ends every usable session as if an administrator had ended it.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn revoke_sessions(&self) -> Result<(), HarnessError> {
        self.command(self.http.post(self.url("/__control/auth/sessions/revoke")))
            .await
    }

    /// Revokes the bearer token or the API key until the next reset.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface did not confirm.
    pub async fn revoke(&self, credential: Credential) -> Result<(), HarnessError> {
        let body = serde_json::json!({"credential": credential});
        self.command(
            self.http
                .post(self.url("/__control/auth/revoke"))
                .json(&body),
        )
        .await
    }

    /// How many times each trap page's write arrived since the last reset, by name
    /// (case 32). A name that never arrived is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn trap_writes(&self) -> Result<BTreeMap<String, u64>, HarnessError> {
        Ok(self.ask::<TrapWrites>("/__control/traps").await?.writes)
    }

    /// Every request the application received outside the control
    /// interface, as `"GET /orders"`, in the order they came, **but for a
    /// browser's own request for the site's icon**. A browser asks for
    /// `/favicon.ico` by itself, some time after a page it loaded, and no
    /// page, script, or driver makes it; a test that takes the list before
    /// an act and again after it, to show the act sent nothing, must not
    /// fail because the browser chose that moment. [`TestApp::seen_all`]
    /// leaves nothing out.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn seen(&self) -> Result<Vec<String>, HarnessError> {
        let mut seen = self.seen_all().await?;
        seen.retain(|line| line != "GET /favicon.ico");
        Ok(seen)
    }

    /// What the application has heard that it had not heard when `before`
    /// was taken: the requests in [`TestApp::seen`] now whose line is not
    /// one of `before`'s. A test uses it to show that an act it expects to
    /// be refused sent nothing of its own. A page loaded earlier may still
    /// be making the requests it makes by itself (its script's own reads),
    /// and such a late repeat of an earlier request is not something the
    /// refused act sent; comparing the two lists for equality would fail
    /// on it. The act under test must be one whose request, had it been
    /// sent, would differ from every earlier one, as a refused key's does.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn heard_since(&self, before: &[String]) -> Result<Vec<String>, HarnessError> {
        Ok(self
            .seen()
            .await?
            .into_iter()
            .filter(|line| !before.contains(line))
            .collect())
    }

    /// Every request the application received outside the control
    /// interface, the browser's own request for the site's icon included.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn seen_all(&self) -> Result<Vec<String>, HarnessError> {
        Ok(self.ask::<Seen>("/__control/seen").await?.seen)
    }

    /// What the pages of cases 6 to 9, 11, and 12 told the application since
    /// the last reset, oldest first, each as `name: text`.
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn case_events(&self) -> Result<Vec<String>, HarnessError> {
        Ok(self.ask::<CaseEvents>("/__control/cases").await?.events)
    }

    /// What the anti-forgery case stored: each order's note, how many writes
    /// changed it, and how many were refused for their token (case 5).
    ///
    /// # Errors
    ///
    /// Returns an error if the control interface could not be asked.
    pub async fn stored_notes(&self) -> Result<NotesReport, HarnessError> {
        self.ask::<NotesReport>("/__control/notes").await
    }

    /// Signs in as [`USERNAME`] through the sign-in form, and the one-time code step
    /// when it is on, as a person would, and returns the value for a `Cookie` header
    /// that carries the new session.
    ///
    /// # Errors
    ///
    /// Returns an error if the application did not give a session.
    pub async fn session_cookie(&self) -> Result<String, HarnessError> {
        let client = client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let form = format!("username={USERNAME}&password={PASSWORD}");
        let mut response = client
            .post(self.url("/secure/sign-in"))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
            .send()
            .await?;
        if let Some(pending) = cookie_from(&response, PENDING_COOKIE) {
            response = client
                .post(self.url("/secure/sign-in/code"))
                .header("content-type", "application/x-www-form-urlencoded")
                .header("cookie", format!("{PENDING_COOKIE}={pending}"))
                .body(format!("code={SECOND_FACTOR_CODE}"))
                .send()
                .await?;
        }
        match cookie_from(&response, SESSION_COOKIE) {
            Some(session) if !session.is_empty() => Ok(format!("{SESSION_COOKIE}={session}")),
            _ => Err(HarnessError::Status(response.status().as_u16())),
        }
    }

    async fn command(&self, request: reqwest::RequestBuilder) -> Result<(), HarnessError> {
        let response = request.send().await?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(HarnessError::Status(response.status().as_u16()))
        }
    }

    async fn ask<T: DeserializeOwned>(&self, path: &str) -> Result<T, HarnessError> {
        let response = self.http.get(self.url(path)).send().await?;
        if !response.status().is_success() {
            return Err(HarnessError::Status(response.status().as_u16()));
        }
        Ok(response.json().await?)
    }
}

/// The value a response sets for one cookie, if it sets it.
fn cookie_from(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(n, _)| *n == name)
        .map(|(_, v)| v.to_owned())
        .next_back()
}

impl Drop for TestApp {
    fn drop(&mut self) {
        self.server.abort();
    }
}
