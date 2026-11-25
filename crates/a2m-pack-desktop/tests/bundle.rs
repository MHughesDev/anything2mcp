//! What the scout learns from the files of a macOS application's bundle a
//! person hands it (decision 0317): the property list in its XML form and
//! its binary form, and the scripting dictionary the property list names.
//! Every file is read as bytes, on every system, and nothing is sent to
//! any application: each test asks the stand what it was asked.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::time::Duration;

use a2m_core::{
    AccessKind, EvidenceLevel, Finding, NoSecrets, Redact, Source, SurfaceKind, Values,
};
use a2m_provider::{
    Access, CheckedPaths, Deadline, DiscoveryReport, DiscoveryScope, FindingSink, Posture,
    ReadOnlyPort, RootName, Roots, SurfaceDescription,
};
use common::{Folder, Scene};

const WITHIN: Duration = Duration::from_secs(20);

struct Kept(Vec<Finding>);

impl FindingSink for Kept {
    fn found(&mut self, finding: Finding) -> Result<(), a2m_core::Failure> {
        self.0.push(finding);
        Ok(())
    }
}

/// A redactor that knows one planted secret.
struct Planted(&'static str);

impl Redact for Planted {
    fn redact(&self, text: &str) -> String {
        text.replace(self.0, a2m_core::REDACTED)
    }
}

fn bundles() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("bundles")
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap().path();
        let name = entry.file_name().unwrap();
        if entry.is_dir() {
            copy(&entry, &to.join(name));
        } else {
            std::fs::copy(&entry, to.join(name)).unwrap();
        }
    }
}

/// The bundle of one fixture form, copied into a folder of the test's own,
/// with `change` applied to its dictionary.
fn bundle(form: &str, change: impl Fn(&str) -> String) -> Folder {
    let folder = Folder::new("bundle");
    copy(&bundles().join(form), folder.path());
    let sdef = folder
        .path()
        .join("Example.app")
        .join("Contents")
        .join("Resources")
        .join("Example.sdef");
    let text = std::fs::read_to_string(&sdef).unwrap();
    std::fs::write(&sdef, change(&text)).unwrap();
    folder
}

/// The pack's discoverer of bundles, run against the scene's session,
/// handed the folder `Example.app` of `inputs` as the bundle.
async fn discover(
    scene: &Scene,
    sources: &Path,
    redact: &dyn Redact,
) -> (Result<DiscoveryReport, a2m_core::Failure>, Vec<Finding>) {
    let description = SurfaceDescription {
        kind: SurfaceKind::new("desktop").unwrap(),
        access_kind: AccessKind::Interface,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = scene
        .registry
        .discoverers_for(&description)
        .into_iter()
        .find(|found| found.declared().name.as_str() == "desktop_bundle")
        .expect("the pack registers a discoverer of bundles");
    assert_eq!(discoverer.declared().files, ["bundle"]);
    let root = RootName::new("inputs").unwrap();
    let mut roots = Roots::none();
    roots.add(root.clone(), sources, Access::Read).unwrap();
    let mut paths = CheckedPaths::none();
    paths.insert(
        "bundle",
        roots.check(&root, "Example.app", Access::Read).unwrap(),
    );
    let (handle, prepared) = scene.prepare(Posture::Discovery).await.unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(WITHIN),
        paths: &paths,
        redact,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    let said = discoverer.discover(&port, &scope, &mut kept).await;
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;
    (said, kept.0)
}

fn pick<'a>(findings: &'a [Finding], from: &str) -> &'a Finding {
    findings
        .iter()
        .find(|finding| finding.found().contains(&format!(r#""from":"{from}""#)))
        .unwrap_or_else(|| panic!("no finding from {from}"))
}

#[tokio::test]
async fn a_property_list_in_its_xml_form_and_its_binary_form_gives_the_same_finding() {
    let Some(scene) = Scene::start("bundle-forms").await else {
        return;
    };
    let xml = bundle("xml", str::to_owned);
    let binary = bundle("binary", str::to_owned);
    // The two files are not the same bytes, and are of the two forms.
    let plist = |folder: &Folder| {
        std::fs::read(
            folder
                .path()
                .join("Example.app")
                .join("Contents")
                .join("Info.plist"),
        )
        .unwrap()
    };
    assert!(plist(&xml).starts_with(b"<?xml"));
    assert!(plist(&binary).starts_with(b"bplist00"));

    let (said, from_xml) =
        discover(&scene, xml.path(), &Planted("secret-for-tests-bundle-url")).await;
    said.unwrap();
    let (said, from_binary) = discover(
        &scene,
        binary.path(),
        &Planted("secret-for-tests-bundle-url"),
    )
    .await;
    said.unwrap();
    let (xml_info, binary_info) = (
        pick(&from_xml, "info_plist"),
        pick(&from_binary, "info_plist"),
    );
    // The same value from both forms: the findings differ in the file's
    // digest, which is of its bytes, and in nothing else.
    assert_eq!(xml_info.found(), binary_info.found());
    assert_ne!(xml_info.subject(), binary_info.subject());
    assert_eq!(xml_info.level(), EvidenceLevel::Inferred);
    assert_eq!(xml_info.source(), Source::Artifact);
    let found = xml_info.found();
    for stated in [
        r#""bundle_id":"example.desk""#,
        // Text outside ASCII, which the binary form keeps in two bytes a letter.
        r#""name":"Example Désk""#,
        r#""scripting_definition":"Example.sdef""#,
        r#""scriptable":true"#,
        r#""schemes":["example-order","example-desk"]"#,
        r#""extensions":["exord"]"#,
        r#""content_types":["example.order"]"#,
        r#""menu_item":"Open in Example Desk""#,
        r#""message":"openOrder""#,
        r#""send_types":["NSStringPboardType"]"#,
    ] {
        assert!(found.contains(stated), "{stated} not in {found}");
    }
    // What the redactor knows is in no finding.
    assert!(!found.contains("secret-for-tests-bundle-url"), "{found}");
    // Nothing was sent to the application.
    assert!(!scene.desk().truth().acted());
}

#[tokio::test]
async fn the_dictionary_the_property_list_names_is_read_with_each_propertys_stated_access() {
    let Some(scene) = Scene::start("bundle-sdef").await else {
        return;
    };
    let folder = bundle("xml", str::to_owned);
    let (said, findings) = discover(&scene, folder.path(), &NoSecrets).await;
    let report = said.unwrap();
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert_eq!(report.coverage, a2m_core::Coverage::Complete);
    let found = pick(&findings, "scripting_dictionary").found();
    for stated in [
        r#""file":"Contents/Resources/Example.sdef""#,
        r#""name":"Example Desk Suite""#,
        r#""name":"open order""#,
        r#""direct":"text""#,
        r#""name":"order changed""#,
        r#""access":"read","access_stated":true,"code":"numb","name":"number""#,
        r#""access":"readwrite","access_stated":true,"code":"cust","name":"customer""#,
        // No access stated: the format's default, which is not evidence.
        r#""access":"readwrite","access_stated":false,"code":"stat","name":"status""#,
        r#""values":["open","closed"]"#,
    ] {
        assert!(found.contains(stated), "{stated} not in {found}");
    }
}

#[tokio::test]
async fn a_dictionary_that_includes_a_file_outside_the_bundle_is_refused_by_name() {
    let Some(scene) = Scene::start("bundle-include").await else {
        return;
    };
    for href in [
        "file://localhost/System/Library/ScriptingDefinitions/CocoaStandard.sdef",
        "../../../../outside.sdef",
        "/etc/outside.sdef",
    ] {
        let folder = bundle("xml", |text| {
            text.replace(
                "<dictionary title=\"Example Desk Terminology\">",
                &format!(
                    "<dictionary xmlns:xi=\"http://www.w3.org/2003/XInclude\" title=\"Example Desk Terminology\"><xi:include href=\"{href}\"/>"
                ),
            )
        });
        let (said, findings) = discover(&scene, folder.path(), &NoSecrets).await;
        let failure = said.unwrap_err();
        assert_eq!(
            failure.code().as_str(),
            "dictionary_include_refused",
            "{href}"
        );
        assert!(
            failure
                .message()
                .contains("outside the application's bundle")
        );
        // Refused whole, before any of it was taken: not even the
        // property list is stored.
        assert!(findings.is_empty(), "{findings:?}");
    }
    // An include that stays inside the bundle is listed and not followed.
    let folder = bundle("xml", |text| {
        text.replace(
            "<dictionary title=\"Example Desk Terminology\">",
            "<dictionary xmlns:xi=\"http://www.w3.org/2003/XInclude\" title=\"Example Desk Terminology\"><xi:include href=\"Part.sdef\"/>",
        )
    });
    let (said, findings) = discover(&scene, folder.path(), &NoSecrets).await;
    let report = said.unwrap();
    assert_eq!(report.coverage, a2m_core::Coverage::Partial);
    assert!(
        pick(&findings, "scripting_dictionary")
            .found()
            .contains(r#""includes":["Contents/Resources/Part.sdef"]"#)
    );
}

#[tokio::test]
async fn a_bundle_that_cannot_be_read_is_a_named_stop_and_nothing_is_stored() {
    let Some(scene) = Scene::start("bundle-bad").await else {
        return;
    };
    // No property list.
    let folder = Folder::new("bundle-bad");
    std::fs::create_dir_all(folder.path().join("Example.app").join("Contents")).unwrap();
    let (said, findings) = discover(&scene, folder.path(), &NoSecrets).await;
    assert!(said.unwrap_err().message().contains("Info.plist"));
    assert!(findings.is_empty());
    // A property list that is neither form, and one that names a
    // dictionary by a path.
    for (bytes, why) in [
        (b"not a property list".to_vec(), "Info.plist"),
        (
            br#"<plist><dict><key>OSAScriptingDefinition</key><string>../../x.sdef</string></dict></plist>"#.to_vec(),
            "plain file name",
        ),
        (
            br#"<plist><dict><key>OSAScriptingDefinition</key><string>Missing.sdef</string></dict></plist>"#.to_vec(),
            "cannot be read",
        ),
    ] {
        let folder = Folder::new("bundle-bad");
        let contents = folder.path().join("Example.app").join("Contents");
        std::fs::create_dir_all(&contents).unwrap();
        std::fs::write(contents.join("Info.plist"), bytes).unwrap();
        let (said, findings) = discover(&scene, folder.path(), &NoSecrets).await;
        let failure = said.unwrap_err();
        assert!(failure.message().contains(why), "{}", failure.message());
        assert!(findings.is_empty());
    }
}
