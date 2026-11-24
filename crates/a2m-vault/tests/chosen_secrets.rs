//! A secret a person chooses: one too short to mask safely is refused, and
//! one that replaces another stays masked (decision 0338).
//! Every value here is invented.

use std::sync::Arc;

use a2m_core::{FailureClass, REDACTED, Redact, Secret, WorkspaceId};
use a2m_vault::{
    ItemId, MemoryKeyRecords, MemoryKeyStore, OwnerRef, SealContext, SealedKind, Vault, codes,
};

fn vault() -> Vault {
    Vault::new(
        Arc::new(MemoryKeyStore::new()) as _,
        Arc::new(MemoryKeyRecords::new()) as _,
    )
}

fn context(item: &str) -> SealContext {
    SealContext {
        workspace: WorkspaceId::from_bytes([1; 16]),
        owner: OwnerRef::person("ana").unwrap_or_else(|failure| panic!("{failure}")),
        kind: SealedKind::SessionState,
        item: ItemId::new(item).unwrap_or_else(|failure| panic!("{failure}")),
    }
}

#[test]
fn a_secret_too_short_to_mask_safely_is_refused_and_registered_nowhere() {
    let vault = vault();
    let cx = context("one");
    for short in ["Z", "1234567", "é23456"] {
        let failure = vault
            .register_chosen(&cx, &Secret::new(short))
            .expect_err("a short secret is refused");
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &codes::SECRET_TOO_SHORT);
        let said = format!("{failure} {failure:?} {}", failure.next_step());
        assert!(!said.contains(short), "{said}");
        // Nothing was registered in its place: other text is untouched.
        let text = "Z 1234567 é23456";
        assert_eq!(vault.redactor().redact(text), text);
        assert_eq!(vault.redactor().health().registered, 0);
    }
    // The least that is accepted is eight bytes.
    vault
        .register_chosen(&cx, &Secret::new("12345678"))
        .expect("eight bytes are accepted");
    assert_eq!(
        vault.redactor().redact("a 12345678 b"),
        format!("a {REDACTED} b")
    );
}

#[test]
fn a_replaced_secret_stays_masked_for_the_life_of_the_process() {
    let vault = vault();
    let cx = context("one");
    let old = "old-planted-value-4711";
    let new = "new-planted-value-8150";
    let redactor = vault.redactor();
    vault.register_chosen(&cx, &Secret::new(old)).expect("old");
    vault.register_chosen(&cx, &Secret::new(new)).expect("new");
    assert_eq!(redactor.redact(old), REDACTED);
    assert!(redactor.contains_secret(old));
    assert_eq!(redactor.redact(new), REDACTED);
}

#[tokio::test]
async fn opening_a_replacement_keeps_the_value_it_replaces_masked() {
    let vault = vault();
    let cx = context("one");
    let redactor = vault.redactor();
    for value in ["first-planted-secret-1", "second-planted-secret-2"] {
        let sealed = vault.seal(&cx, value.as_bytes()).await.expect("seal");
        let opened = vault.open_secret(&cx, &sealed).await.expect("open");
        assert_eq!(opened.expose(), value);
    }
    assert!(redactor.contains_secret("first-planted-secret-1"));
    assert!(redactor.contains_secret("second-planted-secret-2"));
}
