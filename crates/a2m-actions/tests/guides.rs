//! The guidance file and the built-in pages against what exists (package
//! 5.3): every step of the first hour is named in the guidance file, every
//! page the guidance file names and every page a failure's next step names
//! is one `docs.get` serves, every action a page names is in the registry,
//! and no page names an address that is not the imaginary `example`'s.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "this file holds only tests; the helpers the tests share do not carry the test attribute"
)]

use std::collections::BTreeSet;
use std::path::Path;

use a2m_actions::{ActionRegistry, SHORTCUTS};

/// The topics `docs.get` serves that are neither a guide nor an action: the
/// built-in topics of `src/handlers/meta.rs`. The first test holds this
/// list to that file.
const BUILT_IN_TOPICS: &[&str] = &["actions", "output", "settings"];

fn crate_file(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .replace('\r', "")
}

fn workspace_file(relative: &str) -> String {
    crate_file(&format!("../../{relative}"))
}

/// Every guide on disk: its name from the first line, and its page.
fn guides() -> Vec<(String, String)> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("guides");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&folder).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap().replace('\r', "");
        let (first, rest) = text.split_once('\n').unwrap();
        let name = first
            .strip_prefix("name: ")
            .unwrap_or_else(|| panic!("{}: no `name:` line", path.display()));
        assert_eq!(
            path.file_stem().unwrap().to_str().unwrap(),
            name,
            "a guide's file is named as the guide is"
        );
        found.push((name.to_owned(), rest.to_owned()));
    }
    found.sort();
    found
}

/// The text between backticks on a line, in order.
fn quoted(line: &str) -> Vec<&str> {
    line.split('`').skip(1).step_by(2).collect()
}

/// What `docs.get` serves: every guide, the built-in topics, every action.
fn served(registry: &ActionRegistry) -> BTreeSet<String> {
    let mut topics: BTreeSet<String> = guides().into_iter().map(|(name, _)| name).collect();
    topics.extend(BUILT_IN_TOPICS.iter().map(|name| (*name).to_owned()));
    topics.extend(registry.iter().map(|def| def.name.to_string()));
    topics
}

fn guidance() -> String {
    guides()
        .into_iter()
        .find(|(name, _)| name == "guidance")
        .expect("the guidance file")
        .1
}

#[test]
fn the_built_in_topics_listed_here_are_the_ones_docs_get_serves() {
    let source = crate_file("src/handlers/meta.rs");
    let start = source.find("const TOPICS").expect("the topics table");
    let table = &source[start..];
    let table = &table[..table.find("\n];").expect("the end of the table")];
    let found: Vec<&str> = table
        .lines()
        .filter_map(|line| {
            line.strip_prefix("        \"")
                .and_then(|rest| rest.strip_suffix("\","))
        })
        .collect();
    assert_eq!(found, BUILT_IN_TOPICS);
    for (name, _) in guides() {
        assert!(!BUILT_IN_TOPICS.contains(&name.as_str()), "{name}");
    }
}

#[test]
fn each_step_of_the_first_hour_is_named_in_the_guidance_file() {
    let page = guidance();
    let named: BTreeSet<&str> = page.lines().flat_map(quoted).collect();
    // The steps, as the shortcuts name them: `init` has no action beneath
    // it (it writes this file); the others are made of actions.
    for word in [
        "init", "add", "lint", "try", "test", "ready", "publish", "call",
    ] {
        let shortcut = SHORTCUTS
            .iter()
            .find(|shortcut| shortcut.word == word)
            .unwrap_or_else(|| panic!("no shortcut `{word}`"));
        if shortcut.steps.is_empty() {
            assert!(
                named.iter().any(|text| text.split(' ').any(|w| w == word)),
                "the guidance file does not name the shortcut `{word}`"
            );
        }
        for step in shortcut.steps {
            assert!(
                named.contains(step.action),
                "the guidance file does not name `{}`, a step of `{word}`",
                step.action
            );
        }
    }
    // The steps that are no shortcut's.
    for action in ["draft.delete", "fixture.record", "binding.promote"] {
        assert!(
            named.contains(action),
            "the guidance file does not name `{action}`"
        );
    }
}

#[test]
fn every_page_the_guidance_file_names_is_served_and_every_guide_is_named_by_it() {
    let registry = ActionRegistry::kernel_only();
    let topics = served(&registry);
    let page = guidance();
    let more = page.split("\n## More\n").nth(1).expect("a `More` section");
    let named: Vec<&str> = more.lines().flat_map(quoted).collect();
    assert!(named.len() >= 10, "{named:?}");
    for topic in &named {
        if *topic != "docs.get" {
            assert!(topics.contains(*topic), "the guidance file names `{topic}`");
        }
    }
    // A page nothing points to is a page no agent finds.
    for (name, _) in guides() {
        if name != "guidance" {
            assert!(
                named.contains(&name.as_str()),
                "the guidance file does not name the page `{name}`"
            );
        }
    }
    // Wherever any page sends `docs.get` on the same line.
    for (name, page) in guides() {
        for line in page.lines() {
            let Some((_, after)) = line.split_once("`docs.get`") else {
                continue;
            };
            for topic in quoted(after).into_iter().filter(|t| *t != "docs.get") {
                assert!(
                    topics.contains(topic),
                    "{name} sends `docs.get` to `{topic}`"
                );
            }
        }
    }
}

#[test]
fn every_page_a_failures_next_step_names_is_served() {
    let registry = ActionRegistry::kernel_only();
    let topics = served(&registry);
    let reference = workspace_file("docs/reference/technical-reference.md");
    let begin = reference.find("<!-- failure-codes:begin").unwrap();
    let end = reference.find("<!-- failure-codes:end").unwrap();
    let mut rows = 0;
    for row in reference[begin..end]
        .lines()
        .filter(|l| l.starts_with("| `"))
    {
        rows += 1;
        let cells: Vec<&str> = row.split(" | ").collect();
        let next_step = cells[4];
        // A next step that sends a caller to a page says `docs.get` and
        // then the page, or names a guide's own name.
        if let Some((_, after)) = next_step.split_once("docs.get") {
            for topic in quoted(after) {
                assert!(topics.contains(topic), "{} sends to `{topic}`", cells[1]);
            }
        }
        for topic in quoted(next_step) {
            if guides().iter().any(|(name, _)| name == topic) {
                assert!(topics.contains(topic));
            }
        }
    }
    assert!(rows > 100, "{rows} rows of the failure table");
}

#[test]
fn every_action_a_page_names_is_in_the_registry() {
    let registry = ActionRegistry::kernel_only();
    let nouns: BTreeSet<&str> = registry
        .iter()
        .filter_map(|def| def.name.as_str().split('.').next())
        .collect();
    let mut seen = 0;
    for (name, page) in guides() {
        for text in page.lines().flat_map(quoted) {
            let Some((noun, verb)) = text.split_once('.') else {
                continue;
            };
            let shaped = |word: &str| {
                !word.is_empty() && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
            };
            // A file's name, `target.yaml`, is not an action's.
            let file = matches!(verb, "yaml" | "json" | "md" | "toml" | "txt");
            if shaped(noun) && shaped(verb) && !file && nouns.contains(noun) {
                seen += 1;
                assert!(
                    registry.find(text).is_some(),
                    "{name} names the action `{text}`, which is not in the registry"
                );
            }
        }
    }
    assert!(seen > 40, "{seen}");
}

#[test]
fn no_page_names_an_address_that_is_not_the_example_targets() {
    for (name, page) in guides() {
        for (at, _) in page.match_indices("://") {
            // A vault reference is a name in the vault, not an address.
            if page[..at].ends_with("vault") {
                continue;
            }
            let host: String = page[at + 3..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
                .collect();
            let imaginary = host == "localhost"
                || host == "127.0.0.1"
                || host == "example"
                || host == "example.com"
                || host.ends_with(".example")
                || host.ends_with(".example.com")
                || host.ends_with(".test");
            assert!(imaginary, "{name} names the address `{host}`");
        }
        // No mailbox either: a name before an `@` and a dotted name after.
        for (at, _) in page.match_indices('@') {
            let after: String = page[at + 1..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
                .collect();
            assert!(
                !after.contains('.') || after.ends_with("example.com"),
                "{name} names a mailbox at `{after}`"
            );
        }
    }
}
