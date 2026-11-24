//! The watching tab (decision 0392, point 1), in a window on a screen,
//! against the test application, with the test acting as the person
//! through the page itself: nothing in the tab is failed for its method,
//! the engine sends nothing of its own, what is recorded keeps structure
//! and no value, and a snapshot is taken of each screen the person visits.
//!
//! Where no screen can be had the test says so and passes, unless
//! `A2M_REQUIRE_DESKTOP=1` says one must be had: then it fails.

use std::collections::BTreeMap;

use a2m_provider::take_capture;

use super::window::screen;
use super::*;
use crate::{MASK, WatchStart, WatchTab};

/// A made-up secret in the form of a plain word, so that only the
/// redactor, and no rule of form, can tell it is one.
const SECRET: &str = "plantedwatchword";

/// What the person types: a value that is no secret, which nothing kept
/// may hold all the same.
const TYPED: &str = "Seen in the watch";

const ROWS_PAGE: &str = "/cases/rows-by-post";
const ROWS_SEARCH: &str = "/api/cases/rows/search";
const SUMMARY: &str = "/orders/A-100/summary";
const TOTALS: &str = "/api/orders/A-100/totals";
const ITEM: &str = "/api/orders/A-100/items/L-101";

/// A script of the page that answers once `condition` holds, watching the
/// document for it.
fn once(condition: &str) -> String {
    format!(
        "new Promise((done) => {{ const look = () => {{ if ({condition}) {{ done('ok'); return true; }} return false; }}; \
         if (!look()) {{ const watcher = new MutationObserver(() => {{ if (look()) watcher.disconnect(); }}); \
         watcher.observe(document, {{subtree: true, childList: true, characterData: true, attributes: true}}); }} }})"
    )
}

fn state_is(text: &str) -> String {
    once(&format!(
        "document.getElementById('state') && document.getElementById('state').textContent === {text:?}"
    ))
}

/// The person goes to `path` by the page's own address bar, as a person
/// following a link does, and the test waits until the new document has
/// loaded and the tab has looked at it.
async fn person_goes(tab: &WatchTab, app: &TestApp, path: &str) {
    let changes = tab.changes();
    let looked = tab.looked_at();
    let moved = must(
        tab.page
            .as_the_page(
                &format!("location.href = {:?}; 'moved'", app.url(path)),
                soon(),
            )
            .await,
    );
    assert_eq!(moved, "moved");
    must(tab.changed_since(changes, soon()).await);
    assert!(tab.looked_at_since(looked, soon()).await, "{path}");
}

/// The lines the stand heard, each with how often.
fn counted(lines: Vec<String>) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for line in lines {
        *out.entry(line).or_insert(0) += 1;
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watching_tab_records_every_method_by_structure_and_the_engine_sends_nothing() {
    let Some(screen) = screen() else {
        return;
    };
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_redactor("watching", screen.settings(), &[], Arc::new(Knows(SECRET)));
    let person = profile("person");
    let observer = stand.browsers.observer();
    let own = Origins::of([&Address::new(&app.url("/")).unwrap()]);

    // The first address passes the check every address of a discovery
    // passes, before anything is opened.
    let refused = WatchStart::new(
        &Address::new(&app.url("/orders/A-100/delete")).unwrap(),
        &own,
    )
    .unwrap_err();
    assert_eq!(*refused.code(), codes::ADDRESS_LOOKS_LIKE_AN_ACTION);
    let elsewhere =
        WatchStart::new(&Address::new("http://elsewhere.invalid/").unwrap(), &own).unwrap_err();
    assert_eq!(*elsewhere.code(), codes::SURFACE_NOT_DECLARED);
    let start = WatchStart::new(&Address::new(&app.url(ROWS_PAGE)).unwrap(), &own).unwrap();
    assert!(stand.browsers.list().is_empty());

    let seeded = app.case_rows().await.unwrap();
    let before = app.seen().await.unwrap();
    let tab = must(
        observer
            .watching_tab(
                &person,
                &environment(),
                "Watching example (test) for the person",
                &own,
                &start,
                soon(),
            )
            .await,
    );
    assert!(tab.is_open());

    // The page's own search by POST is not failed for its method: the
    // rows arrive.
    assert_eq!(
        must(tab.page.as_the_page(&state_is("Loaded."), soon()).await),
        "ok"
    );
    assert!(tab.looked_at_since(0, soon()).await);
    assert!(tab.blocked().is_empty());

    // A page whose script reads with GET.
    person_goes(&tab, &app, SUMMARY).await;
    assert_eq!(
        must(tab.page.as_the_page(&state_is("Loaded."), soon()).await),
        "ok"
    );

    // The person saves a line item, typing a value and a secret.
    person_goes(&tab, &app, crate::tests::FORM).await;
    let saved = must(
        tab.page
            .as_the_page(
                &format!(
                    "(() => {{ const form = document.getElementById('line-item'); \
                     form.elements.description.value = {:?}; form.requestSubmit(); \
                     return {}; }})()",
                    format!("{TYPED} {SECRET}"),
                    once("document.getElementById('status').textContent === 'Saved.'"),
                ),
                soon(),
            )
            .await,
    );
    assert_eq!(saved, "ok");

    // A body whose field is named by the secret, and a form.
    let named = must(
        tab.page
            .as_the_page(
                &format!(
                    "fetch({ROWS_SEARCH:?}, {{method: 'POST', headers: {{'Content-Type': 'application/json'}}, \
                     body: JSON.stringify({{status: 'open', op: 'search', {SECRET}: {TYPED:?}}})}}) \
                     .then((r) => 'sent:' + r.status, () => 'failed')"
                ),
                soon(),
            )
            .await,
    );
    assert!(named.starts_with("sent:"), "{named}");
    let form = must(
        tab.page
            .as_the_page(
                &format!(
                    "fetch({ROWS_SEARCH:?}, {{method: 'POST', body: new URLSearchParams({{status: 'open', note: {TYPED:?}}})}}) \
                     .then((r) => 'sent:' + r.status, () => 'failed')"
                ),
                soon(),
            )
            .await,
    );
    assert!(form.starts_with("sent:"), "{form}");

    // A move to an origin the surface does not declare is ended in the
    // browser, and the page stays.
    let changes = tab.changes();
    let _ = tab
        .page
        .as_the_page(
            "location.href = 'http://elsewhere.invalid/cases'; 'moved'",
            soon(),
        )
        .await;
    must(tab.changed_since(changes, soon()).await);
    let blocked = tab.blocked();
    assert!(
        blocked.iter().any(|one| one.method == "NAVIGATION"),
        "{blocked:?}"
    );

    // Back to the rows, and to the line item, which now shows what was
    // typed.
    person_goes(&tab, &app, ROWS_PAGE).await;
    assert_eq!(
        must(tab.page.as_the_page(&state_is("Loaded."), soon()).await),
        "ok"
    );
    person_goes(&tab, &app, crate::tests::FORM).await;

    // What the capture yields, through the contract's check.
    let traffic = take_capture(&tab, soon(), &Knows(SECRET)).await;
    let text = format!("{traffic:?}");
    for kept_out in [SECRET, "[redacted]", TYPED, "Leaking valve"] {
        assert!(!text.contains(kept_out), "{kept_out}: {text}");
    }
    assert_eq!((traffic.dropped, traffic.awaited), (0, 0), "{text}");
    let of = |method: &str, path: &str| {
        traffic
            .requests
            .iter()
            .filter(|one| one.method == method && one.path == path)
            .collect::<Vec<_>>()
    };
    let fields = |pairs: &[(&str, &str)]| -> Option<BTreeMap<String, String>> {
        Some(
            pairs
                .iter()
                .map(|(name, kind)| ((*name).to_owned(), (*kind).to_owned()))
                .collect(),
        )
    };
    // The page's search, twice, with its body's names and kinds.
    let searches = of("POST", ROWS_SEARCH);
    assert_eq!(searches.len(), 4, "{text}");
    let by_page: Vec<_> = searches
        .iter()
        .filter(|one| {
            one.body_media_type == "application/json"
                && one.body_fields == fields(&[("op", "string"), ("status", "string")])
                && !one.body_names_withheld
        })
        .collect();
    assert_eq!(by_page.len(), 2, "{text}");
    assert_eq!(by_page[0].status, 200);
    assert!(
        by_page[0]
            .fields
            .as_ref()
            .is_some_and(|answer| !answer.is_empty())
    );
    // The field named by the secret is withheld, and said to be.
    let secret_named: Vec<_> = searches
        .iter()
        .filter(|one| one.body_names_withheld)
        .collect();
    assert_eq!(secret_named.len(), 1, "{text}");
    assert_eq!(
        secret_named[0].body_fields,
        fields(&[("op", "string"), ("status", "string")])
    );
    // The form's entries are names, each a text.
    let forms: Vec<_> = searches
        .iter()
        .filter(|one| one.body_media_type == "application/x-www-form-urlencoded")
        .collect();
    assert_eq!(forms.len(), 1, "{text}");
    assert_eq!(
        forms[0].body_fields,
        fields(&[("note", "string"), ("status", "string")])
    );
    // The read by GET, with no body.
    let totals = of("GET", TOTALS);
    assert_eq!(totals.len(), 1, "{text}");
    assert_eq!(totals[0].body_media_type, "");
    assert_eq!(totals[0].body_fields, None);
    assert!(totals[0].fields.is_some());
    // The save, once, by its body's names and kinds and none of its values.
    let saves = of("PUT", ITEM);
    assert_eq!(saves.len(), 1, "{text}");
    assert_eq!(
        saves[0].body_fields,
        fields(&[
            ("description", "string"),
            ("length_in", "string"),
            ("quantity", "integer"),
            ("version", "integer"),
        ])
    );
    assert_eq!(saves[0].status, 200);

    // A snapshot of each screen the person came to, in order, holding no
    // value.
    let screens = tab.screens();
    let addresses: Vec<&str> = screens
        .snapshots
        .iter()
        .map(|one| one.address.as_str())
        .collect();
    assert_eq!(
        addresses,
        [
            app.url(ROWS_PAGE),
            app.url(SUMMARY),
            app.url(crate::tests::FORM),
            app.url(ROWS_PAGE),
            app.url(crate::tests::FORM),
        ],
        "{screens:?}"
    );
    assert!(!screens.bound_hit);
    assert_eq!(screens.missed, 0);
    let last = screens.snapshots.last().unwrap();
    assert!(
        last.elements
            .iter()
            .any(|one| one.role == "textbox" && one.name == "Description"),
        "{last:?}"
    );
    for snapshot in &screens.snapshots {
        assert!(
            snapshot
                .elements
                .iter()
                .all(|one| one.value.is_none() || one.value.as_deref() == Some(MASK)),
            "{snapshot:?}"
        );
        let shown = format!("{snapshot:?}");
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(!shown.contains(TYPED), "{shown}");
    }

    // The stand heard what the person did and the one address the engine
    // opened, and nothing of the engine's own: no snapshot and no record
    // sent anything.
    let mut heard = app.seen().await.unwrap();
    heard.drain(..before.len());
    let expected = counted(
        [
            format!("GET {ROWS_PAGE}"),
            format!("GET {ROWS_PAGE}"),
            format!("POST {ROWS_SEARCH}"),
            format!("POST {ROWS_SEARCH}"),
            format!("POST {ROWS_SEARCH}"),
            format!("POST {ROWS_SEARCH}"),
            format!("GET {SUMMARY}"),
            format!("GET {TOTALS}"),
            format!("GET {}", crate::tests::FORM),
            format!("GET {}", crate::tests::FORM),
            format!("PUT {ITEM}"),
        ]
        .into(),
    );
    assert_eq!(counted(heard), expected);
    // The save was stored once, as typed; no row was archived.
    let item = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!(item.description, format!("{TYPED} {SECRET}"));
    assert_eq!(app.case_rows().await.unwrap().rows, seeded.rows);

    drop(tab);
    stand.end().await;
}

/// The watching tab's type has no method that presses, types, navigates, or
/// sends: its public methods are the ones listed here, and it has no `Look`.
#[test]
fn a_watching_tab_has_no_method_that_acts() {
    let source = include_str!("../tab/watching.rs");
    let block = source
        .split("impl WatchTab {")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .unwrap();
    let mut methods: Vec<&str> = block
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let rest = line
                .strip_prefix("pub fn ")
                .or_else(|| line.strip_prefix("pub async fn "))?;
            rest.split('(').next()
        })
        .collect();
    methods.sort_unstable();
    assert_eq!(
        methods,
        [
            "blocked",
            "changed_since",
            "changes",
            "closed",
            "exchanges",
            "is_open",
            "looked_at",
            "looked_at_since",
            "screens",
        ]
    );
    for file in [
        include_str!("../tab.rs"),
        include_str!("../tab/watching.rs"),
        include_str!("../capture.rs"),
        include_str!("../browsers.rs"),
    ] {
        assert!(!file.contains("looks!(WatchTab)"));
        assert!(!file.contains("for WatchTab {\n    async fn goto"));
        assert!(!file.contains("impl Look for WatchTab"));
    }
}
