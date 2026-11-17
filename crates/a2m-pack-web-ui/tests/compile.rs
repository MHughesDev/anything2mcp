//! The pack as the kernel sees it before anything runs: a registry that
//! holds only this pack, the fixture target compiled through it, and what
//! the file kind, the lowering, and the lint rules refuse. No test here
//! starts a browser.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::{FailureClass, NoSecrets};
use a2m_files::Level;
use a2m_pack_web_browser::KNOWN_PLACES;
use a2m_pack_web_ui::SharedBrowsers;
use a2m_provider::{Manifest, Pack};
use common::{Stand, compile, compile_changed, shown, ui_mode};

fn stand(name: &str) -> Stand {
    Stand::new(name, Arc::new(NoSecrets))
}

#[test]
fn a_registry_with_only_this_pack_builds_and_its_digest_is_stable() {
    let first = stand("registry");
    let second = stand("registry");
    assert_eq!(first.registry.digest(), second.registry.digest());
    assert_eq!(first.registry.modes().collect::<Vec<_>>(), vec![&ui_mode()]);
    let handle = first.registry.driver(&ui_mode()).unwrap();
    assert_eq!(handle.choice().provider.as_str(), "web");
    // Registering started nothing.
    assert!(first.supervisor.list().is_empty());

    let own = a2m_pack_web_ui::pack(SharedBrowsers::default());
    let manifest = Manifest::parse(own.manifest()).unwrap();
    // The manifest declares every failure code this pack produces, each in
    // its one class, and nothing it does not produce.
    let declared: Vec<(FailureClass, String)> = manifest
        .failure_codes
        .iter()
        .map(|code| (code.class, code.code.as_str().to_owned()))
        .collect();
    let produced: Vec<(FailureClass, String)> = a2m_pack_web_ui::codes::ALL
        .iter()
        .map(|(class, code)| (*class, (*code).to_owned()))
        .collect();
    let sorted = |mut list: Vec<(FailureClass, String)>| {
        list.sort_by(|a, b| a.1.cmp(&b.1));
        list
    };
    assert_eq!(sorted(declared), sorted(produced));
    // The step kinds the lowering makes are the mode's, and no others.
    let steps = &manifest.modes[0].steps;
    for kind in [
        a2m_pack_web_ui::GOTO,
        a2m_pack_web_ui::WAIT,
        a2m_pack_web_ui::READ,
        a2m_pack_web_ui::SET,
        a2m_pack_web_ui::INVOKE,
    ] {
        assert!(steps.contains(&kind), "{kind}");
    }
    assert_eq!(steps.len(), 5);
    // Every place the browser engine looks for a browser is declared, so
    // that the supervisor will start what is found there.
    let declared: Vec<&str> = manifest.requirements[0]
        .paths
        .iter()
        .map(String::as_str)
        .collect();
    let known: Vec<&str> = KNOWN_PLACES.iter().map(|place| place.path).collect();
    assert_eq!(declared, known);
    // The browsers' data directories, and nothing else, may be written.
    let roots: Vec<&str> = manifest
        .effects
        .write_roots
        .iter()
        .map(|root| root.as_str())
        .collect();
    assert_eq!(roots, ["profiles"]);
}

#[test]
fn the_fixture_target_compiles_twice_to_the_same_bytes() {
    let stand = stand("compile");
    let first = compile(&stand.registry, "example");
    let second = compile(&stand.registry, "example");
    assert_eq!(first.encoded.bytes, second.encoded.bytes);
    assert_eq!(first.encoded.hash, second.encoded.hash);

    // What a write lowers to: one commit point, which is the action, and a
    // read-back that is a lookup and never commits.
    let plan = first.plan;
    let update = plan
        .operations
        .iter()
        .find(|operation| operation.name.as_str() == "example_line_item_update")
        .unwrap();
    let binding = &update.bindings[0];
    let kinds = |steps: &[a2m_core::Step]| -> Vec<String> {
        steps.iter().map(|step| step.kind.to_string()).collect()
    };
    assert_eq!(
        kinds(&binding.act),
        ["goto", "wait", "set", "set", "set", "invoke"]
    );
    let commits: Vec<bool> = binding.act.iter().map(|step| step.commits).collect();
    assert_eq!(commits, [false, false, false, false, false, true]);
    assert!(binding.confirm.is_empty());
    assert_eq!(kinds(&binding.read_back), ["goto", "wait", "read"]);
    assert!(
        binding
            .read_back
            .iter()
            .all(|step| step.lookup && !step.commits)
    );
    // The locators are in the pack's section and nowhere in a step; the
    // plan holds the path's template and no address.
    let section = plan
        .packs
        .iter()
        .find(|section| section.pack == "web")
        .unwrap();
    let text = String::from_utf8(section.payload.as_bytes().to_vec()).unwrap();
    assert!(text.contains(r#"{"hole":"order_id"}"#), "{text}");
    assert!(text.contains("Description"), "{text}");
    assert!(!text.contains("127.0.0.1"), "{text}");
    for step in binding.act.iter().chain(&binding.read_back) {
        let payload = String::from_utf8(step.payload.as_bytes().to_vec()).unwrap();
        assert!(!payload.contains("textbox"), "{payload}");
    }
}

/// Compiles the `example` fixture with one file changed, and gives the
/// first problem with the code given.
fn refused(path: &str, change: impl Fn(&str) -> String, code: &str) -> a2m_files::Diagnostic {
    let stand = stand("refused");
    let problems = compile_changed(&stand.registry, "example", path, change)
        .err()
        .unwrap_or_else(|| panic!("the change compiled; `{code}` was expected"));
    problems
        .find(code)
        .unwrap_or_else(|| panic!("no `{code}` in:\n{}", shown(&problems)))
        .clone()
}

#[test]
fn what_a_screen_may_not_say_is_refused_with_a_file_a_line_and_a_fix() {
    let form = "screens/line_item.yaml";

    // A control no enabled pack registered.
    let control = refused(
        form,
        |text| text.replacen("control: number", "control: slider", 1),
        "kernel.unknown_control",
    );
    assert_eq!((control.at.file.as_str(), control.at.line), (form, 16));
    assert!(!control.fix.is_empty());

    // A key the file kind does not know.
    let unknown = refused(
        form,
        |text| text.replacen("commits: true", "retries: 3", 1),
        "kernel.unknown_key",
    );
    assert_eq!(unknown.at.file.as_str(), form);
    assert!(!unknown.fix.is_empty());

    // A locator that matches in two ways at once, and one by position.
    let two = refused(
        form,
        |text| {
            text.replacen(
                "- {label: Description}",
                "- {label: Description, test_id: d}",
                1,
            )
        },
        "web.locator_invalid",
    );
    assert_eq!((two.at.file.as_str(), two.at.line), (form, 12));
    refused(
        form,
        |text| text.replacen("- {label: Description}", "- {xpath: '//input[2]'}", 1),
        "kernel.unknown_key",
    );

    // A selector that is not the last locator, and a selector at all.
    let not_last = refused(
        form,
        |text| {
            text.replacen(
                "- {role: textbox, name: Description}",
                "- {css: '#description'}",
                1,
            )
        },
        "web.css_not_last",
    );
    assert_eq!((not_last.at.line, not_last.level), (11, Level::Error));
    let stand = stand("warned");
    let warned = compile_changed(&stand.registry, "example", form, |text| {
        text.replacen("- {label: Description}", "- {css: '#description'}", 1)
    })
    .unwrap();
    let used = warned.warnings.find("web.css_used").unwrap();
    assert_eq!(used.level, Level::Warning);

    // A field with no fingerprint.
    let bare = refused(
        form,
        |text| {
            text.replacen(
                "    fingerprint: {section: {role: group, name: Dimensions}, near_text: inches}\n",
                "",
                1,
            )
        },
        "web.no_fingerprint",
    );
    assert!(bare.message.contains("`length_in`"), "{bare}");

    // An address that could leave the surface, and a placeholder with no input.
    for bad in [
        "//other.example.test/orders",
        "/orders/../admin",
        "/orders?x=1",
    ] {
        let reach = refused(
            form,
            |text| {
                text.replacen(
                    "\"/orders/{order_id}/items/{item_code}\"",
                    &format!("\"{bad}\""),
                    1,
                )
            },
            "web.reach_invalid",
        );
        assert_eq!(reach.at.line, 4, "{bad}");
    }
    let unbound = refused(
        form,
        |text| text.replacen("{item_code}", "{item}", 1),
        "web.placeholder_unbound",
    );
    assert_eq!(unbound.at.file.as_str(), "operations/line_items.yaml");

    // A binding that names no screen, a field the screen lacks, an output
    // with no field, and an action the screen lacks.
    let operations = "operations/line_items.yaml";
    refused(
        operations,
        |text| text.replacen("screen: line_item", "screen: line_form", 1),
        "web.unknown_screen",
    );
    refused(
        operations,
        |text| text.replacen("quantity: input.quantity", "amount: input.quantity", 1),
        "web.unknown_field",
    );
    let output = refused(
        operations,
        |text| text.replacen("description: {type: text}", "summary: {type: text}", 1),
        "web.unknown_field",
    );
    assert!(output.message.contains("`summary`"), "{output}");
    refused(
        operations,
        |text| text.replacen("action: save", "action: store", 1),
        "web.unknown_action",
    );
    // A record read back through a screen that cannot say it is not there.
    refused(
        form,
        |text| text.replacen("handling: absent", "handling: stop", 1),
        "web.read_back_never_absent",
    );
}
