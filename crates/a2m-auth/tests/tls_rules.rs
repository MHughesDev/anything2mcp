//! Rules about TLS that hold over the repository's sources (decision 0157):
//! every HTTP client of the product is made by `a2m_auth::tls`, nothing
//! switches certificate checking off, and no setting, pack setting, or
//! target-file key exists that would.
//!
//! The rules read source text. They run in every build, with TLS on or off.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a file that cannot be read stops the test"
)]

use std::fs;
use std::path::{Path, PathBuf};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

/// Every file under `folder` whose name ends in one of `endings`, sorted.
fn files(folder: &Path, endings: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.map(|entry| entry.unwrap().path()).collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            if entry.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            files(&entry, endings, out);
        } else if entry
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| endings.iter().any(|ending| name.ends_with(ending)))
        {
            out.push(entry);
        }
    }
}

fn shown(path: &Path) -> String {
    path.strip_prefix(repository())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// This file, which names what it forbids.
fn is_this_file(path: &Path) -> bool {
    path.ends_with(Path::new("a2m-auth").join("tests").join("tls_rules.rs"))
}

/// Whether `line` holds `needle` as a name of its own, not as the end of a
/// longer one.
fn names(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(at, _)| {
        !line[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    })
}

#[test]
fn every_http_client_of_the_product_crates_is_made_by_the_tls_module() {
    let root = repository();
    let mut sources = Vec::new();
    for crate_name in ["a2m-auth", "a2m-pack-http", "a2m-pack-mcp"] {
        let folder = root.join("crates").join(crate_name);
        files(&folder.join("src"), &[".rs"], &mut sources);
        files(&folder.join("tests"), &[".rs"], &mut sources);
    }
    assert!(sources.len() > 10, "the sources were not found");
    let own = Path::new("src").join("tls.rs");
    let made_by_hand = [
        concat!("Client", "::builder("),
        concat!("Client", "::new("),
        concat!("ClientBuilder", "::new("),
    ];
    let mut found = Vec::new();
    for source in &sources {
        if source.ends_with(&own) || is_this_file(source) {
            continue;
        }
        let text = fs::read_to_string(source).unwrap();
        for (number, line) in text.lines().enumerate() {
            if made_by_hand.iter().any(|needle| names(line, needle)) {
                found.push(format!("{}:{}", shown(source), number + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "an HTTP client made without `a2m_auth::tls::client_builder` stops the process in a build with TLS on: {found:?}"
    );
}

#[test]
fn nothing_switches_certificate_checking_off() {
    let root = repository();
    let mut sources = Vec::new();
    for top in ["crates", "targets", "bench"] {
        files(&root.join(top), &[".rs"], &mut sources);
    }
    assert!(sources.len() > 100, "the sources were not found");
    let forbidden = [
        concat!("danger_accept_", "invalid"),
        concat!("NoVerifier", ""),
        concat!("ServerCertVerified", "::assertion"),
        concat!("set_certificate_", "verifier"),
    ];
    let mut found = Vec::new();
    for source in &sources {
        if is_this_file(source) {
            continue;
        }
        let text = fs::read_to_string(source).unwrap();
        for (number, line) in text.lines().enumerate() {
            if forbidden.iter().any(|needle| line.contains(needle)) {
                found.push(format!("{}:{}", shown(source), number + 1));
            }
        }
        // The one custom verifier is `a2m_auth::tls`'s, which checks
        // against the named roots and then the system's; it never accepts
        // a certificate by itself.
        let dangerous = text.matches(concat!(".dangerous", "()")).count();
        let own = source.ends_with(Path::new("a2m-auth").join("src").join("tls.rs"));
        if dangerous > usize::from(own) {
            found.push(format!("{}: a custom certificate verifier", shown(source)));
        }
    }
    assert!(found.is_empty(), "{found:?}");
}

/// The names in quotes or backticks in a text that could be a key of a
/// setting or a target file: lower case, digits, `_`, and `.`. A switch of
/// another program, written with `-`, is not one.
fn keys(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for mark in ['"', '`'] {
        for (at, part) in text.split(mark).enumerate() {
            let key_like = !part.is_empty()
                && part.len() <= 80
                && part.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.')
                });
            if at % 2 == 1 && key_like {
                out.push(part.to_owned());
            }
        }
    }
    out
}

#[test]
fn no_setting_pack_setting_or_target_file_key_switches_certificate_checking_off() {
    let root = repository();
    let mut schemas = Vec::new();
    // The settings schema and every pack's manifest, with its settings.
    files(
        &root
            .join("crates")
            .join("a2m-actions")
            .join("src")
            .join("settings"),
        &[".rs"],
        &mut schemas,
    );
    files(
        &root.join("crates"),
        &["anything2mcp-provider.toml"],
        &mut schemas,
    );
    // The target files' schemas: the file kinds of the core, and the
    // surface kinds and file kinds each pack registers.
    files(
        &root.join("crates").join("a2m-files").join("src"),
        &[".rs"],
        &mut schemas,
    );
    for entry in fs::read_dir(root.join("crates")).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("a2m-pack-"))
        {
            files(&path.join("src"), &[".rs"], &mut schemas);
        }
    }
    // What the documents say the settings and the file formats are.
    schemas.push(root.join("docs").join("reference").join("file-formats.md"));
    schemas.push(
        root.join("docs")
            .join("spec")
            .join("2-kernel")
            .join("226-configuration.md"),
    );
    assert!(schemas.len() > 20, "the schemas were not found");
    let words = [
        "insecure",
        "accept_invalid",
        "allow_invalid",
        "skip_verif",
        "no_verif",
        "disable_verif",
        "verify_tls",
        "tls_verify",
        "verify_cert",
        "verify_peer",
        "verify_host",
        "certificate_check",
        "check_certificate",
        "ignore_cert",
        "skip_tls",
        "danger",
    ];
    let mut found = Vec::new();
    for schema in &schemas {
        let text = fs::read_to_string(schema).unwrap();
        for key in keys(&text) {
            if words.iter().any(|word| key.contains(word)) {
                found.push(format!("{}: `{key}`", shown(schema)));
            }
        }
    }
    assert!(found.is_empty(), "{found:?}");
}

/// The same rule over every crate, test stand, and the benchmark harness
/// (decision 0166), so that the whole workspace can be built and tested
/// with TLS on: a client of the product or of a test that acts through
/// the product is made by `a2m_auth::tls::client_builder`; a client that a
/// test stand or a test uses as a plain user of a stand is made by the
/// stand's `example_app::harness::client_builder`, which states its
/// provider too. Nothing else makes one.
#[test]
fn every_http_client_of_the_workspace_is_made_by_one_of_the_two_helpers() {
    let root = repository();
    let mut sources = Vec::new();
    for top in ["crates", "targets", "bench"] {
        files(&root.join(top), &[".rs"], &mut sources);
    }
    assert!(sources.len() > 100, "the sources were not found");
    let helpers = [
        Path::new("a2m-auth").join("src").join("tls.rs"),
        Path::new("example-app").join("src").join("harness.rs"),
    ];
    // Clients a crate still makes by hand, each in a crate whose owner
    // changes it: listed so that the rule holds everywhere else today, and
    // so that each entry is removed when its line is changed.
    let owed: [&str; 0] = [];
    let made_by_hand = [
        concat!("Client", "::builder("),
        concat!("Client", "::new("),
        concat!("ClientBuilder", "::new("),
    ];
    let mut found = Vec::new();
    let mut still_owed = Vec::new();
    for source in &sources {
        if helpers.iter().any(|helper| source.ends_with(helper)) || is_this_file(source) {
            continue;
        }
        let text = fs::read_to_string(source).unwrap();
        // Only `reqwest`'s client: another library's `Client::new` is no
        // HTTP client of this kind.
        let http = text.contains("reqwest");
        for (number, line) in text.lines().enumerate() {
            if http && made_by_hand.iter().any(|needle| names(line, needle)) {
                let shown = shown(source);
                if owed.contains(&shown.as_str()) {
                    still_owed.push(shown);
                } else {
                    found.push(format!("{shown}:{}", number + 1));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "an HTTP client made without `a2m_auth::tls::client_builder` or `example_app::harness::client_builder` stops the process in a build with TLS on: {found:?}"
    );
    for entry in owed {
        assert!(
            still_owed.iter().any(|shown| shown == entry),
            "{entry} makes its clients through a helper now: remove it from the list of what is owed"
        );
    }
}

/// The text of `fn client_builder` in `text`, up to the end of its body.
fn helper_body(text: &str) -> Option<&str> {
    let start = text.find("pub fn client_builder(")?;
    let rest = &text[start..];
    let end = rest.find("\n}\n")?;
    Some(&rest[..end])
}

/// No client of the workspace takes a proxy from the environment (decision
/// 0197). Every client is made by one of the two helpers (the rule above),
/// so the helpers refuse the environment's proxy themselves, with
/// `.no_proxy()` on what they return, and nothing anywhere names a proxy
/// of `reqwest` or adds one to a builder. A client that could take a proxy
/// from the environment fails one of these.
#[test]
fn no_http_client_of_the_workspace_takes_a_proxy_from_the_environment() {
    let root = repository();
    // Helpers whose owner has still to add `.no_proxy()`: listed so that the
    // rule holds everywhere else today, and removed once the line is added.
    let owed = ["targets/example-app/src/harness.rs"];
    let helpers = [
        root.join("crates")
            .join("a2m-auth")
            .join("src")
            .join("tls.rs"),
        root.join("targets")
            .join("example-app")
            .join("src")
            .join("harness.rs"),
    ];
    let mut found = Vec::new();
    for helper in &helpers {
        let text = fs::read_to_string(helper).unwrap();
        let body = helper_body(&text).unwrap();
        let refuses = body.contains(".no_proxy()");
        let shown = shown(helper);
        match (refuses, owed.contains(&shown.as_str())) {
            (false, false) => found.push(format!("{shown}: `client_builder` has no `.no_proxy()`")),
            (true, true) => found.push(format!(
                "{shown}: refuses the environment's proxy now: remove it from the list of what is owed"
            )),
            _ => {}
        }
    }
    let mut sources = Vec::new();
    for top in ["crates", "targets", "bench"] {
        files(&root.join(top), &[".rs"], &mut sources);
    }
    assert!(sources.len() > 100, "the sources were not found");
    let adds_a_proxy = [
        concat!("reqwest::", "Proxy"),
        concat!("Proxy", "::all("),
        concat!("Proxy", "::http("),
        concat!("Proxy", "::https("),
        concat!("Proxy", "::custom("),
        concat!(".proxy", "("),
    ];
    for source in &sources {
        if is_this_file(source) {
            continue;
        }
        let text = fs::read_to_string(source).unwrap();
        if !text.contains("reqwest") {
            continue;
        }
        for (number, line) in text.lines().enumerate() {
            if adds_a_proxy.iter().any(|needle| {
                names(line, needle) || (needle.starts_with('.') && line.contains(needle))
            }) {
                found.push(format!("{}:{}: a proxy", shown(source), number + 1));
            }
        }
    }
    assert!(found.is_empty(), "{found:?}");
}
