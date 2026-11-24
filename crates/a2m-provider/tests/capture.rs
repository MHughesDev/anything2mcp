//! Traffic capture as a capability of the contract (decision 0391): only a
//! pack whose manifest declares it is asked, and what a capture yields
//! keeps structure and no secret, whatever the pack hands over.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2m_core::{
    CompiledSurface, Environment, Failure, FailureClass, PackPayload, SurfaceKind, SurfaceName,
    TargetName,
};
use a2m_provider::{
    BoxFuture, CAPTURE_PLACEHOLDER, CaptureSource, CapturedRequest, CapturedTraffic, Deadline,
    Pack, PackContext, ProviderSettings, Registrar, RegistryBuilder, Roots, SurfaceRef,
    TrafficCapture, codes, is_captured_word, kept_by_structure,
};
use common::{Folder, OneSecret, host_with};

/// A made-up secret in the form of a plain word, so that only the
/// redactor, and no rule of form, can tell it is one.
const WORD_SECRET: &str = "plainsecretword";

/// A made-up bearer token, in a form no real one has.
const TOKEN: &str = "Bearer tok-for-tests-91c2";

/// What a careless pack would yield: a secret in every field there is.
fn careless() -> CapturedTraffic {
    let mut fields = BTreeMap::new();
    fields.insert("id".to_owned(), "integer".to_owned());
    fields.insert(WORD_SECRET.to_owned(), "string".to_owned());
    fields.insert("note".to_owned(), TOKEN.to_owned());
    fields.insert("items.[].name".to_owned(), "string".to_owned());
    CapturedTraffic {
        requests: vec![
            CapturedRequest {
                method: "GET".to_owned(),
                origin: "http://127.0.0.1:8080".to_owned(),
                path: format!("/api/{WORD_SECRET}/orders/A-100/{TOKEN}?token={WORD_SECRET}"),
                query: vec![
                    "view".to_owned(),
                    WORD_SECRET.to_owned(),
                    "a=b".to_owned(),
                    "view".to_owned(),
                ],
                query_left_out: false,
                credential_headers: vec!["Authorization".to_owned(), TOKEN.to_owned()],
                set_headers: vec!["x-view".to_owned(), WORD_SECRET.to_owned()],
                headers_left_out: false,
                status: 200,
                media_type: "application/json".to_owned(),
                fields: Some(fields),
                names_withheld: false,
            },
            // A secret in the origin withholds the request whole.
            CapturedRequest {
                method: "GET".to_owned(),
                origin: format!("http://{WORD_SECRET}.example.com"),
                path: "/".to_owned(),
                status: 200,
                ..CapturedRequest::default()
            },
            // So does an origin that carries a user part, or a method that
            // is not one.
            CapturedRequest {
                method: "GET".to_owned(),
                origin: "http://someone:pw@example.com".to_owned(),
                path: "/".to_owned(),
                status: 200,
                ..CapturedRequest::default()
            },
            CapturedRequest {
                method: "get /".to_owned(),
                origin: "http://example.com".to_owned(),
                path: "/".to_owned(),
                status: 200,
                ..CapturedRequest::default()
            },
        ],
        dropped: 1,
        awaited: 2,
        withheld: 0,
    }
}

fn all_text(traffic: &CapturedTraffic) -> String {
    format!("{traffic:?}")
}

#[test]
fn a_capture_keeps_structure_and_no_secret_whatever_the_pack_yields() {
    let redact = OneSecret(WORD_SECRET);
    let raw = careless();
    assert!(all_text(&raw).contains(WORD_SECRET));
    let kept = kept_by_structure(raw, &redact);
    let text = all_text(&kept);
    assert!(!text.contains(WORD_SECRET), "{text}");
    assert!(!text.contains("tok-for-tests"), "{text}");
    assert!(!text.contains("pw@"), "{text}");
    assert_eq!((kept.dropped, kept.awaited, kept.withheld), (1, 2, 3));
    assert_eq!(kept.requests.len(), 1);
    let one = &kept.requests[0];
    assert_eq!(one.method, "GET");
    assert_eq!(one.origin, "http://127.0.0.1:8080");
    assert_eq!(
        one.path,
        format!("/api/{CAPTURE_PLACEHOLDER}/orders/A-100/{CAPTURE_PLACEHOLDER}")
    );
    assert_eq!(one.query, vec!["view".to_owned()]);
    assert!(one.query_left_out);
    assert_eq!(one.credential_headers, vec!["authorization".to_owned()]);
    assert_eq!(one.set_headers, vec!["x-view".to_owned()]);
    assert!(one.headers_left_out);
    assert_eq!(one.media_type, "application/json");
    let fields = one.fields.as_ref().unwrap();
    assert_eq!(
        fields.keys().cloned().collect::<Vec<_>>(),
        vec!["id".to_owned(), "items.[].name".to_owned()]
    );
    assert!(one.names_withheld);
    // What is already structure passes unchanged.
    let again = kept_by_structure(kept.clone(), &redact);
    assert_eq!(again.requests, kept.requests);
}

/// A capture that hands over what its function yields.
struct Careless(fn() -> CapturedTraffic);

impl TrafficCapture for Careless {
    fn start(&self) {}

    fn take_recorded<'a>(&'a self, _until: Deadline) -> BoxFuture<'a, CapturedTraffic> {
        Box::pin(async { (self.0)() })
    }
}

/// A capture source that counts how often it is asked.
struct Counted(Arc<AtomicUsize>, fn() -> CapturedTraffic);

impl CaptureSource for Counted {
    fn open<'a>(
        &'a self,
        _surface: &'a SurfaceRef,
        _deadline: Deadline,
    ) -> BoxFuture<'a, Result<Box<dyn TrafficCapture>, Failure>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let yields = self.1;
        Box::pin(async move { Ok(Box::new(Careless(yields)) as Box<dyn TrafficCapture>) })
    }
}

/// A pack with no driver and no discoverer: a manifest, and perhaps a
/// capture source.
struct Bare {
    manifest: &'static str,
    registers: bool,
    asked: Arc<AtomicUsize>,
    yields: fn() -> CapturedTraffic,
}

impl Bare {
    fn new(name: &str, declares: &str, registers: bool) -> Self {
        let text = format!(
            "manifest_format = 1\ncontract = 1\n\n[provider]\nname = \"{name}\"\nversion = \"0.1.0\"\n\
             publisher = \"Example\"\nkind = \"pack\"\n\n[points]\ntraffic_capture = [{declares}]\n"
        );
        Self {
            manifest: Box::leak(text.into_boxed_str()),
            registers,
            asked: Arc::new(AtomicUsize::new(0)),
            yields: careless,
        }
    }

    fn yielding(mut self, yields: fn() -> CapturedTraffic) -> Self {
        self.yields = yields;
        self
    }
}

impl Pack for Bare {
    fn manifest(&self) -> &'static str {
        self.manifest
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        if self.registers {
            to.traffic_capture(Arc::new(Counted(Arc::clone(&self.asked), self.yields)))?;
        }
        Ok(())
    }
}

fn surface(kind: &str) -> SurfaceRef {
    SurfaceRef {
        target: TargetName::new("example").unwrap(),
        environment: Environment::new("test").unwrap(),
        surface: CompiledSurface {
            name: SurfaceName::new("app").unwrap(),
            kind: SurfaceKind::new(kind).unwrap(),
            address: "http://127.0.0.1:8080/".to_owned(),
            same_session_as: None,
            stands: None,
            rate: None,
            programs: Vec::new(),
            payload: PackPayload::none(),
        },
    }
}

#[tokio::test]
async fn a_pack_that_declares_no_capture_is_never_asked() {
    let offers = Bare::new("offers", "\"web\"", true);
    // Registers a source its manifest does not declare: refused whole.
    let sneaks = Bare::new("sneaks", "", true);
    // Declares nothing and registers nothing.
    let quiet = Bare::new("quiet", "", false);
    let folder = Folder::new("capture");
    let mut builder = RegistryBuilder::new(
        ProviderSettings::default(),
        host_with(&folder, Roots::none(), Arc::new(OneSecret(WORD_SECRET))),
    );
    for pack in [&offers, &sneaks, &quiet] {
        builder.add_pack(pack);
    }
    let (registry, problems) = builder.build();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].provider.as_ref().unwrap().as_str(), "sneaks");
    assert_eq!(problems[0].failure.class(), FailureClass::Policy);
    assert_eq!(problems[0].failure.code(), &codes::EFFECT_NOT_DECLARED);

    let web = SurfaceKind::new("web").unwrap();
    let handles = registry.captures_for(&web);
    assert_eq!(handles.len(), 1);
    assert_eq!(handles[0].provider().as_str(), "offers");
    assert!(
        registry
            .captures_for(&SurfaceKind::new("desktop").unwrap())
            .is_empty()
    );

    // A surface of a kind the manifest does not declare is not handed over.
    let refused = handles[0]
        .open(&surface("desktop"), Deadline::after(Duration::from_secs(5)))
        .await
        .unwrap_err();
    assert_eq!(refused.code(), &codes::EFFECT_NOT_DECLARED);
    assert_eq!(offers.asked.load(Ordering::SeqCst), 0);

    // Through the registry, the host's redactor and the contract's check
    // stand between the pack's yield and the caller.
    let capturing = handles[0]
        .open(&surface("web"), Deadline::after(Duration::from_secs(5)))
        .await
        .unwrap();
    let taken = capturing
        .take(Deadline::after(Duration::from_secs(5)))
        .await;
    assert_eq!(taken.requests.len(), 1);
    assert!(!all_text(&taken).contains(WORD_SECRET));
    assert!(!all_text(&taken).contains("tok-for-tests"));

    assert_eq!(offers.asked.load(Ordering::SeqCst), 1);
    assert_eq!(sneaks.asked.load(Ordering::SeqCst), 0);
    assert_eq!(quiet.asked.load(Ordering::SeqCst), 0);
}

/// A UUID, a base-62 token of 24 characters, and a hexadecimal token, none
/// of which the redactor knows.
const UUID: &str = "3f2b8c1e-9a4d-4e7b-b6c5-0d1e2f3a4b5c";
const BASE62: &str = "k3JqP9xR2mWvT7bYcN5dLs8F";
const HEX: &str = "a3f09c7e5b21d4e8f6a7";

/// What a pack yields that failed to flag identifiers as values: in path
/// segments, a query's name, and the keys of an answer's map.
fn id_shaped() -> CapturedTraffic {
    let mut fields = BTreeMap::new();
    fields.insert("orders".to_owned(), "object".to_owned());
    fields.insert("8812.name".to_owned(), "string".to_owned());
    fields.insert("8813.name".to_owned(), "string".to_owned());
    CapturedTraffic {
        requests: vec![CapturedRequest {
            method: "GET".to_owned(),
            origin: "http://127.0.0.1:8080".to_owned(),
            path: format!("/api/v2/users/{UUID}/sessions/{BASE62}/orders/100245"),
            query: vec!["view".to_owned(), HEX.to_owned()],
            credential_headers: vec!["authorization".to_owned()],
            set_headers: vec![format!("x-{HEX}")],
            status: 200,
            media_type: "application/json".to_owned(),
            fields: Some(fields),
            ..CapturedRequest::default()
        }],
        ..CapturedTraffic::default()
    }
}

#[tokio::test]
async fn identifiers_a_pack_failed_to_flag_are_not_kept_as_structure() {
    let ids = Bare::new("ids", "\"web\"", true).yielding(id_shaped);
    let folder = Folder::new("capture-ids");
    let mut builder = RegistryBuilder::new(
        ProviderSettings::default(),
        // The redactor knows none of them: the rule of form alone holds.
        host_with(&folder, Roots::none(), Arc::new(OneSecret(WORD_SECRET))),
    );
    builder.add_pack(&ids);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    let handle = registry
        .captures_for(&SurfaceKind::new("web").unwrap())
        .remove(0);
    let capturing = handle
        .open(&surface("web"), Deadline::after(Duration::from_secs(5)))
        .await
        .unwrap();
    let taken = capturing
        .take(Deadline::after(Duration::from_secs(5)))
        .await;
    let text = all_text(&taken);
    for planted in [UUID, BASE62, HEX, "100245", "8812", "8813"] {
        assert!(!text.contains(planted), "{planted} in {text}");
    }
    assert_eq!(taken.requests.len(), 1);
    let one = &taken.requests[0];
    assert_eq!(
        one.path,
        format!(
            "/api/v2/users/{CAPTURE_PLACEHOLDER}/sessions/{CAPTURE_PLACEHOLDER}/orders/{CAPTURE_PLACEHOLDER}"
        )
    );
    assert_eq!(one.query, vec!["view".to_owned()]);
    assert!(one.query_left_out);
    assert_eq!(one.credential_headers, vec!["authorization".to_owned()]);
    assert!(one.set_headers.is_empty());
    assert!(one.headers_left_out);
    assert_eq!(
        one.fields
            .as_ref()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["orders".to_owned()]
    );
    assert!(one.names_withheld);
    assert_eq!(ids.asked.load(Ordering::SeqCst), 1);
}

#[test]
fn a_kept_word_has_few_digits_and_no_long_hexadecimal_run() {
    for word in [
        "v2",
        "oauth2",
        "s3",
        "A-100",
        "orders",
        "x-csrf-token",
        "items",
    ] {
        assert!(is_captured_word(word), "{word}");
    }
    for value in [
        UUID,
        BASE62,
        HEX,
        "100245",
        "2026-11-24",
        "deadbeefdeadbeef",
        "a-name-that-is-longer-than-thirty-two",
        "..",
        "",
    ] {
        assert!(!is_captured_word(value), "{value}");
    }
}
