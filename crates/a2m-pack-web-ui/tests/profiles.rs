//! Platform profiles, applied to a target's files: a surface names one, and
//! a locator built on an identifier its platform makes does not compile.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;

use a2m_core::NoSecrets;
use common::{Stand, compile_changed_files, shown};

/// The `cases` fixture, with the surface naming `profile` (or none), and
/// the field of `typed_input` also located by a selector on `id`.
fn compiled(profile: Option<&str>, id: &str) -> Result<(), String> {
    let stand = Stand::new("profiles", Arc::new(NoSecrets));
    let target = |text: &str| match profile {
        Some(name) => text.replacen(
            "    kind: web\n",
            &format!("    kind: web\n    profile: {name}\n"),
            1,
        ),
        None => text.to_owned(),
    };
    let screen = |text: &str| {
        text.replacen(
            "      - {role: textbox, name: Reference}",
            &format!("      - {{role: textbox, name: Reference}}\n      - {{css: \"#{id}\"}}"),
            1,
        )
    };
    let outcome = compile_changed_files(
        &stand.registry,
        "cases",
        &[
            ("target.yaml", &target),
            ("screens/typed_input.yaml", &screen),
        ],
    );
    outcome.map(|_| ()).map_err(|problems| shown(&problems))
}

#[test]
fn a_locator_built_on_an_identifier_the_platform_makes_does_not_compile() {
    let refused = compiled(Some("aspnet_webforms"), "ctl00_Content_Reference").unwrap_err();
    assert!(refused.contains("web.generated_id"), "{refused}");
    assert!(refused.contains("ctl00_Content_Reference"), "{refused}");
    assert!(refused.contains("aspnet_webforms"), "{refused}");
    // The fix says what to locate by instead.
    assert!(refused.contains("label"), "{refused}");
}

#[test]
fn the_same_locator_compiles_where_the_profile_does_not_reject_it() {
    // An identifier of the family that is not made by it.
    compiled(Some("aspnet_webforms"), "reference").unwrap();
    // The same made identifier, on a profile that rejects none.
    compiled(Some("plain"), "ctl00_Content_Reference").unwrap();
    // No profile named, and `auto`, which the probe names.
    compiled(None, "ctl00_Content_Reference").unwrap();
    compiled(Some("auto"), "ctl00_Content_Reference").unwrap();
}

#[test]
fn a_profile_that_is_not_one_is_refused_with_the_names_that_are() {
    let refused = compiled(Some("shiny_new_framework"), "reference").unwrap_err();
    assert!(refused.contains("web.profile_unknown"), "{refused}");
    for name in a2m_pack_web_ui::profiles::names() {
        assert!(refused.contains(name), "{refused}");
    }
}

#[test]
fn a_write_through_a_screen_of_a_read_only_profile_does_not_compile_and_a_read_does() {
    let refused = compiled(Some("virtualized_grid"), "reference").unwrap_err();
    let lines: Vec<&str> = refused
        .lines()
        .filter(|line| line.contains("web.profile_readonly"))
        .collect();
    assert!(!lines.is_empty(), "{refused}");
    // Each write bound through a screen is named, whatever its mode, with
    // the profile, its tier, and what to do instead.
    for write in ["example_order_delivery_update", "example_page_note_save"] {
        assert!(
            lines.iter().any(|line| line.contains(write)),
            "{write}: {refused}"
        );
    }
    assert!(
        lines
            .iter()
            .all(|line| line.contains("virtualized_grid") && line.contains("read_only")),
        "{refused}"
    );
    assert!(refused.contains("page_http"), "{refused}");
    // No read is refused: a read-only profile is read.
    for read in [
        "example_options_read",
        "example_note_read",
        "example_order_line_quantity_read",
    ] {
        assert!(
            !lines.iter().any(|line| line.contains(read)),
            "{read}: {refused}"
        );
    }
    // The same target under a profile that may write compiles.
    compiled(Some("plain"), "reference").unwrap();
}
