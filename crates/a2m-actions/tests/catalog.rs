//! The registry against the documents: every action of the action catalog
//! is in the registry with the same name, role, and listing, and nothing
//! else is; every face is generated for every action; and no action can
//! approve.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; the helpers the tests share do not carry the test attribute"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use a2m_actions::{
    ActionRegistry, BuiltInToolset, Handler, PROCESS_COMMANDS, SHORTCUTS, command_faces, mcp_tools,
    rest_routes, sdk_methods, shortcuts_offered,
};

fn document(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .replace('\r', "")
}

fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// The action rows of the catalog: name to (who, listed, phase).
fn catalog_rows() -> BTreeMap<String, (String, String, String)> {
    let text = document("docs/reference/action-catalog.md");
    let actions = text
        .split("## What is deliberately absent")
        .next()
        .unwrap()
        .to_owned();
    let mut rows = BTreeMap::new();
    for line in actions.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        assert_eq!(cells.len(), 5, "a catalog row has five columns: {line}");
        for name in backticked(cells[0]) {
            let before = rows.insert(
                name.clone(),
                (
                    cells[2].to_owned(),
                    cells[3].to_owned(),
                    cells[4].to_owned(),
                ),
            );
            assert!(before.is_none(), "{name} is in the catalog twice");
        }
    }
    rows
}

#[test]
fn the_registry_holds_exactly_the_actions_of_the_catalog() {
    let rows = catalog_rows();
    let registry = ActionRegistry::kernel_only();
    let in_registry: BTreeSet<String> = registry.iter().map(|def| def.name.to_string()).collect();
    let in_catalog: BTreeSet<String> = rows.keys().cloned().collect();
    assert_eq!(
        in_registry.difference(&in_catalog).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "in the registry and not in the catalog"
    );
    assert_eq!(
        in_catalog.difference(&in_registry).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "in the catalog and not in the registry"
    );
    assert!(rows.len() > 80, "the catalog was read: {} rows", rows.len());

    for def in registry.iter() {
        let (who, listed, phase) = &rows[def.name.as_str()];
        assert_eq!(def.who.as_str(), who, "the role of {}", def.name);
        assert_eq!(def.listed.as_str(), listed, "the listing of {}", def.name);
        // Package 5.1: the catalog marks an action with no handler, and only
        // such an action, "not built" (decision 0456, point 6).
        assert_eq!(
            phase.ends_with("not built"),
            !def.built(),
            "the catalog's mark of {}: {phase}",
            def.name
        );
    }
}

#[test]
fn every_action_has_all_four_faces_and_they_share_its_name() {
    let registry = ActionRegistry::kernel_only();
    let commands = command_faces(&registry);
    let routes = rest_routes(&registry);
    let methods = sdk_methods(&registry);
    assert_eq!(commands.len(), registry.len());
    // An action has its method from the build that gives it a handler
    // (decision 0437, point 5).
    assert_eq!(
        methods.len(),
        registry.iter().filter(|def| def.built()).count()
    );
    for def in registry.iter() {
        let name = def.name.as_str();
        let (noun, verb) = name.split_once('.').unwrap();
        let command = commands.iter().find(|face| face.action == name).unwrap();
        assert!(
            command
                .usage
                .starts_with(&format!("anything2mcp {noun} {verb}"))
        );
        assert!(command.args.iter().filter(|arg| arg.positional).count() <= 1);
        let tool = registry
            .by_tool_name(&format!("a2m_{noun}_{verb}"))
            .unwrap();
        assert_eq!(tool.name, def.name);
        assert!(routes.iter().any(|route| {
            route.method == "POST"
                && route.path == format!("/api/v1/actions/{name}")
                && !route.alias
        }));
        if def.built() {
            let method = methods.iter().find(|face| face.action == name).unwrap();
            assert_eq!(
                (method.namespace.as_str(), method.method.as_str()),
                (noun, verb)
            );
        }
        // A summary whose first 240 characters choose the action.
        assert!(
            def.summary.len() >= 15 && def.summary.ends_with('.'),
            "{name}"
        );
        // Flat, about ten properties or fewer; examples that parse and fit.
        assert!(def.input.fields.len() <= 10, "{name}");
        for example in &def.examples {
            let input: serde_json::Value = serde_json::from_str(example).unwrap();
            def.input
                .schema
                .check_json(&input)
                .unwrap_or_else(|misfits| panic!("an example of {name}: {misfits}"));
        }
    }
    // The alias routes of spec section 222.
    for (method, path, action) in [
        ("GET", "/api/v1/tools", "tool.search"),
        ("GET", "/api/v1/runs/{run_id}", "run.get"),
        ("POST", "/api/v1/runs/{run_id}/resume", "run.resume"),
        ("POST", "/api/v1/runs/{run_id}/cancel", "run.cancel"),
        ("GET", "/api/v1/sessions", "session.status"),
    ] {
        let route = routes
            .iter()
            .find(|route| route.alias && route.path == path)
            .unwrap_or_else(|| panic!("no route {path}"));
        assert_eq!((route.method, route.action.as_str()), (method, action));
    }
    let resume = routes
        .iter()
        .find(|route| route.path == "/api/v1/runs/{run_id}/resume")
        .unwrap();
    assert_eq!(resume.path_fields, ["run_id"]);
}

#[test]
fn an_action_has_a_client_method_only_when_it_has_a_handler() {
    // Decision 0437, point 5: an action with no handler is described with
    // `x-sdk: false` and has no method in either client; when it gains a
    // handler, it gains its method. The handler is the one place this is
    // known.
    let registry = ActionRegistry::kernel_only();
    let routes = rest_routes(&registry);
    let methods = sdk_methods(&registry);
    let mut unbuilt = 0;
    for def in registry.iter() {
        let name = def.name.as_str();
        let has = def.faces.sdk && def.built();
        unbuilt += usize::from(!def.built());
        for route in routes.iter().filter(|route| route.action == name) {
            assert_eq!(route.sdk, has, "{} {}", route.method, route.path);
        }
        assert_eq!(
            methods.iter().any(|face| face.action == name),
            has,
            "{name}"
        );
    }
    // Every route of an action that answers `action_not_built` says false.
    for route in routes.iter().filter(|route| route.sdk) {
        let def = registry.find(&route.action).unwrap();
        assert!(def.built(), "{} has `x-sdk` and no handler", route.action);
    }
    assert!(
        unbuilt > 0,
        "every action is built: this test proves nothing"
    );
}

#[test]
fn a_toolset_lists_no_more_than_its_cap_and_always_the_three_that_reach_the_rest() {
    let registry = ActionRegistry::kernel_only();
    for toolset in [BuiltInToolset::Use, BuiltInToolset::Build] {
        let tools = mcp_tools(&registry, toolset);
        assert!(tools.len() <= toolset.cap(), "{}", toolset.as_str());
        for reach in ["a2m_action_search", "a2m_action_describe", "a2m_action_run"] {
            assert!(tools.iter().any(|tool| tool.name == reach), "{reach}");
        }
        // What the cap leaves out is still marked, and reached by name.
        let listed = registry.toolset(toolset).len() + registry.past_cap(toolset).len();
        let marked = registry
            .iter()
            .filter(|def| def.listed.in_toolset(toolset))
            .count();
        assert_eq!(listed, marked);
    }
    let control: Vec<String> = mcp_tools(&registry, BuiltInToolset::Use)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    for tool in [
        "a2m_tool_search",
        "a2m_tool_call",
        "a2m_run_get",
        "a2m_run_resume",
        "a2m_run_cancel",
        "a2m_session_status",
        "a2m_session_login",
    ] {
        assert!(control.contains(&tool.to_owned()), "{tool}");
    }
}

#[test]
fn no_action_can_approve() {
    // Recording an approval is not an action. No name, field, or summary
    // offers one, and the only handle that records one is given to the
    // person surface: see `tests/kernel.rs`, where the approval is recorded
    // through the desk and through nothing a caller reaches.
    let registry = ActionRegistry::kernel_only();
    for def in registry.iter() {
        let name = def.name.as_str();
        for word in ["approve", "approval", "decline"] {
            assert!(!name.contains(word), "{name} is named for approving");
            assert!(
                def.input
                    .fields
                    .iter()
                    .all(|field| !field.name.contains(word)),
                "{name} takes a field named for approving"
            );
        }
    }
    // The catalog says so as well.
    let text = document("docs/reference/action-catalog.md");
    assert!(text.contains("No action approves a write or a publish on a local machine"));
}

#[test]
fn the_shortcuts_and_process_commands_are_those_of_the_catalog_and_add_no_action() {
    let registry = ActionRegistry::kernel_only();
    let text = document("docs/reference/action-catalog.md");
    let shortcuts = text
        .split("## Shortcuts")
        .nth(1)
        .unwrap()
        .split("## Process commands")
        .next()
        .unwrap();
    let mut words = BTreeSet::new();
    for line in shortcuts.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        for word in backticked(cells[0]) {
            words.insert(word.split_whitespace().next().unwrap().to_owned());
        }
    }
    // The table lists the shortcuts offered, those whose every step is
    // built (decision 0456), and no other.
    let ours: BTreeSet<String> = shortcuts_offered(&registry)
        .iter()
        .map(|shortcut| shortcut.word.to_owned())
        .collect();
    assert_eq!(ours, words);
    let nouns: BTreeSet<&str> = registry.iter().map(|def| def.name.noun()).collect();
    for shortcut in SHORTCUTS {
        for step in shortcut.steps {
            let def = registry
                .find(step.action)
                .unwrap_or_else(|| panic!("{} names {}", shortcut.word, step.action));
            if let Some(argument) = step.argument {
                assert!(def.input.field(argument).is_some(), "{}", step.action);
            }
        }
    }
    let process = text.split("## Process commands").nth(1).unwrap();
    let named: BTreeSet<String> = process
        .lines()
        .filter(|line| line.starts_with("| `"))
        .flat_map(|line| backticked(line.trim_matches('|').split('|').next().unwrap()))
        .collect();
    // The table lists the process commands and `completion`, which the
    // program answers itself without an engine; it is the product crate's,
    // not a process command, and this crate cannot name its constant.
    assert_eq!(
        named,
        PROCESS_COMMANDS
            .iter()
            .copied()
            .chain(["completion"])
            .map(str::to_owned)
            .collect()
    );
    // A word may be a noun as well (`runner`, `run`, `test`, `docs`): alone
    // it is the process command or the shortcut, and with a verb after it,
    // the action. Every noun therefore needs a verb.
    assert!(nouns.contains("runner") && nouns.contains("run"));
    for def in registry.iter() {
        assert!(!def.name.verb().is_empty());
    }
}

#[test]
fn an_action_is_served_by_the_kernel_the_client_or_not_yet_and_the_catalog_has_a_digest() {
    let registry = ActionRegistry::kernel_only();
    let built = registry.iter().filter(|def| def.built()).count();
    assert!(built >= 9, "{built} actions are built");
    for def in registry.iter() {
        if def.name.noun() == "engine" && def.name.verb() != "status" {
            assert!(matches!(def.handler, Handler::Client), "{}", def.name);
        }
    }
    let first = registry.catalog();
    let second = ActionRegistry::kernel_only().catalog();
    assert_eq!(first.digest, second.digest);
    assert_eq!(first.actions.len(), registry.len());
    assert_eq!(first.digest.len(), 64);
}

#[test]
fn no_action_without_a_handler_is_listed_up_front_and_the_trial_is() {
    let registry = ActionRegistry::kernel_only();
    for toolset in [BuiltInToolset::Use, BuiltInToolset::Build] {
        for def in registry.toolset(toolset) {
            assert!(
                def.built(),
                "{} is listed in the {} toolset and has no handler",
                def.name,
                toolset.as_str()
            );
        }
    }
    let build: Vec<String> = registry
        .toolset(BuiltInToolset::Build)
        .iter()
        .map(|def| def.name.to_string())
        .collect();
    assert!(build.iter().any(|name| name == "draft.try"), "{build:?}");
    assert!(
        !build.iter().any(|name| name == "endpoints.infer"),
        "{build:?}"
    );
    for built in ["capture.start", "capture.stop"] {
        assert!(build.iter().any(|name| name == built), "{built}");
    }
}

#[test]
fn a_shortcut_is_offered_once_its_last_step_is_built_in_the_catalog() {
    // Asked of the action catalog: what is offered follows what each
    // entry says is built, and nothing else.
    let registry = ActionRegistry::kernel_only();
    let catalog = registry.catalog();
    let built_in = |catalog: &a2m_actions::Catalog, action: &str| {
        catalog
            .actions
            .iter()
            .any(|entry| entry.name == action && entry.built)
    };
    for shortcut in SHORTCUTS {
        assert_eq!(
            shortcut.offered_where(|action| built_in(&catalog, action)),
            shortcut.offered(&registry),
            "{}",
            shortcut.word
        );
    }
    let withheld: Vec<_> = SHORTCUTS
        .iter()
        .filter(|shortcut| !shortcut.offered(&registry))
        .collect();
    assert!(!withheld.is_empty(), "no shortcut waits for a step today");
    for shortcut in withheld {
        // The catalog as it will be once the steps it waits for are built:
        // the shortcut is offered, with nothing else changed.
        let mut later = catalog.clone();
        for entry in &mut later.actions {
            if shortcut.steps.iter().any(|step| step.action == entry.name) {
                entry.built = true;
            }
        }
        assert!(
            shortcut.offered_where(|action| built_in(&later, action)),
            "{}",
            shortcut.word
        );
        // And not while one of them is still unbuilt.
        let last = shortcut.steps.last().unwrap().action;
        for entry in &mut later.actions {
            if entry.name == last {
                entry.built = false;
            }
        }
        assert!(
            !shortcut.offered_where(|action| built_in(&later, action)),
            "{}",
            shortcut.word
        );
    }
}
