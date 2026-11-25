//! Every action in the catalog with no handler, each named with its reason
//! (package 5.1). An action left unbuilt is a decision, not an oversight: the
//! test fails when an action gains or loses a handler and this list does not
//! change with it, and `tools/docs_audit.py` holds the action catalog's
//! "not built" marks to the same list.

use a2m_actions::ActionRegistry;

/// The actions with no handler, in catalog order, each with where its reason
/// is written. Decision 0456, point 6, is the list; decision 0397 gave the
/// first 23 of it.
const UNBUILT: &[(&str, &str)] = &[
    (
        "run.undo",
        "decision 0456, point 6: it builds a reverse write that a person approves, Phase 9",
    ),
    (
        "run.trace",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; `trace.show` gives a run's evidence",
    ),
    (
        "screen.snapshot",
        "decision 0456, point 6: a look at one screen, for which the scout and `draft.try` stand on one machine",
    ),
    (
        "endpoints.infer",
        "decision 0456, points 2 and 6: running the inference again over stored findings, after the release; `capture.stop` scouts what a capture found",
    ),
    (
        "provider.enable",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release",
    ),
    (
        "provider.disable",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release",
    ),
    (
        "component.list",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release",
    ),
    (
        "unknown.settle",
        "decision 0456, point 6: a held draft's question is settled by writing the draft with `draft.write`",
    ),
    (
        "version.activate",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; it moves a toolset through the candidate stage, Phase 6",
    ),
    (
        "version.rollback",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; its shortcut is not offered (decision 0456, point 1)",
    ),
    (
        "version.bundle",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; its shortcut is not offered (decision 0456, point 1)",
    ),
    (
        "version.import",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; signed bundles come with `version.bundle`, Phase 6",
    ),
    (
        "version.migrate",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; its shortcut is not offered (decision 0456, point 1)",
    ),
    (
        "runner.enroll",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; runners are for a shared server, Phase 7",
    ),
    (
        "runner.list",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; runners are for a shared server, Phase 7",
    ),
    (
        "runner.drain",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; runners are for a shared server, Phase 7",
    ),
    (
        "runner.revoke",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; runners are for a shared server, Phase 7",
    ),
    (
        "vault.rotate",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; rotating a key layer is Phase 9",
    ),
    (
        "delegation.grant",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; delegation is Phase 7",
    ),
    (
        "delegation.revoke",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; delegation is Phase 7",
    ),
    (
        "delegation.list",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; delegation is Phase 7",
    ),
    (
        "config.set",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release",
    ),
    (
        "browser.install",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release",
    ),
    (
        "plugin.install",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; plug-ins are Phase 8",
    ),
    (
        "plugin.list",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; plug-ins are Phase 8",
    ),
    (
        "trace.export",
        "one of decision 0397's 23, kept by decision 0456, point 6: no package builds it before the first release; `trace.show` gives a run's evidence",
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
