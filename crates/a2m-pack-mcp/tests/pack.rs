//! The pack as the registry and the compiler see it: its manifest, its
//! names, its fixture target, and its lint rules.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::{Archetype, FailureClass, Values};
use a2m_provider::{Manifest, SurfaceDescription};
use common::{Folder, compile, compiled_fixture, fixture_root, registry, shown, steps, write_target};
use example_mcp::harness::HttpStand;

#[test]
fn a_registry_with_only_this_pack_builds_and_its_digest_is_stable() {
    let folder = Folder::new("registry");
    let one = registry(&folder);
    let other = registry(&folder);
    assert_eq!(one.digest(), other.digest());

    let names = a2m_pack_mcp::Names::new().unwrap();
    assert_eq!(one.archetype(&names.mode), Some(Archetype::Request));
    assert_eq!(one.modes().collect::<Vec<_>>(), [&names.mode]);
    assert!(one.surface_kind(&a2m_pack_mcp::SURFACE_KIND).unwrap().session);
    let handle = one.driver(&names.mode).unwrap();
    assert_eq!(handle.choice().provider.as_str(), a2m_pack_mcp::PACK);
    let providers: Vec<String> = one
        .auth_providers()
        .iter()
        .map(|provider| provider.name().as_str().to_owned())
        .collect();
    assert_eq!(providers, ["upstream_mcp"]);
    let surface = SurfaceDescription {
        kind: a2m_pack_mcp::SURFACE_KIND,
        archetype: Archetype::Request,
        address: "http://127.0.0.1:9/mcp".to_owned(),
        facts: Values::new(),
    };
    let discoverers = one.discoverers_for(&surface);
    assert_eq!(discoverers.len(), 1);
    assert_eq!(discoverers[0].declared().name.as_str(), "mcp_tools");

    let set = one.compiler_set();
    assert_eq!(
        set.file_kinds().collect::<Vec<_>>(),
        [&(a2m_pack_mcp::MAPPING_KIND, a2m_pack_mcp::SURFACE_KIND)]
    );
    let codes: Vec<String> = set
        .lint_codes()
        .into_iter()
        .map(|(code, _)| code)
        .filter(|code| code.starts_with("mcp."))
        .collect();
    assert_eq!(
        codes,
        [
            "mcp.discovery_bound",
            "mcp.free_text",
            "mcp.program_unpinned",
            "mcp.tool_unmapped",
            "mcp.tool_unpinned",
            "mcp.write_no_readback"
        ]
    );
}

#[test]
fn every_failure_code_of_the_pack_is_declared_in_its_manifest_with_its_one_class() {
    let manifest = Manifest::parse(a2m_pack_mcp::MANIFEST).unwrap();
    let declared: Vec<(FailureClass, String)> = manifest
        .failure_codes
        .iter()
        .map(|declared| (declared.class, declared.code.as_str().to_owned()))
        .collect();
    let own: Vec<(FailureClass, String)> = a2m_pack_mcp::codes::ALL
        .iter()
        .map(|(class, code)| (*class, (*code).to_owned()))
        .collect();
    assert_eq!(declared, own);
}

#[test]
fn the_fixture_target_compiles_twice_to_the_same_bytes() {
    let folder = Folder::new("compile");
    let registry = registry(&folder);
    let one = compile(&registry, &fixture_root()).unwrap_or_else(|problems| {
        panic!("the fixture target does not compile:\n{}", shown(&problems))
    });
    let other = compile(&registry, &fixture_root()).unwrap();
    assert_eq!(one.encoded.bytes, other.encoded.bytes);
    assert_eq!(one.encoded.hash, other.encoded.hash);

    // The one warning is the reviewer's: a tool that takes free text.
    let warnings: Vec<String> = one
        .warnings
        .iter()
        .filter(|warning| warning.code.as_str().starts_with("mcp."))
        .map(|warning| warning.code.to_string())
        .collect();
    assert_eq!(warnings, ["mcp.free_text"]);

    // A write commits and has the mapped read tool as its read-back; a read
    // by key is a lookup; a create carries its lookup.
    let plan = compiled_fixture(&registry);
    let update = steps(&plan, "example_order_note_update");
    assert!(update.act.commits && !update.act.lookup);
    let read_back = update.read_back.unwrap();
    assert!(!read_back.commits && read_back.lookup);
    assert!(read_back.args.is_empty());
    let get = steps(&plan, "example_order_get");
    assert!(!get.act.commits && get.act.lookup);
    let create = steps(&plan, "example_order_create");
    assert!(create.act.commits && create.lookup.unwrap().lookup);
    assert_eq!(plan.surfaces[1].programs.len(), 1);
}

#[tokio::test]
async fn the_fixture_pins_are_the_stands_definitions() {
    let stand = HttpStand::start().await.unwrap();
    let pins = common::listed_pins(&stand.url()).await;
    let mapping = std::fs::read_to_string(fixture_root().join("mappings").join("orders.yaml")).unwrap();
    let wrong: Vec<String> = pins
        .iter()
        .filter(|(tool, _)| tool != "orders_delete")
        .filter(|(tool, digest)| !mapping.contains(&format!("{tool}:\n    digest: \"{digest}\"")))
        .map(|(tool, digest)| format!("{tool} {digest}"))
        .collect();
    assert!(
        wrong.is_empty(),
        "the fixture pins other digests than the stand's definitions have: {wrong:#?}"
    );
}

/// Compiles the fixture with one file changed, and gives the problems.
fn problems_after(file: &str, from: &str, to: &str) -> a2m_files::Diagnostics {
    let folder = Folder::new("lint");
    let registry = registry(&folder);
    let root = folder.path().join("target");
    write_target(&root, &|path, text| {
        if path == file {
            assert!(text.contains(from), "`{file}` does not hold `{from}`");
            text.replace(from, to)
        } else {
            text
        }
    });
    compile(&registry, &root).expect_err("the changed target must not compile")
}

#[test]
fn what_the_mapping_does_not_allow_is_refused_with_a_file_a_line_and_a_fix() {
    // A binding to a tool the mapping does not name.
    let problems = problems_after(
        "operations/order_get.yaml",
        "tool: orders_get",
        "tool: orders_delete",
    );
    let unmapped = problems.find("mcp.tool_unmapped").expect("tool_unmapped");
    assert_eq!(unmapped.at.file.as_str(), "operations/order_get.yaml");
    assert!(unmapped.at.line > 1 && !unmapped.fix.is_empty());

    // A mapped tool with no digest.
    let problems = problems_after(
        "mappings/orders_local.yaml",
        "    digest: \"sha256:945f610cfadfb8a9656a1702f1ce4c5a8dbb403f6b6d2246a3ac46836312ec44\"\n",
        "",
    );
    let unpinned = problems.find("mcp.tool_unpinned").expect("tool_unpinned");
    assert_eq!(unpinned.at.file.as_str(), "mappings/orders_local.yaml");

    // A local server program with no digest.
    let problems = problems_after("target.yaml", "    digest: \"sha256:0000000000000000000000000000000000000000000000000000000000000000\"\n", "");
    let unpinned = problems.find("mcp.program_unpinned").expect("program_unpinned");
    assert_eq!(unpinned.at.file.as_str(), "target.yaml");

    // A write with no mapped read tool.
    let problems = problems_after(
        "mappings/orders.yaml",
        "    effect: write\n    read_back: orders_get\n    result: text",
        "    effect: write\n    result: text",
    );
    let no_readback = problems
        .find("mcp.write_no_readback")
        .expect("write_no_readback");
    assert_eq!(no_readback.at.file.as_str(), "mappings/orders.yaml");

    // A binding to a tool of a mapping that is for discovery only.
    let problems = problems_after(
        "mappings/orders.yaml",
        "purpose: tools",
        "purpose: discovery",
    );
    assert!(problems.find("mcp.discovery_bound").is_some());

    // An unknown key in a mapping, and an unknown key in a binding.
    let problems = problems_after(
        "mappings/orders.yaml",
        "    records: orders",
        "    records: orders\n    retries: 3",
    );
    let in_file = |problems: &a2m_files::Diagnostics, file: &str| {
        problems
            .iter()
            .any(|problem| problem.at.file.as_str() == file && problem.at.line > 1 && !problem.fix.is_empty())
    };
    assert!(in_file(&problems, "mappings/orders.yaml"), "{}", shown(&problems));
    let problems = problems_after(
        "operations/order_get.yaml",
        "    tool: orders_get",
        "    tool: orders_get\n    retry: true",
    );
    assert!(in_file(&problems, "operations/order_get.yaml"), "{}", shown(&problems));

    // An operation that states less than the tool it calls.
    let problems = problems_after(
        "mappings/orders.yaml",
        "    effect: read\n    absent_error",
        "    effect: irreversible\n    absent_error",
    );
    assert!(problems.find("mcp.effect_understated").is_some());

    // Text the system's users wrote, under a field type that does not mark
    // it untrusted.
    let problems = problems_after(
        "operations/order_get.yaml",
        "  note: {type: free_text}",
        "  note: {type: identifier}",
    );
    assert!(problems.find("mcp.untrusted_unmarked").is_some());
}
