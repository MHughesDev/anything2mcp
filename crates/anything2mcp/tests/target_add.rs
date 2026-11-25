//! Field finding 17: `target add` gives a surface the fields its kind
//! declares beyond the address, such as a database's store, so that a
//! person need not write the target file by hand before a scout. The
//! compiler checks them against the kind before anything is written.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use common::Machine;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_surface_is_added_with_the_fields_its_kind_declares_and_checked_before_it_is_written() {
    let machine = Machine::new("storefile", &[]);
    let world = machine.open().await;
    let store = machine.project.path().join("shop.db").display().to_string();
    let folder = machine.project.path().join("targets").join("shop");

    // Without the store the kind requires, the compiler refuses it and
    // nothing is written.
    let bare = world
        .agent(&[
            "target",
            "add",
            "shop",
            "--address",
            &store,
            "--kind",
            "database",
        ])
        .await;
    assert_eq!(
        bare.error()["code"],
        "schema_invalid",
        "{:?}",
        bare.finished
    );
    assert!(
        bare.error()["message"].as_str().unwrap().contains("store"),
        "{:?}",
        bare.finished
    );
    // Field test 7: the fix names the fields, which is where a missing one
    // is given, and not only the address and the kind.
    assert!(
        bare.error()["fix"].as_str().unwrap().contains("--fields"),
        "{:?}",
        bare.finished
    );
    assert!(!folder.exists());

    // A field the kind does not declare is refused the same way, and so is
    // a field of the kernel's own.
    for fields in [
        r#"{"store": "sqlite", "colour": "blue"}"#,
        r#"{"store": "sqlite", "kind": "database"}"#,
        r#"{"store": ["sqlite"]}"#,
    ] {
        let refused = world
            .agent(&[
                "target",
                "add",
                "shop",
                "--address",
                &store,
                "--kind",
                "database",
                "--fields",
                fields,
            ])
            .await;
        assert_eq!(
            refused.error()["code"],
            "schema_invalid",
            "{fields}: {:?}",
            refused.finished
        );
        assert!(!folder.exists(), "{fields}");
    }

    // With the store the file is written, the field as data, and it lints.
    let added = world
        .ok(&[
            "target",
            "add",
            "shop",
            "--address",
            &store,
            "--kind",
            "database",
            "--fields",
            r#"{"store": "sqlite"}"#,
        ])
        .await;
    assert_eq!(
        added["files"],
        serde_json::json!(["target.yaml"]),
        "{added}"
    );
    let written = std::fs::read_to_string(folder.join("target.yaml")).unwrap();
    assert!(written.contains("    store: \"sqlite\"\n"), "{written}");
    let linted = world.agent(&["lint", "shop"]).await;
    assert_eq!(linted.finished.code, 0, "{:?}", linted.finished);

    // Fields with no surface are refused by name.
    let alone = world
        .agent(&[
            "target",
            "add",
            "other",
            "--fields",
            r#"{"store": "sqlite"}"#,
        ])
        .await;
    assert_eq!(
        alone.error()["code"],
        "schema_invalid",
        "{:?}",
        alone.finished
    );
    world.close().await;
}
