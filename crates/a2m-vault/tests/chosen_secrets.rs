//! A secret a person chooses: one too short to mask safely is refused, and
//! one that replaces another releases the old value at once (decision 0399).
//! Every value here is invented.

use std::sync::Arc;

use a2m_core::{FailureClass, REDACTED, Redact, Secret, WorkspaceId};
use a2m_vault::{
    ItemId, MemoryKeyRecords, MemoryKeyStore, OwnerRef, Redactor, SealContext, SealedKind, Vault,
    codes,
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
fn a_replaced_secret_is_no_longer_masked_and_the_new_one_is() {
    let vault = vault();
    let cx = context("one");
    let old = "old-planted-value-4711";
    let new = "new-planted-value-8150";
    let redactor = vault.redactor();

    vault.register_chosen(&cx, &Secret::new(old)).expect("old");
    assert!(redactor.contains_secret(old));
    vault.register_chosen(&cx, &Secret::new(new)).expect("new");
    assert!(!redactor.contains_secret(old), "the old value is released");
    assert_eq!(redactor.redact(old), old);
    assert!(redactor.contains_secret(new));
    assert_eq!(redactor.redact(new), REDACTED);
    assert_eq!(redactor.health().registered, 1);

    // Another item holding the same value keeps it masked.
    let other = context("two");
    vault
        .register_chosen(&other, &Secret::new(new))
        .expect("shared");
    vault
        .register_chosen(&cx, &Secret::new("third-planted-value-2"))
        .expect("third");
    assert!(
        redactor.contains_secret(new),
        "still held by the other item"
    );
    vault.release_chosen(&other);
    assert!(!redactor.contains_secret(new));
}

#[tokio::test]
async fn opening_a_replacement_releases_the_value_it_replaces() {
    let vault = vault();
    let cx = context("one");
    let redactor = vault.redactor();
    for (n, value) in ["first-planted-secret-1", "second-planted-secret-2"]
        .into_iter()
        .enumerate()
    {
        let sealed = vault.seal(&cx, value.as_bytes()).await.expect("seal");
        let opened = vault.open_secret(&cx, &sealed).await.expect("open");
        assert_eq!(opened.expose(), value);
        assert_eq!(redactor.contains_secret("first-planted-secret-1"), n == 0);
    }
    assert!(redactor.contains_secret("second-planted-secret-2"));
}

#[test]
fn a_slot_holds_one_value_and_a_permanent_registration_is_never_released() {
    let redactor = Redactor::new();
    redactor.register_in("a", &Secret::new("planted-first-0001"));
    redactor.register_in("a", &Secret::new("planted-second-0002"));
    assert!(!redactor.contains_secret("planted-first-0001"));
    assert!(redactor.contains_secret("planted-second-0002"));
    // Its hexadecimal form went with it.
    let hex: String = "planted-first-0001"
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(redactor.redact(&hex), hex);

    // A value registered with no slot stays, whatever a slot does with it.
    redactor.register(&Secret::new("planted-always-0003"));
    redactor.register_in("a", &Secret::new("planted-always-0003"));
    redactor.register_in("a", &Secret::new("planted-other-00004"));
    assert!(redactor.contains_secret("planted-always-0003"));
    redactor.release("a");
    redactor.release("never-used");
    assert!(!redactor.contains_secret("planted-other-00004"));
    assert!(redactor.contains_secret("planted-always-0003"));
    assert_eq!(redactor.health().registered, 1);
}
