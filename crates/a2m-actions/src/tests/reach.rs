//! The reach as actions (package N3, decisions 0327 and 0399): an agent
//! cannot name or withdraw one; a reach is named on the network the machine
//! is on and is not in force on another; and a listing shows the facts a
//! reach states by name, and never a digest.

use a2m_actions::Response;
use a2m_netstate::NetworkFingerprint;
use a2m_provider::{Reach, ReachEntry};
use serde_json::{Value as Json, json};

use super::kernel::World;

fn code_of(response: &Response) -> String {
    response
        .error
        .as_ref()
        .unwrap_or_else(|| panic!("no error in {response:?}"))
        .code
        .clone()
}

fn fingerprint(hardware: u8, gateway: &str, subnet: &str) -> NetworkFingerprint {
    NetworkFingerprint::of_facts(
        Some(&[0, 0x11, 0x22, 0x33, 0x44, hardware]),
        Some(gateway.parse().unwrap()),
        None,
        Some((subnet.parse().unwrap(), 24)),
        &["office.example"],
    )
}

impl World {
    /// Keeps reaches as the engine's own store would, to stand for ones
    /// named on a network the test is not on.
    async fn keep_reaches(&self, reaches: Vec<Reach>) {
        let text = serde_json::to_string(&reaches).unwrap();
        self.kernel
            .store()
            .unwrap()
            .write(a2m_ledger::Durability::Synced, move |tx| {
                crate::kernel::set_fact(tx, "net.reaches", &text)
            })
            .await
            .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_cannot_name_or_withdraw_a_reach() {
    let world = World::ready().await;
    assert!(world.caller.is_agent);
    // An engine with no page for a person's act refuses the agent outright.
    let named = world
        .call(
            "reach.name",
            json!({"label": "office", "send": ["app.example.com"]}),
        )
        .await;
    assert!(!named.ok, "{named:?}");
    assert_eq!(code_of(&named), "person_required");
    world
        .keep_reaches(vec![Reach {
            label: "office".to_owned(),
            network: fingerprint(1, "192.168.1.1", "192.168.1.20"),
            look: Vec::new(),
            send: vec![ReachEntry::direct("app.example.com")],
        }])
        .await;
    let withdrawn = world
        .call("reach.withdraw", json!({"label": "office"}))
        .await;
    assert!(!withdrawn.ok, "{withdrawn:?}");
    assert_eq!(code_of(&withdrawn), "person_required");
    // The reach is still kept: only a person withdraws one.
    let listed = world.ok("reach.list", json!({})).await;
    assert_eq!(listed["reaches"].as_array().unwrap().len(), 1, "{listed}");
    // Nor can the call carry a network: the input has no field for one.
    let sneaky = world
        .call(
            "reach.name",
            json!({"label": "x", "network": {"gateway_address": "a"}}),
        )
        .await;
    assert!(!sneaky.ok);
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reach_named_on_another_network_is_listed_not_in_force_and_shows_no_digest() {
    let world = World::ready().await;
    let other = fingerprint(9, "10.20.0.1", "10.20.0.77");
    world
        .keep_reaches(vec![Reach {
            label: "cafe".to_owned(),
            network: other.clone(),
            look: vec![ReachEntry::direct("printer.example")],
            send: vec![ReachEntry::through(
                "app.example.com",
                a2m_provider::GatewayName::new("cafe_gateway").unwrap(),
            )],
        }])
        .await;
    // The machine the test runs on is not on that network.
    let listed = world.ok("reach.list", json!({})).await;
    let reach = &listed["reaches"][0];
    assert_eq!(reach["label"], "cafe");
    assert_eq!(reach["in_force"], false, "{listed}");
    assert_eq!(
        reach["stated"],
        json!([
            "gateway_hardware",
            "gateway_address",
            "subnet",
            "dns_domain"
        ])
    );
    assert_eq!(reach["send"][0]["destination"], "app.example.com");
    assert_eq!(reach["send"][0]["gateway"], "cafe_gateway");
    // No digest of the stored fingerprint is in what a listing shows.
    let stored: Json = serde_json::to_value(&other).unwrap();
    let shown = listed.to_string();
    for digest in stored.as_object().unwrap().values() {
        for text in digest
            .as_array()
            .map(|many| many.iter().collect::<Vec<_>>())
            .unwrap_or_else(|| vec![digest])
        {
            assert!(
                !shown.contains(&text.to_string().replace('"', "")),
                "{shown}"
            );
        }
    }
    // The kernel's own view, for the derivation of a place, holds it as not
    // in force on this network too.
    let (reaches, current) = world.kernel.reaches_and_network().await.unwrap();
    assert_eq!(reaches.len(), 1);
    assert!(!reaches[0].is_in_force(&current));
    // A person withdraws it; an unknown label is not found.
    let person = world.person();
    let gone = world
        .call_as(&person, "reach.withdraw", json!({"label": "cafe"}))
        .await;
    assert!(gone.ok, "{gone:?}");
    let again = world
        .call_as(&person, "reach.withdraw", json!({"label": "cafe"}))
        .await;
    assert_eq!(code_of(&again), "not_found");
    assert!(
        world.ok("reach.list", json!({})).await["reaches"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_person_names_the_network_the_machine_is_on_when_it_can_be_recognized_again() {
    let world = World::ready().await;
    let person = world.person();
    let current = world.ok("net.current", json!({})).await;
    let usable = current["can_be_named"].as_bool().unwrap();
    // `net.current` shows the facts by name and the reasons for the rest.
    assert!(
        current["stated"].is_array() && current["left_out"].is_array(),
        "{current}"
    );
    let named = world
        .call_as(
            &person,
            "reach.name",
            json!({"label": "here", "look": ["printer.example"], "send": ["app.example.com via here_gateway"]}),
        )
        .await;
    if usable {
        assert!(named.ok, "{named:?}");
        let listed = world.ok("reach.list", json!({})).await;
        assert_eq!(listed["reaches"][0]["in_force"], true, "{listed}");
        assert_eq!(listed["reaches"][0]["send"][0]["gateway"], "here_gateway");
        // A second reach of the same label is refused.
        let twice = world
            .call_as(&person, "reach.name", json!({"label": "HERE"}))
            .await;
        assert!(!twice.ok);
        let (reaches, current) = world.kernel.reaches_and_network().await.unwrap();
        assert!(reaches[0].is_in_force(&current));
    } else {
        // A network that states too few facts is not named: nothing is kept.
        assert!(!named.ok, "{named:?}");
        assert_eq!(code_of(&named), "network_not_recognizable");
        assert!(
            world.ok("reach.list", json!({})).await["reaches"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    // Entries are the person's own words, in one of two forms.
    let bad = world
        .call_as(
            &person,
            "reach.name",
            json!({"label": "x", "send": ["a b c"]}),
        )
        .await;
    assert!(!bad.ok);
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}
