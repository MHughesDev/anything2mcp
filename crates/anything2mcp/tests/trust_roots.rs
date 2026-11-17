//! The trust roots an installation names (decision 0166): read when the
//! engine opens, through the folders `files.allowed_roots` allows, and
//! trusted by every HTTP client of the product. Through the real program,
//! so that each command is a process of its own with its own trust.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::process::Output;

use a2m_actions::Paths;
use common::Machine;
use serde_json::Value as Json;

const PLACEHOLDER: &str = "http://127.0.0.1:4010";

fn object(output: &Output) -> Json {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "not one JSON object: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Writes the installation's settings file: the vault's passphrase route,
/// and the trust roots named.
fn name_roots(machine: &Machine, roots: &[&std::path::Path]) {
    let listed: Vec<String> = roots
        .iter()
        .map(|path| format!("{:?}", path.display().to_string()))
        .collect();
    let installation = Paths::under(machine.home.path()).installation_config;
    std::fs::write(
        installation.join("settings.toml"),
        format!(
            "[vault]\nbackend = \"passphrase\"\n\n[network]\ntrust_roots = [{}]\n",
            listed.join(", ")
        ),
    )
    .unwrap();
}

async fn run(machine: &Machine, args: &[&str]) -> Output {
    let mut command = machine.program(args);
    tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_trust_root_outside_the_allowed_folders_stops_the_engine_by_name() {
    let machine = Machine::new("orders", &[(PLACEHOLDER, PLACEHOLDER)]).with_passphrase();
    let outside = common::Folder::new("outside-roots");
    let elsewhere = outside.path().join("authority.pem");
    std::fs::write(&elsewhere, "made-up certificate text").unwrap();
    name_roots(&machine, &[&elsewhere]);
    let refused = run(&machine, &["engine", "status"]).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let error = &object(&refused)["error"];
    assert_eq!(error["code"], "trust_roots_unreadable", "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("files.allowed_roots"),
        "{error}"
    );

    // Inside the project folder, which `<project>` allows, a file with no
    // certificate is refused for that; a build without TLS refuses any.
    let inside = machine.project.path().join("authority.pem");
    std::fs::write(&inside, "made-up certificate text").unwrap();
    name_roots(&machine, &[&inside]);
    let refused = run(&machine, &["engine", "status"]).await;
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let expected = if cfg!(feature = "tls") {
        "trust_roots_unreadable"
    } else {
        "tls_not_enabled"
    };
    assert_eq!(object(&refused)["error"]["code"], expected, "{refused:?}");
}

#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_over_https_is_trusted_through_the_authority_the_installation_names() {
    use example_app::harness::TestApp;
    use example_app::tls::{ServerCertificate, TestAuthority};

    let authority = TestAuthority::new("example test authority").unwrap();
    let app = TestApp::start_tls(authority.server(ServerCertificate::Good).unwrap())
        .await
        .unwrap();
    let machine = Machine::new("orders", &[(PLACEHOLDER, &app.secure_url(""))]).with_passphrase();
    {
        let trust = a2m_auth::tls::Trust::with_named_roots(&[authority.pem().as_bytes()]).unwrap();
        let client = a2m_auth::tls::client_builder(&trust).no_proxy().build().unwrap();
        let got = client.get(app.secure_url("/api/orders/A-100")).send().await;
        eprintln!("DEBUG direct: {:?}", got.as_ref().map(|r| r.status()));
        if let Err(e) = &got { let mut s: Option<&dyn std::error::Error> = Some(e); while let Some(x) = s { eprintln!("DEBUG cause: {x}"); s = x.source(); } }
    }
    let world = machine.open().await;
    let published = world.person(&["version", "publish", "example"]).await;
    assert_eq!(published.finished.code, 0, "{:?}", published.finished);
    world.close().await;
    let read = [
        "tool",
        "call",
        "example_order_get",
        "--input",
        r#"{"order_id": "A-100"}"#,
    ];

    // Not named, the authority is nobody's: the certificate is refused by
    // name, and nothing was read.
    let untrusted = run(&machine, &read).await;
    let response = object(&untrusted);
    eprintln!("DEBUG first {}", response["result"]["run_id"]);
    assert_eq!(
        response["error"]["code"], "certificate_untrusted",
        "{response}"
    );

    // Named, inside the project folder, the same read is made over TLS.
    let pem = machine.project.path().join("authority.pem");
    std::fs::write(&pem, authority.pem()).unwrap();
    name_roots(&machine, &[&pem]);
    {
        let out = std::process::Command::new("curl").args(["-sS","-o","/dev/null","-w","%{http_code}","--cacert"]).arg(&pem).arg(app.secure_url("/api/orders/A-100")).output().unwrap();
        eprintln!("DEBUG curl {} {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    }
    let trusted = run(&machine, &read).await;
    eprintln!("DEBUG second {}", object(&trusted)["result"]["run_id"]);
    assert_eq!(trusted.status.code(), Some(0), "{trusted:?}");
    let response = object(&trusted);
    assert_eq!(response["result"]["status"], "completed", "{response}");
    assert_eq!(
        response["result"]["result"]["customer"],
        "Example Customer One"
    );
}
