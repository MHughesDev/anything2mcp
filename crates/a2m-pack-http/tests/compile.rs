//! The pack as the kernel sees it before anything runs: a registry that
//! holds only this pack, the fixture targets compiled through it, and what
//! the file kind, the lowering, and the lint rules refuse.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{FailureClass, NoSecrets, StepKind};
use a2m_files::Level;
use a2m_provider::Manifest;
use common::{Folder, compile, compile_changed, http_mode, registry, shown};

#[test]
fn a_registry_with_only_this_pack_builds_and_its_digest_is_stable() {
    let folder = Folder::new("registry");
    let first = registry(&folder, Arc::new(NoSecrets));
    let second = registry(&folder, Arc::new(NoSecrets));
    assert_eq!(first.digest(), second.digest());
    assert_eq!(first.modes().collect::<Vec<_>>(), vec![&http_mode()]);
    let handle = first.driver(&http_mode()).unwrap();
    assert_eq!(handle.choice().provider.as_str(), "http");

    // The manifest declares every failure code this pack produces, each in
    // its one class, and nothing it does not produce.
    let manifest = Manifest::parse(a2m_provider::Pack::manifest(&a2m_pack_http::HttpPack)).unwrap();
    let declared: Vec<(FailureClass, String)> = manifest
        .failure_codes
        .iter()
        .map(|code| (code.class, code.code.as_str().to_owned()))
        .collect();
    let produced: Vec<(FailureClass, String)> = a2m_pack_http::codes::ALL
        .iter()
        .map(|(class, code)| (*class, (*code).to_owned()))
        .collect();
    assert_eq!(declared, produced);
    // The step kinds the lowering and the discoverers make are the mode's.
    let steps = &manifest.modes[0].steps;
    for kind in [
        a2m_pack_http::REQUEST,
        a2m_pack_http::VALIDATE,
        a2m_pack_http::EXTRACT,
        a2m_pack_http::PROBE,
    ] {
        assert!(steps.contains(&kind), "{kind}");
    }
    assert_eq!(steps.len(), 4);
    assert!(!steps.contains(&StepKind::fixed("token")));
}

#[test]
fn the_fixture_targets_compile_twice_to_the_same_bytes() {
    let folder = Folder::new("compile");
    let registry = registry(&folder, Arc::new(NoSecrets));
    for name in ["example", "secured"] {
        let first = compile(&registry, name);
        let second = compile(&registry, name);
        assert_eq!(first.encoded.bytes, second.encoded.bytes, "{name}");
        assert_eq!(first.encoded.hash, second.encoded.hash, "{name}");
    }

    // What a write lowers to: one commit point, a confirm, and a read-back
    // that is a lookup and never commits.
    let plan = compile(&registry, "example").plan;
    let upsert = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_line_item_upsert")
        .unwrap();
    let binding = &upsert.bindings[0];
    let kinds = |steps: &[a2m_core::Step]| -> Vec<String> {
        steps.iter().map(|step| step.kind.to_string()).collect()
    };
    assert_eq!(kinds(&binding.act), ["request"]);
    assert!(binding.act[0].commits && !binding.act[0].lookup);
    assert_eq!(kinds(&binding.confirm), ["validate"]);
    assert_eq!(
        kinds(&binding.read_back),
        ["request", "validate", "extract"]
    );
    assert!(
        binding
            .read_back
            .iter()
            .all(|step| step.lookup && !step.commits)
    );
    // The plan holds the template, and no address: the origin is the surface's.
    let section = plan
        .packs
        .iter()
        .find(|section| section.pack == "http")
        .unwrap();
    let text = String::from_utf8(section.payload.as_bytes().to_vec()).unwrap();
    assert!(text.contains(r#"{"hole":"order_id"}"#), "{text}");
    assert!(!text.contains("127.0.0.1"), "{text}");
}

/// Compiles the `example` fixture with one file changed, and gives the
/// first problem with the code given.
fn refused(path: &str, change: impl Fn(&str) -> String, code: &str) -> a2m_files::Diagnostic {
    let folder = Folder::new("refused");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let problems = compile_changed(&registry, "example", path, change)
        .err()
        .unwrap_or_else(|| panic!("the change compiled; `{code}` was expected"));
    problems
        .find(code)
        .unwrap_or_else(|| panic!("no `{code}` in:\n{}", shown(&problems)))
        .clone()
}

#[test]
fn what_an_endpoint_may_not_say_is_refused_with_a_file_a_line_and_a_fix() {
    let orders = "endpoints/orders.yaml";

    // A key the file kind does not know.
    let unknown = refused(
        orders,
        |text| text.replacen("auth: none", "retries: 3", 1),
        "kernel.unknown_key",
    );
    assert_eq!((unknown.at.file.as_str(), unknown.at.line), (orders, 5));
    assert!(!unknown.fix.is_empty());

    // A word in a shape that this pack does not check.
    let word = refused(
        orders,
        |text| {
            text.replacen(
                "order_id: {type: string}",
                "order_id: {type: string, pattern: '^A'}",
                1,
            )
        },
        "http.shape_invalid",
    );
    assert_eq!((word.at.file.as_str(), word.at.line), (orders, 14));
    assert!(
        word.message.contains("`pattern`") && !word.fix.is_empty(),
        "{word}"
    );

    // A path that names another host, and one that mixes a placeholder with text.
    for bad in [
        "http://other.example.test/api/orders",
        "/api/orders-{order_id}",
        "/api/../admin",
    ] {
        let path = refused(
            orders,
            |text| text.replacen("\"/api/orders\"", &format!("\"{bad}\""), 1),
            "http.path_invalid",
        );
        assert_eq!(path.at.line, 4, "{bad}");
        assert_eq!(path.level, Level::Error);
    }

    // A placeholder with no input and no entry under `path_params`.
    let unbound = refused(
        orders,
        |text| text.replacen("\"/api/orders/{order_id}\"", "\"/api/orders/{order}\"", 1),
        "http.placeholder_unbound",
    );
    assert_eq!(unbound.at.file.as_str(), "operations/orders.yaml");
    assert!(unbound.fix.contains("path_params"), "{unbound}");

    // An output the endpoint's declared shape does not hold.
    let extract = refused(
        "operations/orders.yaml",
        |text| text.replacen("customer: {type: text}", "buyer: {type: text}", 1),
        "http.extract_unknown",
    );
    assert!(extract.message.contains("`buyer`"), "{extract}");

    // A binding that names no endpoint of the catalog.
    let missing = refused(
        "operations/orders.yaml",
        |text| text.replacen("endpoint: order_get", "endpoint: order_fetch", 1),
        "http.unknown_endpoint",
    );
    assert_eq!(missing.at.file.as_str(), "operations/orders.yaml");

    // A read verb bound to an endpoint that writes.
    refused(
        "operations/orders.yaml",
        |text| text.replacen("endpoint: order_get", "endpoint: line_item_put", 1),
        "http.effect_mismatch",
    );
    // A body field the endpoint does not declare.
    refused(
        "operations/line_items.yaml",
        |text| text.replacen("quantity: input.quantity", "amount: input.quantity", 1),
        "http.body_undeclared",
    );
    // An endpoint that sends a record's version as a precondition, read
    // back through an endpoint that reads no version: the write could only
    // ever go out with no precondition.
    let unread = refused(
        "endpoints/line_items.yaml",
        |text| text.replacen("version:\n  read: {field: version}\n", "", 1),
        "http.version_invalid",
    );
    assert_eq!(unread.at.file.as_str(), "operations/line_items.yaml");
    assert!(unread.message.contains("`line_item_get`"), "{unread}");
    // What is not built is refused by name, not ignored.
    let not_built = refused(
        orders,
        |text| text.replacen("auth: none", "auth: none\nanti_forgery: {cookie: xsrf}", 1),
        "http.not_built",
    );
    assert_eq!(not_built.at.line, 6);
}

#[test]
fn a_write_never_compiles_as_less_than_what_its_binding_does() {
    let items = "operations/line_items.yaml";
    // The upsert declared a read, with no commit point.
    refused(
        items,
        |text| {
            text.replacen("effect: write\n", "effect: read\n", 1)
                .replacen("commit_points: 1\n", "", 1)
        },
        "kernel.effect_lower_than_derived",
    );
    // A delete cannot be taken back, whatever reads it back.
    refused(
        items,
        |text| text.replacen("verb: upsert", "verb: delete", 1),
        "kernel.effect_lower_than_derived",
    );
}

#[test]
fn the_lint_rules_find_what_they_name() {
    let orders = "endpoints/orders.yaml";

    let secret = refused(
        orders,
        |text| {
            text.replacen(
                "headers: {accept: \"application/json\"}",
                "headers: {accept: \"application/json\", authorization: \"Bearer secret-for-tests-0001\"}",
                1,
            )
        },
        "http.secret_literal",
    );
    assert_eq!((secret.at.file.as_str(), secret.at.line), (orders, 7));
    assert!(!secret.message.contains("secret-for-tests"), "{secret}");
    refused(
        orders,
        |text| text.replacen("auth: none", "auth: none\nquery: {api_key: \"abc\"}", 1),
        "http.secret_literal",
    );

    let shapeless = refused(
        orders,
        |text| {
            let (before, after) = text.split_once("---").unwrap();
            let kept = before.split_once("response_schema:").unwrap().0;
            format!("{kept}---{after}")
        },
        "http.no_response_schema",
    );
    assert_eq!(shapeless.at.file.as_str(), orders);

    let signed = refused(
        orders,
        |text| {
            text.replacen("auth: none", "auth: none\nsigned: true", 2)
                .replacen("signed: true", "", 1)
        },
        "http.signed_endpoint",
    );
    assert_eq!(signed.at.file.as_str(), "operations/orders.yaml");

    // An endpoint marked `read` whose method HTTP does not mark safe is
    // refused, unless it says so with `read_asserted` (decision 0249); and
    // `read_asserted` where nothing needs it is refused too.
    let unasserted = refused(
        orders,
        |text| text.replacen("method: GET", "method: POST", 1),
        "http.effect_method",
    );
    assert_eq!((unasserted.level, unasserted.at.line), (Level::Error, 3));
    let folder = Folder::new("asserted");
    let registry = registry(&folder, Arc::new(NoSecrets));
    let compiled = compile_changed(&registry, "example", orders, |text| {
        text.replacen("method: GET", "method: POST", 1).replacen(
            "effect: read\n",
            "effect: read\nread_asserted: true\n",
            1,
        )
    })
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    assert!(compiled.warnings.find("http.effect_method").is_none());
    refused(
        orders,
        |text| text.replacen("effect: read\n", "effect: read\nread_asserted: true\n", 1),
        "http.effect_method",
    );
}

/// A method override in a header or as the `_method` query entry is
/// judged as the method it names (decision 0270): an endpoint marked
/// `read` whose override names a method HTTP does not mark safe, or one
/// that is not a method at all, is refused, `read_asserted` or not, and so
/// is a binding that computes `_method` for an endpoint marked `read`. An
/// override that names a safe method changes nothing.
#[test]
fn a_read_whose_method_override_names_a_method_that_may_write_is_refused() {
    let orders = "endpoints/orders.yaml";
    let accept = "headers: {accept: \"application/json\"}";
    for header in [
        "X-HTTP-Method-Override: DELETE",
        "x-http-method: put",
        "X-Method-Override: PROPFIND",
    ] {
        let found = refused(
            orders,
            |text| {
                text.replacen(
                    accept,
                    &format!("headers: {{accept: \"application/json\", {header}}}"),
                    1,
                )
            },
            "http.effect_method",
        );
        assert_eq!(
            (found.level, found.at.file.as_str()),
            (Level::Error, orders),
            "{header}"
        );
    }
    refused(
        orders,
        |text| text.replacen("auth: none", "auth: none\nquery: {_method: \"PUT\"}", 1),
        "http.effect_method",
    );
    // The assertion that a `POST` only reads does not cover an override.
    refused(
        orders,
        |text| {
            text.replacen("method: GET", "method: POST", 1)
                .replacen("effect: read\n", "effect: read\nread_asserted: true\n", 1)
                .replacen(accept, "headers: {X-HTTP-Method-Override: DELETE}", 1)
        },
        "http.effect_method",
    );
    // A binding that computes the override of a read.
    let computed = refused(
        "operations/orders.yaml",
        |text| {
            text.replacen(
                "    endpoint: order_get\n",
                "    endpoint: order_get\n    query: {_method: input.order_id}\n",
                1,
            )
        },
        "http.effect_method",
    );
    assert_eq!(computed.at.file.as_str(), "operations/orders.yaml");
    // An override that names a safe method compiles.
    let folder = Folder::new("safe-override");
    let registry = registry(&folder, Arc::new(NoSecrets));
    compile_changed(&registry, "example", orders, |text| {
        text.replacen(
            accept,
            "headers: {accept: \"application/json\", X-HTTP-Method-Override: HEAD}",
            1,
        )
    })
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
}

/// A text answer's pattern is bounded in its length when the endpoint is
/// compiled, so that what is matched at run time is bounded too (decision
/// 0270).
#[test]
fn a_pattern_longer_than_its_bound_is_refused() {
    let found = refused(
        "endpoints/status.yaml",
        |_| {
            let pattern = format!("(?P<count>[0-9]+){}", "x?".repeat(3_000));
            format!(
                "endpoint: status_text_get\nsurface: api\nmethod: GET\npath: \"/views/text/status\"\nauth: none\neffect: read\nanswer:\n  form: text\n  pattern: '{pattern}'\n  fields:\n    count: {{type: integer}}\n"
            )
        },
        "http.answer_invalid",
    );
    assert!(found.message.contains("4,096 bytes"), "{found}");
}

#[test]
fn a_submit_that_names_no_read_back_states_the_acknowledgement_as_its_evidence() {
    let folder = Folder::new("evidence");
    let registry = registry(&folder, Arc::new(NoSecrets));
    // The version a write sends is read back through the record: with no
    // read-back the endpoint sends none.
    let unversioned = |text: &str| text.replace("version:\n  send: {body: version}\n", "");
    let submit = |text: &str| {
        text.replace("verb: upsert", "verb: submit")
            .replace("    read_back: {endpoint: line_item_get}\n", "")
            .replace(
                "effect: write\nkey: [order_id, item_code]\n",
                "effect: irreversible\n",
            )
    };
    let plan = common::compile_changed_files(
        &registry,
        "example",
        &[
            ("endpoints/line_items.yaml", &unversioned),
            ("operations/line_items.yaml", &submit),
        ],
    )
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)))
    .plan;
    let operation = |plan: &a2m_core::Plan| {
        plan.operations
            .iter()
            .find(|operation| operation.name.as_str() == "example_line_item_upsert")
            .unwrap()
            .clone()
    };
    let operation_of_submit = operation(&plan);
    assert_eq!(
        operation_of_submit.bindings[0].evidence,
        a2m_core::WriteEvidence::Acknowledged
    );
    assert_eq!(
        operation_of_submit.effect,
        a2m_core::EffectClass::Irreversible
    );

    // A write that names a read-back keeps it.
    let kept = operation(&compile(&registry, "example").plan);
    assert_eq!(kept.bindings[0].evidence, a2m_core::WriteEvidence::ReadBack);
}
