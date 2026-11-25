//! Described against observed (decision 0999): the comparison's four
//! results, the key's fit, the side by source, the level of each result,
//! and the key rule a pack registers for a kind it owns.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{
    Address, ComparisonResult, Coverage, EvidenceLevel, Failure, FailureClass, Finding,
    FindingKind, NotCompared, ProducedBy, Source, Subject, Timestamp,
};
use a2m_provider::{
    Comparison, ComparisonKey, KeyRule, KeySegment, Pack, PackContext, ProviderSettings, Registrar,
    Registry, RegistryBuilder, Roots, compare_findings,
};
use common::{Folder, host};

const ORIGIN: &str = "https://example.test";

/// A finding whose `found` is `VERB /path`, as the test's own kinds hold
/// it.
fn at(
    origin: &str,
    kind: &str,
    said: &str,
    source: Source,
    level: EvidenceLevel,
    coverage: Coverage,
) -> Finding {
    Finding::about(
        Subject::Address(Address::new(origin, "/").unwrap()),
        FindingKind::new(kind).unwrap(),
        said,
        source,
        ProducedBy {
            provider: "alpha".to_owned(),
            version: "0.1.0".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(1_800_000_000_000),
    )
    .level(level)
    .coverage(coverage)
    .build()
    .unwrap()
}

fn described(said: &str) -> Finding {
    at(
        ORIGIN,
        "claim",
        said,
        Source::PublishedDescription,
        EvidenceLevel::Inferred,
        Coverage::Complete,
    )
}

fn seen(said: &str, level: EvidenceLevel) -> Finding {
    at(
        ORIGIN,
        "call",
        said,
        Source::LiveSystem,
        level,
        Coverage::Partial,
    )
}

/// The test's key rule: the origin of the subject, and the verb and path
/// of `found`. A `found` with no space has no key.
struct Spaced;

impl KeyRule for Spaced {
    fn key(&self, finding: &Finding) -> Option<ComparisonKey> {
        let Subject::Address(address) = finding.subject() else {
            return None;
        };
        let (verb, path) = finding.found().split_once(' ')?;
        ComparisonKey::parse(address.origin(), verb, path)
    }
}

fn compared(findings: &[Finding]) -> Comparison {
    compare_findings(findings.iter().map(|one| (Spaced.key(one), one)))
}

/// The entry whose key has the verb and path given.
fn entry<'a>(comparison: &'a Comparison, verb: &str, path: &str) -> &'a a2m_provider::Compared {
    comparison
        .entries
        .iter()
        .find(|one| {
            one.key
                .as_ref()
                .is_some_and(|key| key.verb() == verb && key.path() == path)
        })
        .unwrap_or_else(|| panic!("no {verb} {path} in {comparison:?}"))
}

#[test]
fn a_key_is_its_place_verb_and_segments_without_a_query() {
    let key = ComparisonKey::parse(
        "HTTPS://Example.test",
        "get",
        "/orders/{order_id}/items/?x=1",
    )
    .unwrap();
    assert_eq!(key.place(), "https://example.test");
    assert_eq!(key.verb(), "GET");
    assert_eq!(
        key.segments(),
        [
            KeySegment::Literal("orders".to_owned()),
            KeySegment::Placeholder,
            KeySegment::Literal("items".to_owned()),
        ]
    );
    assert_eq!(key.path(), "/orders/{}/items");
    assert_eq!(
        ComparisonKey::parse(ORIGIN, "GET", "/").unwrap().path(),
        "/"
    );
    assert!(ComparisonKey::parse(ORIGIN, "GET", "orders").is_none());
    assert!(ComparisonKey::parse("", "GET", "/").is_none());
    assert!(ComparisonKey::parse(ORIGIN, "", "/").is_none());
}

#[test]
fn described_and_never_seen_is_inferred_seen_and_described_is_discovered_and_seen_alone_is_named() {
    let findings = [
        described("GET /orders/{order_id}/totals"),
        described("GET /orders"),
        seen("GET /orders/A-100/totals", EvidenceLevel::Discovered),
        seen("GET /orders/{p1}/lengths", EvidenceLevel::Discovered),
    ];
    let comparison = compared(&findings);
    assert_eq!(comparison.entries.len(), 3, "{comparison:?}");

    let matched = entry(&comparison, "GET", "/orders/{}/totals");
    assert_eq!(matched.result, ComparisonResult::Matched);
    assert_eq!(matched.level, EvidenceLevel::Discovered);
    assert_eq!(matched.described, [findings[0].id()]);
    assert_eq!(matched.observed, [findings[2].id()]);
    assert_eq!(matched.coverage, Coverage::Partial);

    let only = entry(&comparison, "GET", "/orders");
    assert_eq!(only.result, ComparisonResult::DescribedOnly);
    assert_eq!(only.level, EvidenceLevel::Inferred);
    assert_eq!(only.reason, None);
    assert!(only.observed.is_empty());

    let alone = entry(&comparison, "GET", "/orders/{}/lengths");
    assert_eq!(alone.result, ComparisonResult::ObservedOnly);
    assert_eq!(alone.level, EvidenceLevel::Discovered);
    assert_eq!(alone.observed, [findings[3].id()]);
    assert!(alone.described.is_empty());

    assert_eq!(comparison.count(ComparisonResult::Matched), 1);
    assert_eq!(comparison.count(ComparisonResult::DescribedOnly), 1);
    assert_eq!(comparison.count(ComparisonResult::ObservedOnly), 1);
    assert_eq!(comparison.count(ComparisonResult::Unknown), 0);
    // The same findings in another order: the same comparison.
    let mut reversed = findings.to_vec();
    reversed.reverse();
    assert_eq!(compared(&reversed), comparison);
}

#[test]
fn a_match_is_never_stronger_than_what_was_observed() {
    let findings = [
        described("GET /orders/{order_id}"),
        seen("GET /orders/{p1}", EvidenceLevel::Inferred),
    ];
    let comparison = compared(&findings);
    let matched = entry(&comparison, "GET", "/orders/{}");
    assert_eq!(matched.result, ComparisonResult::Matched);
    assert_eq!(matched.level, EvidenceLevel::Inferred);
}

#[test]
fn an_observed_placeholder_fits_no_described_literal_and_the_most_literal_takes_a_path() {
    let findings = [
        described("GET /orders/summary"),
        described("GET /orders/{order_id}"),
        seen("GET /orders/summary", EvidenceLevel::Discovered),
        // Another verb and another place are other keys.
        seen("PUT /orders/A-1", EvidenceLevel::Discovered),
    ];
    let comparison = compared(&findings);
    assert_eq!(
        entry(&comparison, "GET", "/orders/summary").result,
        ComparisonResult::Matched
    );
    assert_eq!(
        entry(&comparison, "GET", "/orders/{}").result,
        ComparisonResult::DescribedOnly
    );
    assert_eq!(
        entry(&comparison, "PUT", "/orders/A-1").result,
        ComparisonResult::ObservedOnly
    );

    let placeholder = compared(&[
        described("GET /orders/summary"),
        seen("GET /orders/{p1}", EvidenceLevel::Discovered),
    ]);
    assert_eq!(
        entry(&placeholder, "GET", "/orders/summary").result,
        ComparisonResult::DescribedOnly
    );
    assert_eq!(
        entry(&placeholder, "GET", "/orders/{}").result,
        ComparisonResult::ObservedOnly
    );
}

#[test]
fn a_place_nobody_observed_or_whose_coverage_is_unknown_cannot_be_compared() {
    let elsewhere = at(
        "https://other.example.test",
        "call",
        "GET /",
        Source::LiveSystem,
        EvidenceLevel::Discovered,
        Coverage::Complete,
    );
    let comparison = compared(&[described("GET /orders"), elsewhere]);
    let unknown = entry(&comparison, "GET", "/orders");
    assert_eq!(unknown.result, ComparisonResult::Unknown);
    assert_eq!(unknown.reason, Some(NotCompared::NotObserved));
    assert_eq!(unknown.level, EvidenceLevel::Inferred);

    let vague = at(
        ORIGIN,
        "call",
        "GET /",
        Source::LiveSystem,
        EvidenceLevel::Discovered,
        Coverage::Unknown,
    );
    let comparison = compared(&[described("GET /orders"), vague]);
    let unknown = entry(&comparison, "GET", "/orders");
    assert_eq!(unknown.result, ComparisonResult::Unknown);
    assert_eq!(unknown.reason, Some(NotCompared::CoverageUnknown));
}

#[test]
fn the_source_is_the_side_and_a_finding_with_no_key_is_unknown() {
    let artifact = at(
        ORIGIN,
        "claim",
        "GET /orders",
        Source::Artifact,
        EvidenceLevel::Inferred,
        Coverage::Complete,
    );
    let reasoned = at(
        ORIGIN,
        "claim",
        "GET /notes",
        Source::Reasoning,
        EvidenceLevel::Inferred,
        Coverage::Complete,
    );
    let keyless = described("unreadable");
    let findings = [
        artifact,
        reasoned,
        keyless,
        seen("GET /", EvidenceLevel::Discovered),
    ];
    let comparison = compared(&findings);
    // The file is described; the reasoning is on neither side.
    assert_eq!(
        entry(&comparison, "GET", "/orders").result,
        ComparisonResult::DescribedOnly
    );
    assert!(
        comparison
            .entries
            .iter()
            .all(|one| one.key.as_ref().is_none_or(|key| key.path() != "/notes"))
    );
    let no_key = comparison
        .entries
        .iter()
        .find(|one| one.key.is_none())
        .unwrap();
    assert_eq!(no_key.result, ComparisonResult::Unknown);
    assert_eq!(no_key.reason, Some(NotCompared::NoKey));
    assert_eq!(no_key.described, [findings[2].id()]);
    assert_eq!(comparison.entries.len(), 3, "{comparison:?}");
}

const MANIFEST: &str = r#"manifest_format = 1
contract = 1

[provider]
name = "keyed"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[surface_kinds]]
name = "web"
archetype = "interface"

[[finding_kinds]]
name = "claim"
surface_kinds = ["web"]

[[finding_kinds]]
name = "call"
surface_kinds = ["web"]

[effects]
network = "surfaces"
"#;

/// A pack that registers key rules for the kinds named.
struct Keyed(&'static [&'static str]);

impl Pack for Keyed {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, _cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        for kind in self.0 {
            to.key_rule(FindingKind::new(kind).unwrap(), Arc::new(Spaced))?;
        }
        Ok(())
    }
}

fn registry_of(pack: &Keyed) -> (Registry, Vec<a2m_provider::RegistryProblem>) {
    let folder = Folder::new("compare");
    let mut builder =
        RegistryBuilder::new(ProviderSettings::default(), host(&folder, Roots::none()));
    builder.add_pack(pack);
    builder.build()
}

#[test]
fn a_key_rule_is_registered_only_for_a_kind_the_pack_owns_and_once() {
    for kinds in [&["claim", "other"][..], &["claim", "claim"][..]] {
        let (_, problems) = registry_of(&Keyed(Box::leak(kinds.to_vec().into_boxed_slice())));
        assert_eq!(problems.len(), 1, "{kinds:?}: {problems:?}");
        assert_eq!(
            (
                problems[0].failure.class(),
                problems[0].failure.code().as_str()
            ),
            (FailureClass::Policy, "effect_not_declared"),
            "{kinds:?}"
        );
    }
}

#[test]
fn the_registry_compares_by_the_rule_of_each_kind_and_leaves_out_a_kind_with_none() {
    let (registry, problems) = registry_of(&Keyed(&["claim", "call"]));
    assert!(problems.is_empty(), "{problems:?}");
    let unruled = at(
        ORIGIN,
        "screen",
        "GET /screens",
        Source::LiveSystem,
        EvidenceLevel::Discovered,
        Coverage::Partial,
    );
    let findings = [
        described("GET /orders/{order_id}"),
        seen("GET /orders/A-1", EvidenceLevel::Discovered),
        unruled,
    ];
    let comparison = registry.compare(&findings);
    assert_eq!(comparison.entries.len(), 1, "{comparison:?}");
    assert_eq!(comparison.entries[0].result, ComparisonResult::Matched);

    // Without the rule of the observed kind, nothing observed is keyed,
    // and the place is not covered.
    let (registry, _) = registry_of(&Keyed(&["claim"]));
    let comparison = registry.compare(&findings);
    assert_eq!(comparison.entries.len(), 1, "{comparison:?}");
    assert_eq!(comparison.entries[0].reason, Some(NotCompared::NotObserved));
}
