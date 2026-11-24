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
    let page_http = common::page_http_mode();
    let site_tools = common::site_tools_mode();
    assert_eq!(
        first.registry.modes().collect::<Vec<_>>(),
        vec![&page_http, &site_tools, &ui_mode()]
    );
    let handle = first.registry.driver(&site_tools).unwrap();
    assert_eq!(handle.choice().provider.as_str(), "web");
    let handle = first.registry.driver(&page_http).unwrap();
    assert_eq!(handle.choice().provider.as_str(), "web");
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
    // The pack's own codes, which keep the class the crates below give the
    // same code, then the browser engine's it passes on.
    let book = a2m_core::CodeBook::new(&[
        a2m_pack_web_ui::codes::TABLE,
        a2m_pack_web_browser::codes::TABLE,
        a2m_core::codes::TABLE,
        a2m_provider::codes::TABLE,
        a2m_auth::codes::TABLE,
    ])
    .unwrap();
    let passed_on = a2m_pack_web_ui::codes::PASSED_ON.iter().map(|code| {
        let entry = book.explain(code.as_str()).unwrap();
        assert_eq!(entry.declared_in, "a2m-pack-web-browser", "{code}");
        (entry.class, entry.code)
    });
    let produced: Vec<(FailureClass, String)> = a2m_pack_web_ui::codes::ALL
        .iter()
        .copied()
        .chain(passed_on)
        .map(|(class, code)| (class, code.to_owned()))
        .collect();
    let sorted = |mut list: Vec<(FailureClass, String)>| {
        list.sort_by(|a, b| a.1.cmp(&b.1));
        list
    };
    assert_eq!(sorted(declared), sorted(produced));
    // Every mode is given long enough to prepare for a browser to start
    // to the end of its launch deadline and a tab to open after it, so the
    // engine never drops a slow first start before the pack itself gives
    // it up (decision 0999).
    let browser = a2m_pack_web_browser::BrowserSettings::default();
    for mode in &manifest.modes {
        let within = mode.prepare_within().unwrap();
        assert!(
            within >= browser.launch_deadline + browser.call_deadline,
            "{}",
            mode.name
        );
    }
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
    // And those of `page_http`: neither saves.
    let page_steps = &manifest.modes[1].steps;
    assert_eq!(manifest.modes[1].name, page_http);
    assert_eq!(
        page_steps,
        &vec![a2m_pack_web_ui::GOTO, a2m_pack_web_ui::REQUEST]
    );
    assert!(manifest.modes[1].commit_steps.is_empty());
    // And those of `site_tools`: the call is the one step that saves.
    let tool_steps = &manifest.modes[2].steps;
    assert_eq!(manifest.modes[2].name, site_tools);
    assert_eq!(
        tool_steps,
        &vec![
            a2m_pack_web_ui::GOTO,
            a2m_pack_web_ui::WAIT,
            a2m_pack_web_ui::CALL_TOOL
        ]
    );
    assert_eq!(
        manifest.modes[2].commit_steps,
        vec![a2m_pack_web_ui::CALL_TOOL]
    );
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

/// The hash of the compiled target the `example` pages target compiles to.
/// Held in the source so that each of the three CI systems compares its own
/// build to the same value (package 1.3): the compiled target's bytes carry
/// no line ending of a system, and no path or time of a machine.
const PAGES_COMPILED_TARGET_HASH: &str =
    "b7964c167c02b0501e690dd8083c2d794e4aba382193b9d8c3811416a51caf31";

#[test]
fn the_pages_target_compiles_to_the_same_compiled_target_bytes_on_every_machine() {
    let stand = stand("compiled_target_hash");
    let compiled = compile(&stand.registry, "example");
    assert!(!compiled.encoded.bytes.contains(&b'\r'));
    assert_eq!(
        compiled.encoded.hash.to_string(),
        PAGES_COMPILED_TARGET_HASH
    );
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
    let compiled_target = first.compiled_target;
    let update = compiled_target
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
    // compiled target holds the path's template and no address.
    let section = compiled_target
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
fn a_binding_is_held_to_the_least_effect_of_what_it_does() {
    // A `submit` cannot be taken back, and an operation that says it only
    // writes does not compile.
    let ops = "operations/sign_in.yaml";
    let submit = refused(
        ops,
        |text| text.replacen("effect: irreversible", "effect: write", 1),
        "kernel.effect_lower_than_derived",
    );
    assert_eq!(submit.at.file.as_str(), ops);
    // An update that names no read-back could never be found again: it is
    // irreversible too.
    refused(
        "operations/line_items.yaml",
        |text| text.replacen("    read_back: {screen: line_item}\n", "", 1),
        "kernel.effect_lower_than_derived",
    );
}

#[test]
fn what_a_screen_may_not_say_is_refused_with_a_file_a_line_and_a_fix() {
    let form = "screens/line_item.yaml";

    // A control no enabled pack registered.
    let control = refused(
        form,
        |text| text.replacen("control: number", "control: signature_pad", 1),
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
    // A download whose name and size go to one output, and one that gives
    // an output the operation does not declare (decision 0421).
    let downloads = |download: &'static str| {
        move |text: &str| {
            text.replacen(
                "    commits: true\n",
                &format!("    commits: true\n    download: {download}\n"),
                1,
            )
        }
    };
    refused(
        form,
        downloads("{most_bytes: 10, into: file, size_into: file}"),
        "web.download_invalid",
    );
    let undeclared = refused(
        form,
        downloads("{most_bytes: 10, into: file}"),
        "web.unknown_field",
    );
    assert!(undeclared.message.contains("`file`"), "{undeclared}");
}

/// The review states where a web tool sends, which the browser holds it to
/// (decision 0292): its surface, and the destinations its operation
/// declares; it starts no program and sets no variable.
#[test]
fn the_review_states_that_a_web_tool_sends_to_its_surface_and_its_declared_destinations() {
    let stand = stand("review");
    let compiled = compile_changed(
        &stand.registry,
        "example",
        "operations/line_items.yaml",
        |text| {
            text.replacen(
                "description: >\n  Reads one line item",
                "permissions:\n  destinations: [\"https://assets.example.com\"]\ndescription: >\n  Reads one line item",
                1,
            )
        },
    )
    .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    assert!(!compiled.review.items.is_empty());
    for item in &compiled.review.items {
        for binding in &item.bindings {
            assert_eq!(
                binding.states,
                a2m_files::BindingStates::Stated {
                    runs: Vec::new(),
                    environment: Vec::new()
                },
                "{}",
                item.tool
            );
        }
        let a2m_files::Reach::Stated { groups, .. } = &item.reach else {
            panic!("{}: {:?}", item.tool, item.reach);
        };
        let addresses: Vec<&str> = groups
            .iter()
            .flat_map(|group| group.addresses.iter().map(String::as_str))
            .collect();
        let mut expected = vec!["http://127.0.0.1:4010"];
        if item.tool.as_str() == "example_line_item_get" {
            expected.push("https://assets.example.com");
        }
        let mut addresses = addresses;
        addresses.sort_unstable();
        expected.sort_unstable();
        assert_eq!(addresses, expected, "{}", item.tool);
    }
}

/// Decision 0404: a page request's shapes are read by the provider
/// contract's shared shape, and a page request takes a part of it in this
/// version. What the file kind's own schema refused before is still
/// refused, now under `web.request_shape_invalid` at the word or value; and
/// what the shared shape refuses that this pack also refused stays refused.
#[test]
fn a_page_requests_shape_takes_only_flat_typed_properties_of_the_shared_shape() {
    let stand = stand("shape");
    let entry = "page_requests/order_totals.yaml";
    compile_changed(&stand.registry, "secure", entry, str::to_owned)
        .unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    for (from, to) in [
        (
            "order_id: {type: string}",
            "order_id: {type: string, nullable: true}",
        ),
        (
            "order_id: {type: string}",
            "order_id: {type: string, enum: [a]}",
        ),
        ("order_id: {type: string}", "order_id: {type: any}"),
        (
            "order_id: {type: string}",
            "order_id: {type: object, properties: {a: {type: string}}}",
        ),
        ("order_id: {type: string}", "order_id: {type: text}"),
        ("  type: object\n", "  type: object\n  nullable: true\n"),
        ("  type: object\n", "  type: array\n"),
        (
            "  required: [order_id, total_quantity]\n",
            "  required: order_id\n",
        ),
        (
            "  required: [order_id, total_quantity]\n",
            "  required: [order_id, total_quantity, other]\n",
        ),
        ("  type: object\n", "  type: object\n  pattern: x\n"),
    ] {
        let problems = compile_changed(&stand.registry, "secure", entry, |text| {
            assert!(text.contains(from), "{from}");
            text.replacen(from, to, 1)
        })
        .err()
        .unwrap_or_else(|| panic!("`{to}` compiled"));
        let found = problems
            .find("web.request_shape_invalid")
            .unwrap_or_else(|| panic!("`{to}`:\n{}", shown(&problems)));
        assert_eq!(found.at.file.as_str(), entry, "{to}");
        assert!(found.at.line > 1, "{to}: {found:?}");
    }
}
