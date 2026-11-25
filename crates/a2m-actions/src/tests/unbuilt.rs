//! Every action in the catalog with no handler, each named with its reason
//! (package 5.1). An action left unbuilt is a decision, not an oversight: the
//! test fails when an action gains or loses a handler and this list does not
//! change with it, and `tools/docs_audit.py` holds the action catalog's
//! "not built" marks to the same list.

use a2m_actions::ActionRegistry;

/// The actions with no handler, in catalog order, each with where its reason
/// is written. Decision 0456, point 6, is the list; decision 0397 gave the
/// first 23 of it; decision 0997 sorted it by what each waits on, after
/// decision 0465 had the ones that serve one machine built.
const UNBUILT: &[(&str, &str)] = &[
    (
        "run.undo",
        "decision 0456, point 6, and decision 0997: a reverse write that a person approves, a design of its own (Phase 9)",
    ),
    (
        "screen.snapshot",
        "decision 0456, point 6, and decision 0997: a snapshot operation each pack would supply, which the provider contract does not have",
    ),
    (
        "endpoints.infer",
        "decision 0456, point 6, and decision 0997: running the inference again over stored findings, after the release (decision 0456, point 2)",
    ),
    (
        "provider.enable",
        "decision 0456, point 6, and decision 0997: it changes `providers.enabled` through the record `config.set` keeps, and the registry is built again while the engine runs",
    ),
    (
        "provider.disable",
        "decision 0456, point 6, and decision 0997: it changes `providers.enabled` through the record `config.set` keeps, and the registry is built again while the engine runs",
    ),
    (
        "unknown.settle",
        "decision 0456, point 6, and decision 0997: what evidence settles a question is not designed; a held draft's question is settled by writing the draft",
    ),
    (
        "version.activate",
        "decision 0456, point 6, and decision 0997: the candidate stage, with shadowed reads and a toolset pinned to an exact version",
    ),
    (
        "version.bundle",
        "decision 0456, point 6, and decision 0997: the bundle's format and the key that signs it are not designed",
    ),
    (
        "version.import",
        "decision 0456, point 6, and decision 0997: the bundle's format and the key that signs it are not designed",
    ),
    (
        "version.migrate",
        "decision 0456, point 6, and decision 0997: the store keeps compiled targets, not the sources a newer engine would compile again",
    ),
    (
        "runner.enroll",
        "decision 0456, point 6, and decision 0997: a shared server's only; a shared server comes after the first release",
    ),
    (
        "runner.list",
        "decision 0456, point 6, and decision 0997: a shared server's only; a shared server comes after the first release",
    ),
    (
        "runner.drain",
        "decision 0456, point 6, and decision 0997: a shared server's only; a shared server comes after the first release",
    ),
    (
        "runner.revoke",
        "decision 0456, point 6, and decision 0997: a shared server's only; a shared server comes after the first release",
    ),
    (
        "vault.rotate",
        "decision 0456, point 6, and decision 0997: the vault's step 7 leaves open what rotating an owner's key means",
    ),
    (
        "delegation.grant",
        "decision 0456, point 6, and decision 0997: a shared server's only: on one machine an agent acts for the person by reaching the engine as that user",
    ),
    (
        "delegation.revoke",
        "decision 0456, point 6, and decision 0997: a shared server's only: on one machine an agent acts for the person by reaching the engine as that user",
    ),
    (
        "delegation.list",
        "decision 0456, point 6, and decision 0997: a shared server's only: on one machine an agent acts for the person by reaching the engine as that user",
    ),
    (
        "config.set",
        "decision 0456, point 6, and decision 0997: a writer that keeps a settings file's comments, and the record a loosened protected value is kept as",
    ),
    (
        "browser.install",
        "decision 0456, point 6, and decision 0997: where a pinned browser is fetched from, and its digests on each system",
    ),
    (
        "plugin.install",
        "decision 0456, point 6, and decision 0997: the plug-in host and its signature check (package F3)",
    ),
    (
        "plugin.list",
        "decision 0456, point 6, and decision 0997: the plug-in host (package F3), and where a search for installed plug-ins looks",
    ),
];

#[test]
fn every_action_without_a_handler_is_named_with_its_reason() {
    let registry = ActionRegistry::kernel_only();
    let unbuilt: Vec<&str> = registry
        .iter()
        .filter(|def| !def.built())
        .map(|def| def.name.as_str())
        .collect();
    let listed: Vec<&str> = UNBUILT.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        unbuilt, listed,
        "the actions with no handler and the list of package 5.1 differ: an action that gains \
         or loses a handler changes this list, the action catalog's marks, and decision 0456's \
         count in the same change"
    );
    for (name, reason) in UNBUILT {
        assert!(
            reason.contains("decision 0456"),
            "`{name}` has no reason in decision 0456"
        );
    }
}
