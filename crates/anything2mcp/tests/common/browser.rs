//! A person with a real browser: it opens a page's link and presses one of
//! its buttons, as a person does, so that the page meets what a browser
//! really sends with a press (its `Origin`, under the page's own referrer
//! policy) and not what a test would write by hand.
//!
//! It is the browser engine of the web pack, started as its own tests start
//! it: the browser of the test environment, with a data directory under a
//! folder of the test's own.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use a2m_core::{AccountName, Environment, Failure, Identity, NoSecrets, TargetName};
use a2m_logs::{LogLevel, LogsAndTraces, SystemClock};
use a2m_pack_web_browser::{
    Address, BrowserProfileKey, Browsers, By, Deadline, Input, Look, Origins, PROFILE_ROOT,
    PageEnvironment, Primitive, Query, RawValue, ReadWhat, TextMatch, Visibility, test_support,
};
use a2m_provider::{
    Access, HostServices, Pack, PackContext, ProviderSettings, Registrar, RegistryBuilder,
    RootName, Roots, Supervisor, SupervisorLimits,
};

use super::Folder;

/// A pack that registers nothing and keeps the context it is handed: the
/// one way a test gets a scoped supervisor and a profile root.
struct Capture {
    manifest: &'static str,
    context: Mutex<Option<PackContext>>,
}

impl Pack for Capture {
    fn manifest(&self) -> &'static str {
        self.manifest
    }

    fn register(&self, cx: &PackContext, _to: &mut Registrar<'_>) -> Result<(), Failure> {
        *self.context.lock().unwrap() = Some(cx.clone());
        Ok(())
    }
}

/// The person's browser.
pub struct Person {
    browsers: Browsers,
    _folder: Folder,
}

fn css(selector: &str) -> Query {
    Query::new(By::Css {
        selector: selector.to_owned(),
    })
}

fn soon() -> Deadline {
    Deadline::after(Duration::from_secs(60))
}

#[track_caller]
fn must<T>(result: Result<T, a2m_pack_web_browser::BrowserError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => match error.failure() {
            Some(failure) => panic!("{}/{}: {failure}", failure.class(), failure.code()),
            None => panic!("{error}"),
        },
    }
}

impl Person {
    pub fn new() -> Self {
        let folder = Folder::new("browser");
        let profiles = folder.path().join("profiles");
        std::fs::create_dir_all(&profiles).unwrap();
        let mut roots = Roots::none();
        roots
            .add(
                RootName::new(PROFILE_ROOT).unwrap(),
                &profiles,
                Access::Write,
            )
            .unwrap();
        let host = HostServices {
            supervisor: Supervisor::new(SupervisorLimits::default()),
            roots,
            redact: Arc::new(NoSecrets),
            logs_and_traces: LogsAndTraces::in_memory(
                LogLevel::Error,
                Arc::new(NoSecrets),
                Arc::new(SystemClock::new()),
            ),
        };
        let manifest = format!(
            "manifest_format = 1\ncontract = 1\n\n[provider]\nname = \"web\"\nversion = \"0.1.0\"\n\
             publisher = \"Example\"\nkind = \"pack\"\n\n[effects]\nwrite_roots = [\"{PROFILE_ROOT}\"]\n\
             network = \"surfaces\"\nprocesses = []\n{}",
            test_support::requirement()
        );
        let pack = Capture {
            manifest: Box::leak(manifest.into_boxed_str()),
            context: Mutex::new(None),
        };
        let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
        builder.add_pack(&pack);
        let (_registry, problems) = builder.build();
        assert!(problems.is_empty(), "{problems:?}");
        let context = pack.context.lock().unwrap().clone().unwrap();
        Self {
            browsers: Browsers::new(&context, test_support::settings()).unwrap(),
            _folder: folder,
        }
    }

    /// Opens `link`, presses the button named `button` as a person does,
    /// and gives the title and the text of the page that answers the press.
    pub async fn press(&self, link: &str, button: &str) -> (String, String) {
        let profile = BrowserProfileKey {
            target: TargetName::new("example").unwrap(),
            environment: Environment::new("test").unwrap(),
            identity: Identity::Account(AccountName::new("person").unwrap()),
        };
        let browser = must(self.browsers.open(&profile, Visibility::Background).await);
        let address = Address::new(link).unwrap();
        let tab = must(
            browser
                .acting_tab(&PageEnvironment::default(), &Origins::of([&address]))
                .await,
        );
        must(tab.goto(&address, soon()).await);
        let pressed = must(tab.locate(&Query::role("button", button), soon()).await);
        must(tab.input(&pressed, &Input::Click, soon()).await);
        // The form posts to the page's own address and the answer replaces
        // the page.
        let answered = format!("{link}/{}", button.to_lowercase());
        must(
            tab.wait(
                &Primitive::Url(TextMatch {
                    text: answered,
                    contains: false,
                }),
                soon(),
            )
            .await,
        );
        must(tab.wait(&Primitive::Visible(css("main p")), soon()).await);
        let text_of = |selector: &'static str| {
            let tab = &tab;
            async move {
                let element = must(tab.locate(&css(selector), soon()).await);
                match must(tab.read(&element, &ReadWhat::Text, soon()).await) {
                    RawValue::Text(text) => text,
                    other => panic!("not text: {other:?}"),
                }
            }
        };
        let title = text_of("main h1").await;
        let detail = text_of("main p").await;
        drop(tab);
        (title, detail)
    }

    pub async fn end(self) {
        let cleanup = self.browsers.shutdown().await;
        assert!(cleanup.complete, "{cleanup:?}");
    }
}
