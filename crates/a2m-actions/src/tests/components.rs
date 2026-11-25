//! `component.list` (spec sections 203 and 220): each outside program an
//! installed provider needs, looked for at its manifest's fixed places in
//! order, with the path and the digest of what was found, and whether that
//! digest is one the manifest knows. Nothing is started.

use std::sync::Arc;

use a2m_actions::CloseReason;
use a2m_core::Digest;
use serde_json::{Value as Json, json};

use super::kernel::{Options, world};

/// A pack that needs four outside programs, at places the test made.
struct NeedingPack(&'static str);

impl a2m_provider::Pack for NeedingPack {
    fn manifest(&self) -> &'static str {
        self.0
    }

    fn register(
        &self,
        _cx: &a2m_provider::PackContext,
        _to: &mut a2m_provider::Registrar<'_>,
    ) -> Result<(), a2m_core::Failure> {
        Ok(())
    }
}

fn toml_path(path: &std::path::Path) -> String {
    format!("'{}'", path.display())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_outside_program_a_provider_needs_is_found_or_missing_with_its_path_and_digest() {
    let kept = super::kernel::engine::Folder::new("components");
    let folder = kept.path().to_path_buf();
    let known = folder.join("known-program");
    let other = folder.join("other-program");
    let absent = folder.join("absent-program");
    std::fs::write(&known, b"a program whose digest is known").unwrap();
    std::fs::write(&other, b"a program nobody listed").unwrap();
    let digest = Digest::of(b"a program whose digest is known");
    let manifest = format!(
        r#"manifest_format = 1
contract = 1

[provider]
name = "example_needs"
version = "0.1.0"
publisher = "Example"
kind = "pack"

[[requirements]]
name = "known_one"
detect = {{ paths = [{absent}, {known}] }}
known_digests = ["sha256:{digest}"]
versions = "1.x"
missing = "Install the known one."

[[requirements]]
name = "unverified_one"
detect = {{ paths = [{other}] }}
known_digests = ["sha256:{digest}"]
missing = "Install the unverified one."

[[requirements]]
name = "unlisted_one"
detect = {{ paths = [{other}] }}
missing = "Install the unlisted one."

[[requirements]]
name = "missing_one"
detect = {{ paths = [{absent}, 'C:\Program Files\Example\absent.exe', '/opt/example/absent'] }}
missing = "Install the missing one."
"#,
        absent = toml_path(&absent),
        known = toml_path(&known),
        other = toml_path(&other),
    );
    let pack = NeedingPack(Box::leak(manifest.into_boxed_str()));
    let mut options = Options::default();
    options.extra_pack = Some(Arc::new(pack));
    let world = world(options).await;

    let listed = world.ok("component.list", json!({})).await;
    let ours: Vec<&Json> = listed["components"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|component| component["provider"] == "example_needs")
        .collect();
    let names: Vec<&str> = ours.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["known_one", "unverified_one", "unlisted_one", "missing_one"]
    );
    // The first place that holds a file, its digest, and the manifest's
    // own word on versions.
    assert_eq!(ours[0]["state"], "known");
    assert_eq!(ours[0]["path"], known.display().to_string());
    assert_eq!(ours[0]["digest"], format!("sha256:{digest}"));
    assert_eq!(ours[0]["versions"], "1.x");
    assert!(ours[0].get("fix").is_none());
    // Found, with a digest the manifest does not list, or a manifest that
    // lists none: unverified, with the fix.
    for unverified in &ours[1..3] {
        assert_eq!(unverified["state"], "unverified", "{unverified}");
        assert_eq!(unverified["path"], other.display().to_string());
        assert_eq!(
            unverified["digest"],
            format!("sha256:{}", Digest::of(b"a program nobody listed"))
        );
        assert!(unverified["fix"].as_str().unwrap().contains("Review"));
    }
    // Found nowhere, a place written for another system passed over:
    // missing, with what the manifest says to do.
    assert_eq!(ours[3]["state"], "missing");
    assert!(ours[3].get("path").is_none());
    assert_eq!(ours[3]["fix"], "Install the missing one.");

    // The provider is reported as lacking the program by `provider.list`
    // as well: one check, two views.
    let providers = world.ok("provider.list", json!({})).await;
    let needs = providers["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|provider| provider["name"] == "example_needs")
        .unwrap()
        .clone();
    assert_eq!(needs["availability"], "component_unverified", "{needs}");
    world.kernel.close(CloseReason::Asked).await;
}
