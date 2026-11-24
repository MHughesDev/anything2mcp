//! Platform profiles: one preset for each family of web interface, as data.
//!
//! A profile says how a family behaves: how to detect it, how to know a page
//! is ready, which locators to trust and in what order, which identifiers a
//! locator may not be built on, how a value is committed, whether frames are
//! declared or found, how often the family changes, and how far a write is
//! allowed to go. The files are in `profiles/` at the root of the repository,
//! are validated here, and are embedded in the program: adding one changes
//! no code but the list below, and a test holds the list to the folder.
//!
//! Detecting a family, which uses the `detect` rules and a page, is the
//! probe's. Applying a profile to a surface is this crate's: so far, the
//! identifier patterns, by the lint `web.generated_id`, and the name, by
//! `web.profile_unknown`. The readiness rule, the input commit, the frame
//! mode, and the tier's limit on writes are read and checked here and are not
//! applied yet (decision 0399, the profiles record).

use std::sync::OnceLock;

use a2m_files::{Doc, Located, RelPath, parse_yaml};
use regex::Regex;

/// The names of the profiles' files, with their text, embedded. A test holds
/// this list to the folder `profiles/`.
const FILES: &[(&str, &str)] = &[
    (
        "aspnet_webforms",
        include_str!("../../../profiles/aspnet_webforms.yaml"),
    ),
    (
        "controlled_inputs",
        include_str!("../../../profiles/controlled_inputs.yaml"),
    ),
    ("plain", include_str!("../../../profiles/plain.yaml")),
    (
        "virtualized_grid",
        include_str!("../../../profiles/virtualized_grid.yaml"),
    ),
];

/// The locator kinds a profile may put in its priority.
const LOCATORS: &[&str] = &[
    "role",
    "label",
    "placeholder",
    "text",
    "test_id",
    "title",
    "css",
];
/// The ways a value may be committed in the page.
const COMMITS: &[&str] = &[
    "none",
    "blur",
    "enter",
    "blur_then_wait_ready",
    "native_set_then_blur",
];
/// The kinds of evidence a rule of `detect` gives.
const DETECTS: &[&str] = &["dom", "js", "script", "header", "cookie"];
/// The tiers of support.
const TIERS: &[&str] = &["full", "read_only", "api_preferred", "unsupported"];
/// The keys a profile file may have.
const KEYS: &[&str] = &[
    "profile",
    "detect",
    "ready",
    "locators",
    "input_commit",
    "frames",
    "drift_risk",
    "support_tier",
];
/// The longest a pattern of identifiers may be.
const MOST_PATTERN: usize = 200;

/// One rule that says a page is of a family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectRule {
    /// What the rule looks at: `dom`, `js`, `script`, `header`, or `cookie`.
    pub kind: String,
    /// What it looks for.
    pub of: String,
}

/// A platform profile.
#[derive(Debug, Clone)]
pub struct Profile {
    /// Its name, which a surface's `profile` names.
    pub name: String,
    /// The rules of which any one names the family, with a confidence.
    pub detect: Vec<DetectRule>,
    /// The family's own test that a page is ready, as script text. Not run
    /// by this crate yet.
    pub ready_js: Option<String>,
    /// The locator kinds, in the order to trust them.
    pub locator_priority: Vec<String>,
    /// The identifiers a locator may not be built on.
    pub reject_id_patterns: Vec<Regex>,
    /// How a value is committed in the page.
    pub input_commit: String,
    /// Whether frames are `declared` or `found`.
    pub frames: String,
    /// How often the family changes: `low`, `medium`, or `high`.
    pub drift_risk: String,
    /// `full`, `read_only`, `api_preferred`, or `unsupported`.
    pub support_tier: String,
}

impl Profile {
    /// Whether an identifier is one the profile says is made, not chosen.
    #[must_use]
    pub fn rejects_id(&self, id: &str) -> bool {
        self.reject_id_patterns
            .iter()
            .any(|pattern| pattern.is_match(id))
    }

    /// Whether a write may be bound on a surface of this profile.
    #[must_use]
    pub fn may_write(&self) -> bool {
        self.support_tier == "full" || self.support_tier == "api_preferred"
    }
}

fn words<'a>(
    doc: &'a Located<Doc>,
    key: &str,
    known: &[&str],
    what: &str,
) -> Result<&'a str, String> {
    let text = doc
        .get(key)
        .and_then(Located::as_text)
        .ok_or_else(|| format!("`{key}` is missing, or is not text"))?;
    if known.contains(&text) {
        Ok(text)
    } else {
        Err(format!("`{key}` is `{text}`, which is not {what}"))
    }
}

/// Reads one profile's text, held to the schema: the keys it may have, the
/// words each takes, and patterns that are expressions this crate can run.
///
/// # Errors
///
/// A sentence that says what is wrong, and where.
pub fn parse(file: &str, text: &str) -> Result<Profile, String> {
    let at = RelPath::new(&format!("profiles/{file}.yaml"))
        .map_err(|_| "the file's name is not one a path may have".to_owned())?;
    let docs = parse_yaml(&at, text).map_err(|found| found.to_string())?;
    let [doc] = docs.as_slice() else {
        return Err("a profile file holds exactly one document".to_owned());
    };
    for entry in doc.entries() {
        if !KEYS.contains(&entry.key.value.as_str()) {
            return Err(format!("`{}` is not a key of a profile", entry.key.value));
        }
    }
    let name = doc
        .get("profile")
        .and_then(Located::as_text)
        .ok_or("`profile` is missing, or is not text")?;
    if name != file {
        return Err(format!(
            "the file is named `{file}` and the profile `{name}`"
        ));
    }
    let mut detect = Vec::new();
    let any = doc
        .get("detect")
        .and_then(|detect| detect.get("any"))
        .ok_or("`detect` holds an `any` list")?;
    for rule in any.items() {
        let [entry] = rule.entries() else {
            return Err("a rule of `detect` states exactly one thing".to_owned());
        };
        if !DETECTS.contains(&entry.key.value.as_str()) {
            return Err(format!("`{}` is not a rule of `detect`", entry.key.value));
        }
        let of = entry
            .value
            .as_text()
            .filter(|of| !of.is_empty())
            .ok_or("a rule of `detect` names what to look for as text")?;
        detect.push(DetectRule {
            kind: entry.key.value.clone(),
            of: of.to_owned(),
        });
    }
    let ready_js = match doc.get("ready") {
        Some(ready) => Some(
            ready
                .get("js")
                .and_then(Located::as_text)
                .ok_or("`ready` holds a `js` text")?
                .to_owned(),
        ),
        None => None,
    };
    let locators = doc.get("locators").ok_or("`locators` is missing")?;
    let mut locator_priority = Vec::new();
    for item in locators
        .get("priority")
        .ok_or("`locators` holds a `priority` list")?
        .items()
    {
        let kind = item.as_text().ok_or("a locator kind is text")?;
        if !LOCATORS.contains(&kind) {
            return Err(format!("`{kind}` is not a kind of locator"));
        }
        if locator_priority.iter().any(|known| known == kind) {
            return Err(format!("`{kind}` is in the priority twice"));
        }
        locator_priority.push(kind.to_owned());
    }
    if locator_priority.is_empty() {
        return Err("the locator priority is not empty".to_owned());
    }
    let mut reject_id_patterns = Vec::new();
    for item in locators
        .get("reject_id_patterns")
        .ok_or("`locators` holds a `reject_id_patterns` list, which may be empty")?
        .items()
    {
        let pattern = item.as_text().ok_or("a pattern is text")?;
        if pattern.len() > MOST_PATTERN {
            return Err(format!("a pattern is longer than {MOST_PATTERN} bytes"));
        }
        reject_id_patterns.push(
            Regex::new(pattern)
                .map_err(|_| format!("`{pattern}` is not a pattern this crate runs"))?,
        );
    }
    let input_commit = words(doc, "input_commit", COMMITS, "a way to commit a value")?.to_owned();
    let frames = doc
        .get("frames")
        .and_then(|frames| frames.get("mode"))
        .and_then(Located::as_text)
        .filter(|mode| ["declared", "found"].contains(mode))
        .ok_or("`frames` holds a `mode`: `declared` or `found`")?
        .to_owned();
    let drift_risk = words(
        doc,
        "drift_risk",
        &["low", "medium", "high"],
        "a drift risk",
    )?
    .to_owned();
    let support_tier = words(doc, "support_tier", TIERS, "a tier of support")?.to_owned();
    Ok(Profile {
        name: name.to_owned(),
        detect,
        ready_js,
        locator_priority,
        reject_id_patterns,
        input_commit,
        frames,
        drift_risk,
        support_tier,
    })
}

/// Every profile the program holds, read once.
///
/// # Errors
///
/// The first file that does not fit, as a sentence. A test reads them all,
/// so that none that does not fit is ever embedded.
pub fn all() -> Result<&'static [Profile], &'static str> {
    static HELD: OnceLock<Result<Vec<Profile>, String>> = OnceLock::new();
    HELD.get_or_init(|| {
        FILES
            .iter()
            .map(|(name, text)| {
                parse(name, text).map_err(|why| format!("profiles/{name}.yaml: {why}"))
            })
            .collect()
    })
    .as_ref()
    .map(Vec::as_slice)
    .map_err(String::as_str)
}

/// The profile of a name.
#[must_use]
pub fn named(name: &str) -> Option<&'static Profile> {
    all().ok()?.iter().find(|profile| profile.name == name)
}

/// The names of the profiles, and `auto`, which a surface may name.
#[must_use]
pub fn names() -> Vec<&'static str> {
    FILES.iter().map(|(name, _)| *name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file of the folder is embedded, every embedded file is in the
    /// folder, and each of them fits the schema.
    #[test]
    fn every_profile_of_the_folder_is_embedded_and_fits() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("profiles");
        let mut found: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        let mut held: Vec<String> = FILES
            .iter()
            .map(|(name, _)| format!("{name}.yaml"))
            .collect();
        held.sort();
        assert_eq!(found, held);
        let profiles = all().unwrap_or_else(|why| panic!("{why}"));
        assert_eq!(profiles.len(), FILES.len());
        for profile in profiles {
            assert!(!profile.locator_priority.is_empty(), "{}", profile.name);
        }
    }

    #[test]
    fn a_family_rejects_the_identifiers_it_makes_and_only_those() {
        let forms = named("aspnet_webforms").unwrap();
        assert!(forms.rejects_id("ctl00_Content_Save"));
        assert!(forms.rejects_id("MainContent_Total"));
        assert!(!forms.rejects_id("save"));
        assert!(named("plain").unwrap().may_write());
        assert!(!named("virtualized_grid").unwrap().may_write());
    }

    /// What does not fit is refused with a sentence, and nothing is guessed.
    #[test]
    fn a_profile_that_does_not_fit_is_refused_and_says_why() {
        let good = "profile: x\ndetect: {any: []}\nlocators: {priority: [role], reject_id_patterns: []}\ninput_commit: blur\nframes: {mode: declared}\ndrift_risk: low\nsupport_tier: full\n";
        assert!(parse("x", good).is_ok());
        for (bad, says) in [
            (
                good.replace("support_tier: full", "support_tier: mostly"),
                "tier of support",
            ),
            (
                good.replace("input_commit: blur", "input_commit: hope"),
                "commit",
            ),
            (good.replace("[role]", "[role, role]"), "twice"),
            (good.replace("[role]", "[xpath]"), "kind of locator"),
            (
                good.replace("reject_id_patterns: []", "reject_id_patterns: [\"(\"]"),
                "pattern",
            ),
            (
                good.replace("detect: {any: []}", "detect: {any: [{dom: a, js: b}]}"),
                "exactly one",
            ),
            (format!("{good}retries: 3\n"), "not a key"),
            (good.replace("profile: x", "profile: y"), "named"),
        ] {
            let why = parse("x", &bad).unwrap_err();
            assert!(why.contains(says), "{says}: {why}");
        }
    }
}
