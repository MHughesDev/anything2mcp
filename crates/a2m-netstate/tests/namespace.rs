//! On Linux: every reader, run in a child process inside a new network
//! namespace that holds only a loopback interface, gives that namespace's
//! own facts. A reader that reached outside this machine, or read another
//! namespace's tables, would show something else.
//!
//! The child is this test binary itself, started under `unshare` with a
//! variable that tells the one child test to run. Where the system refuses
//! a new namespace to this user (some hosted runners do), the test says so
//! and passes, unless `A2M_REQUIRE_NAMESPACE=1` makes the refusal a failure.
#![cfg(target_os = "linux")]

use std::process::Command;

use a2m_netstate::{InterfaceKind, LinkState};

const CHILD: &str = "A2M_NETSTATE_NAMESPACE_CHILD";

#[test]
fn every_reader_in_a_namespace_with_only_loopback_gives_that_namespaces_facts() {
    let exe = std::env::current_exe().unwrap();
    let ran = Command::new("unshare")
        .args(["--user", "--map-root-user", "--net", "--"])
        .arg(&exe)
        .args([
            "--exact",
            "the_child_reads_inside_the_namespace",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(CHILD, "1")
        .output();
    let required = std::env::var("A2M_REQUIRE_NAMESPACE").is_ok_and(|v| v == "1");
    let output = match ran {
        Ok(output) => output,
        Err(error) => {
            assert!(!required, "unshare could not be started: {error}");
            eprintln!("not run: unshare could not be started: {error}");
            return;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.contains("the child ran") {
        // `unshare` itself failed: the system refused the namespace.
        assert!(
            !required && !stdout.contains("panicked"),
            "the namespace was refused or the child failed:\n{stdout}\n{stderr}"
        );
        eprintln!("not run: the system refused a new namespace:\n{stderr}");
        return;
    }
    assert!(
        output.status.success(),
        "the child failed:\n{stdout}\n{stderr}"
    );
}

#[test]
fn the_child_reads_inside_the_namespace() {
    if std::env::var(CHILD).is_err() {
        return;
    }
    let interfaces = a2m_netstate::interfaces().unwrap();
    assert_eq!(interfaces.interfaces.len(), 1, "{interfaces:?}");
    let lo = &interfaces.interfaces[0];
    assert_eq!(lo.name, "lo");
    assert_eq!(lo.kind, Some(InterfaceKind::Loopback));
    // A new namespace's loopback is down and has no address.
    assert_eq!(lo.state, Some(LinkState::Down));
    assert!(lo.addresses.is_empty(), "{lo:?}");
    let routes = a2m_netstate::routes().unwrap();
    assert!(routes.routes.is_empty(), "{routes:?}");
    assert!(routes.default_routes.is_empty());
    let neighbours = a2m_netstate::neighbours().unwrap();
    assert!(neighbours.neighbours.is_empty(), "{neighbours:?}");
    let listeners = a2m_netstate::listeners().unwrap();
    assert!(listeners.listeners.is_empty(), "{listeners:?}");
    // The name servers and the proxy are files and variables, not the
    // namespace's: they read as outside it.
    let resolvers = a2m_netstate::resolvers().unwrap();
    let proxy = a2m_netstate::proxy().unwrap();
    let standing = a2m_netstate::standing().unwrap();
    assert!(standing.default_routes.is_empty());
    println!(
        "{}",
        serde_json::json!({
            "interfaces": interfaces,
            "resolvers": resolvers,
            "proxy": proxy,
            "standing": standing,
        })
    );
    println!("the child ran");
}
