//! The fixed rules that recommend a sign-in from findings.

use a2m_core::{
    Address, EvidenceLevel, Finding, FindingKind, ProducedBy, Source, Subject, Timestamp,
};

use crate::{INFERRED, SignInSign, recommend};

fn finding(found: &str, kind: &str) -> Finding {
    Finding::about(
        Subject::Address(Address::new("http://example.test", "/").unwrap()),
        FindingKind::new(kind).unwrap(),
        found,
        Source::LiveSystem,
        ProducedBy {
            provider: "test".to_owned(),
            version: "0".to_owned(),
            component: None,
        },
        Timestamp::from_unix_millis(0),
    )
    .level(EvidenceLevel::Inferred)
    .build()
    .unwrap()
}

fn sign(name: &str, second_factor: &str) -> Finding {
    finding(
        &format!(r#"{{"format":"1","sign_in":"{name}","second_factor":"{second_factor}"}}"#),
        "sign_in_method",
    )
}

#[test]
fn every_sign_has_a_row_and_a_name_that_parses_back() {
    for each in SignInSign::ALL {
        assert_eq!(SignInSign::parse(each.name()), Some(each));
        let one = recommend(&[sign(each.name(), "asked")]);
        assert_eq!(one.choices.len() + one.questions.len(), 1, "{each:?}");
    }
}

#[test]
fn a_password_form_is_a_managed_session_and_other_origin_is_oidc_unless_saml_was_posted() {
    let form = recommend(&[sign("password_form", "unknown")]);
    assert_eq!(form.first().unwrap().provider, "form_login");
    assert_eq!(form.first().unwrap().mode, "managed_session");
    assert!(form.unknown.contains(&"second_factor"));
    let moved = recommend(&[sign("other_origin", "unknown")]);
    assert_eq!(moved.first().unwrap().provider, "sso_oidc");
    let saml = recommend(&[sign("saml_post", "unknown")]);
    assert_eq!(saml.first().unwrap().provider, "sso_saml");
    let code = recommend(&[sign("code_step", "asked")]);
    assert_eq!(code.first().unwrap().provider, "window_login");
    assert!(!code.unknown.contains(&"second_factor"));
    assert_eq!(form.to_json()["level"], INFERRED);
}

#[test]
fn an_unknown_sign_is_a_question_and_never_a_guess() {
    let none = recommend(&[]);
    assert!(none.choices.is_empty());
    assert!(none.unknown.contains(&"identity_mode"));
    let odd = recommend(&[
        sign("something_new", "unknown"),
        sign("challenge", "unknown"),
    ]);
    assert!(odd.choices.is_empty());
    assert_eq!(odd.questions.len(), 2);
    let other_kind = recommend(&[finding("{}", "endpoint")]);
    assert!(other_kind.choices.is_empty() && other_kind.from.is_empty());
}

#[test]
fn the_order_of_findings_does_not_change_the_answer() {
    let (one, two) = (sign("http_basic", "unknown"), sign("api_key", "unknown"));
    let forward = recommend(&[one.clone(), two.clone()]);
    let backward = recommend(&[two, one]);
    assert_eq!(forward, backward);
    assert_eq!(forward.choices.len(), 2);
}
