//! Toolsets, grants, and policy sets as actions (package 6.3, decisions
//! 0328 and 0399): each change is a person's act published as a new
//! version; a toolset only narrows; the built-in toolsets keep working once
//! a set defines toolsets; a set that asks for a second administrator gets
//! one who proves presence of their own; and an explanation is for an
//! auditor or an administrator.

use a2m_actions::{Caller, Request, Response};
use a2m_policy::{BuiltIn, Role, RoleBinding, Subject, ToolsetName};
use serde_json::{Value as Json, json};

use super::kernel::World;
use super::kernel::engine::{ITEM_UPSERT, ORDER_GET};

fn code_of(response: &Response) -> String {
    response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"))
        .code
        .clone()
}

impl World {
    fn user_id(&self) -> String {
        self.caller.principal.principal().id.to_string()
    }

    fn stranger_id(&self) -> String {
        self.stranger().principal.principal().id.to_string()
    }

    /// A caller through a toolset, as an agent on the person's machine.
    fn through(&self, toolset: &str) -> Caller {
        let mut caller = self.caller.clone();
        caller.toolset = Some(ToolsetName::new(toolset).unwrap());
        caller
    }

    async fn as_person(&self, action: &str, input: Json) -> Response {
        self.call_as(&self.person(), action, input).await
    }

    async fn readers(&self) -> Response {
        self.as_person(
            "toolset.create",
            json!({
                "name": "readers",
                "who": ["anyone"],
                "tools": [ORDER_GET],
                "max_effect": "read",
            }),
        )
        .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_toolset_is_created_listed_updated_and_removed_as_a_persons_act() {
    let world = World::ready().await;
    // An agent cannot publish a change to the set.
    let by_agent = world
        .call(
            "toolset.create",
            json!({"name": "readers", "who": ["anyone"], "tools": [ORDER_GET], "max_effect": "read"}),
        )
        .await;
    assert!(!by_agent.ok);
    assert_eq!(code_of(&by_agent), "person_required");
    assert!(
        world.ok("toolset.list", json!({})).await["toolsets"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let made = world.readers().await;
    assert!(made.ok, "{made:?}");
    assert_eq!(made.result.as_ref().unwrap()["changed"], "toolset readers");
    let twice = world.readers().await;
    assert_eq!(code_of(&twice), "schema_invalid");

    let listed = world.ok("toolset.list", json!({})).await;
    let readers = &listed["toolsets"][0];
    assert_eq!(readers["name"], "readers");
    assert_eq!(readers["max_effect"], "read");
    assert_eq!(readers["identity_mode"], "per_user_session");
    assert_eq!(readers["selects"], json!([ORDER_GET]));

    let updated = world
        .as_person(
            "toolset.update",
            json!({"name": "readers", "tools": ["example_*"], "max_effect": "write"}),
        )
        .await;
    assert!(updated.ok, "{updated:?}");
    let listed = world.ok("toolset.list", json!({"name": "readers"})).await;
    assert_eq!(listed["toolsets"][0]["max_effect"], "write");
    assert!(
        listed["toolsets"][0]["selects"]
            .as_array()
            .unwrap()
            .contains(&json!(ITEM_UPSERT)),
        "{listed}"
    );
    let missing = world
        .as_person(
            "toolset.update",
            json!({"name": "ghost", "max_effect": "read"}),
        )
        .await;
    assert_eq!(code_of(&missing), "not_found");

    let removed = world
        .as_person("toolset.remove", json!({"name": "readers"}))
        .await;
    assert!(removed.ok, "{removed:?}");
    assert!(
        world.ok("toolset.list", json!({})).await["toolsets"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let again = world
        .as_person("toolset.remove", json!({"name": "readers"}))
        .await;
    assert!(!again.ok);

    // Every change is a version, and the newest is in force.
    let versions = world.ok("policy.versions", json!({})).await;
    let versions = versions["versions"].as_array().unwrap();
    assert!(versions.len() >= 4, "{versions:?}");
    assert_eq!(versions.last().unwrap()["in_force"], true);
    assert_eq!(versions.iter().filter(|v| v["in_force"] == true).count(), 1);
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_toolset_only_narrows_and_the_built_in_toolsets_keep_working() {
    let world = World::ready().await;
    let before = world
        .call_as(
            &world.through("use"),
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(before.ok, "{before:?}");

    assert!(world.readers().await.ok);
    let user = world.user_id();

    // Asked about as a catalog: through the toolset only its tool, without
    // it every published tool the user holds.
    let through = world
        .ok(
            "policy.tools_allowed",
            json!({"principal": user, "toolset": "readers"}),
        )
        .await;
    assert_eq!(through["tools"], json!([ORDER_GET]));
    let plain = world
        .ok("policy.tools_allowed", json!({"principal": user}))
        .await;
    assert!(plain["tools"].as_array().unwrap().len() > 1, "{plain}");
    assert!(
        plain["tools"]
            .as_array()
            .unwrap()
            .contains(&json!(ITEM_UPSERT))
    );

    // The set now defines toolsets, and `use`, `build`, and `admin`, which
    // it does not define, still work: they name no toolset in a decision.
    for name in ["use", "build", "admin"] {
        let call = world
            .call_as(
                &world.through(name),
                "tool.call",
                json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
            )
            .await;
        assert!(call.ok, "{name}: {call:?}");
    }
    // A toolset the set defines narrows a call through it, and one it does
    // not define stops it.
    let readers_ok = world
        .call_as(
            &world.through("readers"),
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(readers_ok.ok, "{readers_ok:?}");
    let writes = world
        .call_as(
            &world.through("readers"),
            "tool.call",
            json!({"tool": ITEM_UPSERT, "input": super::runs::item("L-900")}),
        )
        .await;
    assert!(!writes.ok, "{writes:?}");
    assert_eq!(code_of(&writes), "not_granted");
    let ghost = world
        .call_as(
            &world.through("ghost"),
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(!ghost.ok, "{ghost:?}");
    assert_eq!(code_of(&ghost), "not_granted");

    // A set that defines a toolset of a built-in name narrows it.
    let narrowed = world
        .as_person(
            "toolset.create",
            json!({"name": "use", "who": ["anyone"], "tools": ["nothing_*"], "max_effect": "read"}),
        )
        .await;
    assert!(narrowed.ok, "{narrowed:?}");
    let refused = world
        .call_as(
            &world.through("use"),
            "tool.call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-100"}}),
        )
        .await;
    assert!(!refused.ok, "{refused:?}");
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grants_are_added_listed_and_removed_and_who_can_asks_over_the_catalog() {
    let world = World::ready().await;
    let stranger = world.stranger_id();
    let user = world.user_id();
    let who = |candidates: &[&str]| json!({"tool": ORDER_GET, "candidates": candidates});
    let before = world.ok("policy.who_can", who(&[&user, &stranger])).await;
    assert_eq!(before["who"], json!([user]));

    let added = world
        .as_person(
            "grant.add",
            json!({"who": stranger, "scope": "example_order_*", "max_effect": "read"}),
        )
        .await;
    assert!(added.ok, "{added:?}");
    let listed = world.ok("grant.list", json!({})).await;
    assert!(
        listed["grants"]
            .as_array()
            .unwrap()
            .iter()
            .any(|grant| grant["who"] == json!(stranger) && grant["scope"] == "example_order_*"),
        "{listed}"
    );
    let after = world.ok("policy.who_can", who(&[&user, &stranger])).await;
    assert_eq!(after["who"].as_array().unwrap().len(), 2, "{after}");

    let removed = world
        .as_person(
            "grant.remove",
            json!({"who": stranger, "scope": "example_order_*"}),
        )
        .await;
    assert!(removed.ok, "{removed:?}");
    let gone = world.ok("policy.who_can", who(&[&user, &stranger])).await;
    assert_eq!(gone["who"], json!([user]));
    let missing = world
        .as_person(
            "grant.remove",
            json!({"who": stranger, "scope": "example_order_*"}),
        )
        .await;
    assert_eq!(code_of(&missing), "not_found");
    let bad = world
        .as_person(
            "grant.add",
            json!({"who": "somebody", "scope": "x", "max_effect": "read"}),
        )
        .await;
    assert_eq!(code_of(&bad), "schema_invalid");
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_policy_file_is_tested_and_published_only_when_its_tests_pass() {
    let world = World::ready().await;
    let project = world.project("policy");
    let user = world.user_id();
    let layers = BuiltIn::local(
        world.caller.principal.principal().id,
        world.caller.principal.workspace(),
    )
    .layers()
    .clone();
    let row = |expect: &str| {
        json!({
            "name": "the user reads an order",
            "principal": user,
            "target": "example",
            "tool": ORDER_GET,
            "effect": "read",
            "expect": expect,
        })
    };
    let write = |name: &str, expect: &str| {
        let mut value = serde_json::to_value(&layers).unwrap();
        value["tests"] = json!([row(expect)]);
        std::fs::write(project.join(name), value.to_string()).unwrap();
    };
    write("good.json", "allow");
    write("bad.json", "deny");
    let run = |action: &'static str, file: &str| {
        let request =
            Request::new(action, json!({"file": file}), world.person()).in_project(project.clone());
        world.kernel.dispatch(request)
    };
    let tested = run("policy.test", "good.json").await;
    assert!(tested.ok, "{tested:?}");
    assert_eq!(tested.result.as_ref().unwrap()["ok"], true);
    assert_eq!(tested.result.as_ref().unwrap()["passed"], 1);
    let failing = run("policy.test", "bad.json").await;
    let failed = &failing.result.as_ref().unwrap()["failed"];
    assert_eq!(failed[0]["expected"], "deny", "{failing:?}");
    assert_eq!(failed[0]["got"], "allow");

    let refused = run("policy.publish", "bad.json").await;
    assert!(!refused.ok);
    assert_eq!(code_of(&refused), "policy_tests_failed");
    let versions_before = world.ok("policy.versions", json!({})).await["versions"]
        .as_array()
        .unwrap()
        .len();
    let published = run("policy.publish", "good.json").await;
    assert!(published.ok, "{published:?}");
    let versions = world.ok("policy.versions", json!({})).await;
    assert_eq!(
        versions["versions"].as_array().unwrap().len(),
        versions_before + 1
    );

    // A path that leaves the folder, or an absolute one, is not read.
    for file in ["../good.json", "/etc/hostname"] {
        let outside = run("policy.test", file).await;
        assert!(!outside.ok, "{file}");
        assert_eq!(code_of(&outside), "path_not_allowed", "{file}");
    }
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explanation_is_for_an_auditor_or_an_administrator() {
    let world = World::ready().await;
    let stranger = world.stranger();
    let id = world.stranger_id().parse().unwrap();
    // A viewer, an auditor, and an administrator, each only that.
    world
        .publish(|layers| {
            layers.roles.push(RoleBinding {
                who: Subject::Principal(id),
                role: Role::Viewer,
            });
        })
        .await;
    let decided = world.call("run.list", json!({})).await;
    let explain = json!({"decision": decided.decision.unwrap().to_string()});
    let viewer = world
        .call_as(&stranger, "policy.explain", explain.clone())
        .await;
    assert!(!viewer.ok, "{viewer:?}");
    assert_eq!(code_of(&viewer), "not_granted");
    for role in [Role::Auditor, Role::Admin] {
        world
            .publish(|layers| {
                layers.roles.push(RoleBinding {
                    who: Subject::Principal(id),
                    role: Role::Viewer,
                });
                layers.roles.push(RoleBinding {
                    who: Subject::Principal(id),
                    role,
                });
            })
            .await;
        let explained = world
            .call_as(&stranger, "policy.explain", explain.clone())
            .await;
        assert!(explained.ok, "{role:?}: {explained:?}");
    }
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_set_that_asks_for_a_second_administrator_gets_one_who_proves_presence() {
    let world = World::ready().await;
    let second = world.stranger_id();
    let id = second.parse().unwrap();
    world
        .publish(|layers| {
            layers.second_admin_publish = true;
            layers.roles.push(RoleBinding {
                who: Subject::Principal(id),
                role: Role::Admin,
            });
        })
        .await;
    let create = |extra: Json| {
        let mut input = json!({"name": "readers", "who": ["anyone"], "tools": [ORDER_GET], "max_effect": "read"});
        for (key, value) in extra.as_object().unwrap() {
            input[key] = value.clone();
        }
        world.as_person("toolset.create", input)
    };
    // Alone, the publisher is refused.
    let alone = create(json!({})).await;
    assert!(!alone.ok, "{alone:?}");
    assert_eq!(code_of(&alone), "second_admin_required");
    // The reviewer who drafted it is refused too.
    let author = create(json!({"reviewer": second, "drafted_by": second})).await;
    assert_eq!(code_of(&author), "second_admin_required");
    // A reviewer who proves presence of their own is taken: the surface
    // was asked for the reviewer's proof, and it was the reviewer's.
    let asked_before = world
        .surface
        .presence_asked
        .load(std::sync::atomic::Ordering::SeqCst);
    let reviewed = create(json!({"reviewer": second})).await;
    assert!(reviewed.ok, "{reviewed:?}");
    let asked_after = world
        .surface
        .presence_asked
        .load(std::sync::atomic::Ordering::SeqCst);
    assert!(
        asked_after >= asked_before + 2,
        "the publisher's and the reviewer's prompts"
    );
    // A reviewer who does not answer is not taken.
    *world.surface.proof_of.lock().unwrap() = Some(world.caller.principal.principal().id);
    let wrong = world
        .as_person(
            "toolset.update",
            json!({"name": "readers", "max_effect": "write", "reviewer": second}),
        )
        .await;
    assert!(!wrong.ok, "{wrong:?}");
    assert_eq!(code_of(&wrong), "presence_required");
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}
