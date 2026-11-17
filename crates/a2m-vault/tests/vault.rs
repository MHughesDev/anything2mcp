//! The vault's invariants: what is sealed opens only under the right keys
//! and the right context, nothing leaves in the clear, and everything
//! survives a restart.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use a2m_core::{
    Digest, DigestPurpose, Failure, FailureClass, KeyedDigest, REDACTED, Redact, Secret,
    WorkspaceId,
};
use a2m_vault::{
    BoxFuture, Generation, ItemId, KeyId, KeyMaterial, KeyRecord, KeyRecords, KeyStore, Layer,
    MemoryKeyRecords, MemoryKeyStore, Opened, OsRandom, OwnerRef, RandomSource, SealContext,
    Sealed, SealedKind, StoreHealth, StoreStrength, Vault, Wrapped, codes,
};

const PLAIN: &[u8] = b"cookie=tok_3x4mpl3-0f-the-example-target";

fn workspace(n: u8) -> WorkspaceId {
    WorkspaceId::from_bytes([n; 16])
}

fn person(name: &str) -> OwnerRef {
    OwnerRef::person(name).unwrap_or_else(|failure| panic!("{failure}"))
}

fn item(name: &str) -> ItemId {
    ItemId::new(name).unwrap_or_else(|failure| panic!("{failure}"))
}

fn context() -> SealContext {
    SealContext {
        workspace: workspace(1),
        owner: person("ana"),
        kind: SealedKind::SessionState,
        item: item("session-1"),
    }
}

/// What stands for the operating system's key store and the store's key
/// table: the two things that outlive the process.
fn durable() -> (Arc<MemoryKeyStore>, Arc<MemoryKeyRecords>) {
    (
        Arc::new(MemoryKeyStore::new()),
        Arc::new(MemoryKeyRecords::new()),
    )
}

fn vault(store: &Arc<MemoryKeyStore>, records: &Arc<MemoryKeyRecords>) -> Vault {
    Vault::new(Arc::clone(store) as _, Arc::clone(records) as _)
}

/// A random source that hands out the operating system's bytes and keeps a
/// copy of every key-sized run, so a test can look for them afterwards.
#[derive(Default)]
struct Recording(Mutex<Vec<Vec<u8>>>);

impl RandomSource for Recording {
    fn fill(&self, out: &mut [u8]) -> Result<(), Failure> {
        OsRandom.fill(out)?;
        if out.len() == KeyMaterial::LENGTH {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(out.to_vec());
        }
        Ok(())
    }
}

/// A random source that is the same on every run.
#[derive(Default)]
struct Counting(AtomicU64);

impl RandomSource for Counting {
    fn fill(&self, out: &mut [u8]) -> Result<(), Failure> {
        for byte in out {
            *byte = self.0.fetch_add(1, Ordering::Relaxed).to_le_bytes()[0];
        }
        Ok(())
    }
}

fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[tokio::test]
async fn a_sealed_item_opens_under_its_own_context() {
    let (store, records) = durable();
    let vault = vault(&store, &records);
    assert_eq!(vault.health().await.store_health, StoreHealth::EntryMissing);

    let sealed = vault.seal(&context(), PLAIN).await.unwrap();
    assert_eq!(
        vault.open(&context(), &sealed).await.unwrap().expose(),
        PLAIN
    );
    let empty = vault.seal(&context(), b"").await.unwrap();
    assert!(vault.open(&context(), &empty).await.unwrap().is_empty());

    let health = vault.health().await;
    assert_eq!(health.store_health, StoreHealth::Ok);
    assert_eq!(health.store.strength, StoreStrength::Memory);
    assert!(health.store.strength.is_weaker());

    // One workspace key under the root and one owner key under it; the data
    // key travels in the envelope.
    let layers: Vec<Layer> = records.all().iter().map(|record| record.layer).collect();
    assert_eq!(layers.len(), 2);
    assert!(layers.contains(&Layer::Workspace) && layers.contains(&Layer::Owner));
    assert_eq!(
        sealed.owner_key().id,
        KeyId::owner(&workspace(1), &context().owner)
    );
    assert_eq!(sealed.owner_key().generation, Generation::FIRST);
}

#[tokio::test]
async fn an_item_does_not_open_under_any_other_context() {
    let (store, records) = durable();
    let vault = vault(&store, &records);
    let sealed = vault.seal(&context(), PLAIN).await.unwrap();

    let others = [
        SealContext {
            workspace: workspace(2),
            ..context()
        },
        SealContext {
            owner: person("ben"),
            ..context()
        },
        SealContext {
            owner: OwnerRef::service("ana").unwrap(),
            ..context()
        },
        SealContext {
            kind: SealedKind::Credential,
            ..context()
        },
        SealContext {
            kind: SealedKind::Blob,
            ..context()
        },
        SealContext {
            item: item("session-2"),
            ..context()
        },
    ];
    let mut failures = Vec::new();
    for other in &others {
        // Whether or not the other context has keys of its own.
        for prepare in [false, true] {
            if prepare {
                vault.seal(other, b"theirs").await.unwrap();
            }
            let failure = vault.open(other, &sealed).await.unwrap_err();
            assert_eq!(failure.class(), FailureClass::Input, "{other:?}");
            assert_eq!(failure.code(), &codes::SEALED_ITEM_MISMATCH, "{other:?}");
            assert!(vault.open_secret(other, &sealed).await.is_err());
            failures.push(failure);
        }
    }
    // The failure is the same whatever the reason: it never says which.
    assert!(failures.iter().all(|failure| failure == &failures[0]));
    assert_eq!(
        vault.open(&context(), &sealed).await.unwrap().expose(),
        PLAIN
    );
}

#[tokio::test]
async fn a_sealed_item_with_any_bit_changed_does_not_open() {
    let (store, records) = durable();
    let vault = vault(&store, &records);
    let sealed = vault.seal(&context(), PLAIN).await.unwrap();
    let bytes = sealed.to_bytes();
    assert_eq!(Sealed::from_bytes(&bytes).unwrap(), sealed);

    for at in 0..bytes.len() {
        for bit in 0..8 {
            let mut changed = bytes.clone();
            changed[at] ^= 1 << bit;
            let failure = match Sealed::from_bytes(&changed) {
                Ok(changed) => vault.open(&context(), &changed).await.unwrap_err(),
                Err(failure) => failure,
            };
            assert_eq!(failure.class(), FailureClass::Input, "byte {at} bit {bit}");
            assert!(
                [
                    codes::SEALED_ITEM_MISMATCH,
                    codes::SEALED_FORMAT_UNSUPPORTED
                ]
                .contains(failure.code()),
                "byte {at} bit {bit}: {failure}"
            );
        }
    }
    // Cut short, or with bytes added, it does not open either.
    for length in 0..bytes.len() {
        let opened = match Sealed::from_bytes(&bytes[..length]) {
            Ok(cut) => vault.open(&context(), &cut).await.is_ok(),
            Err(_) => false,
        };
        assert!(!opened, "cut to {length}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    let longer = Sealed::from_bytes(&longer).unwrap();
    assert!(vault.open(&context(), &longer).await.is_err());
}

#[tokio::test]
async fn two_seals_of_the_same_value_differ() {
    let (store, records) = durable();
    let vault = vault(&store, &records);
    let first = vault.seal(&context(), PLAIN).await.unwrap().to_bytes();
    let second = vault.seal(&context(), PLAIN).await.unwrap().to_bytes();
    assert_ne!(first, second);
    // Past the header and the owner's key, nothing is shared: the wrapped
    // data key, the nonce, and the ciphertext are all new.
    let shared = first
        .iter()
        .zip(&second)
        .take_while(|(a, b)| a == b)
        .count();
    assert!(shared < 4 + 2 + 32 + 4 + 8, "{shared} bytes shared");
    assert_eq!(first.len(), second.len());
}

#[tokio::test]
async fn randomness_comes_only_from_the_source_given() {
    let seal = || async {
        let random: Arc<dyn RandomSource> = Arc::new(Counting::default());
        let store = Arc::new(MemoryKeyStore::with_random(Arc::clone(&random)));
        let records = Arc::new(MemoryKeyRecords::new());
        let vault = Vault::with_random(store, Arc::clone(&records) as _, random);
        let sealed = vault.seal(&context(), PLAIN).await.unwrap().to_bytes();
        (sealed, records.all())
    };
    assert_eq!(seal().await, seal().await);
}

#[tokio::test]
async fn no_key_and_no_plain_byte_is_in_anything_the_caller_keeps_or_sees() {
    let random = Arc::new(Recording::default());
    let store = Arc::new(MemoryKeyStore::with_random(Arc::clone(&random) as _));
    let records = Arc::new(MemoryKeyRecords::new());
    let vault = Vault::with_random(
        Arc::clone(&store) as _,
        Arc::clone(&records) as _,
        Arc::clone(&random) as _,
    );
    let sealed = vault.seal(&context(), PLAIN).await.unwrap();
    let digest = vault.keyed_digest().await.unwrap();
    let opened = vault.open(&context(), &sealed).await.unwrap();
    let mismatch = vault
        .open(
            &SealContext {
                kind: SealedKind::Blob,
                ..context()
            },
            &sealed,
        )
        .await
        .unwrap_err();
    store.remove_root();
    let missing = Vault::new(Arc::clone(&store) as _, Arc::clone(&records) as _)
        .open(&context(), &sealed)
        .await
        .unwrap_err();

    // The root, the workspace key, the owner key, the data key, the digest
    // key: every key made, in the order it was made.
    let keys = random.0.lock().unwrap().clone();
    assert_eq!(keys.len(), 5);

    // What a caller stores: the envelope and the key records.
    let mut kept = vec![sealed.to_bytes(), serde_json::to_vec(&sealed).unwrap()];
    for record in records.all() {
        kept.push(record.wrapped.as_bytes().to_vec());
        kept.push(serde_json::to_vec(&record.wrapped).unwrap());
    }
    // What a caller may print.
    let shown = format!(
        "{vault:?} {sealed:?} {opened:?} {digest:?} {:?} {mismatch:?} {mismatch} {missing:?} {missing} {:?}",
        records.all(),
        vault.health().await,
    );
    kept.push(shown.clone().into_bytes());

    for (index, bytes) in kept.iter().enumerate() {
        assert!(!holds(bytes, PLAIN), "plain bytes in item {index}");
        assert!(!holds(bytes, &PLAIN[..8]), "plain bytes in item {index}");
        for key in &keys {
            assert!(!holds(bytes, key), "key bytes in item {index}");
        }
    }
    // Printed, a key would be numbers or hexadecimal; neither is there.
    for key in &keys {
        let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
        let numbers = format!("{:?}", &key[..4]);
        assert!(!shown.contains(&hex[..16]), "{shown}");
        assert!(!shown.contains(numbers.trim_matches(['[', ']'])), "{shown}");
    }
    assert!(shown.contains("Opened(40 bytes)"), "{shown}");
}

#[tokio::test]
async fn key_material_and_plain_bytes_cannot_be_printed_copied_or_serialized() {
    let key = KeyMaterial::generate(&OsRandom).unwrap();
    assert_eq!(format!("{key:?}"), "KeyMaterial([redacted])");
    let wrapped = Wrapped::from_bytes(vec![0xab; 72]);
    assert_eq!(format!("{wrapped:?}"), "Wrapped(72 bytes)");

    // These fail to compile if either type gains a way to be serialized or
    // copied: a type that has the trait makes the call ambiguous.
    trait Probe<Marker> {
        fn check() {}
    }
    impl<T: ?Sized> Probe<()> for T {}
    struct Serializes;
    impl<T: ?Sized + serde::Serialize> Probe<Serializes> for T {}
    struct Clones;
    impl<T: Clone> Probe<Clones> for T {}
    struct Shows;
    impl<T: ?Sized + std::fmt::Display> Probe<Shows> for T {}

    <KeyMaterial as Probe<_>>::check();
    <Opened as Probe<_>>::check();
    <a2m_vault::VaultDigest as Probe<_>>::check();
}

#[tokio::test]
async fn a_secret_is_registered_before_it_is_returned() {
    let (store, records) = durable();
    let vault = vault(&store, &records);
    let value = "tok_3x4mpl3 w/ \"quotes\"";
    let cx = SealContext {
        kind: SealedKind::Credential,
        ..context()
    };
    let sealed = vault.seal(&cx, value.as_bytes()).await.unwrap();
    let redactor = vault.redactor();
    assert!(!redactor.contains_secret(value));

    let secret = vault.open_secret(&cx, &sealed).await.unwrap();
    assert_eq!(secret.expose(), value);
    let json = serde_json::to_string(value).unwrap();
    for text in [
        value.to_owned(),
        json,
        "tok_3x4mpl3%20w%2F%20%22quotes%22".to_owned(),
        "dG9rXzN4NG1wbDMgdy8gInF1b3RlcyI=".to_owned(),
    ] {
        assert!(redactor.contains_secret(&text), "{text}");
        assert!(redactor.redact(&text).contains(REDACTED), "{text}");
    }

    // A secret that arrived some other way is registered by hand.
    vault.register(&Secret::new("refresh-0f-the-example"));
    assert_eq!(
        redactor.redact("got refresh-0f-the-example"),
        format!("got {REDACTED}")
    );

    // Bytes that are not text are not a secret, and say no more than that.
    let binary = vault.seal(&cx, &[0xff, 0xfe, 0x00]).await.unwrap();
    let failure = vault.open_secret(&cx, &binary).await.unwrap_err();
    assert_eq!(failure.code(), &codes::SEALED_ITEM_MISMATCH);
}

#[tokio::test]
async fn everything_survives_a_restart_and_nothing_opens_without_the_root() {
    let (store, records) = durable();
    let other = SealContext {
        workspace: workspace(2),
        owner: OwnerRef::service("runner").unwrap(),
        kind: SealedKind::Blob,
        item: item("blob-1"),
    };
    let (kept, kept_other, name) = {
        let vault = vault(&store, &records);
        let sealed = vault.seal(&context(), PLAIN).await.unwrap();
        let sealed_other = vault.seal(&other, b"evidence").await.unwrap();
        let digest = vault.keyed_digest().await.unwrap();
        let name = digest.keyed_digest(&workspace(1), DigestPurpose::BlobName, PLAIN);
        (sealed.to_bytes(), sealed_other.to_bytes(), name)
        // Every object of the process is dropped here. What remains is what
        // a real installation keeps: the key store's entry, the key records,
        // and the sealed bytes.
    };

    let vault = vault(&store, &records);
    let sealed = Sealed::from_bytes(&kept).unwrap();
    let sealed_other = Sealed::from_bytes(&kept_other).unwrap();
    assert_eq!(
        vault.open(&context(), &sealed).await.unwrap().expose(),
        PLAIN
    );
    assert_eq!(
        vault.open(&other, &sealed_other).await.unwrap().expose(),
        b"evidence"
    );
    let digest = vault.keyed_digest().await.unwrap();
    assert_eq!(
        digest.keyed_digest(&workspace(1), DigestPurpose::BlobName, PLAIN),
        name
    );
    // New items are sealed under the same keys: no new record is made.
    let count = records.all().len();
    let again = vault.seal(&context(), PLAIN).await.unwrap();
    assert_eq!(again.owner_key(), sealed.owner_key());
    assert_eq!(records.all().len(), count);

    // The key store is reset. A vault already running keeps what it opened.
    store.remove_root();
    assert_eq!(
        vault.open(&context(), &sealed).await.unwrap().expose(),
        PLAIN
    );
    assert_eq!(vault.health().await.store_health, StoreHealth::EntryMissing);

    // After a restart nothing opens, and nothing new is sealed over it.
    drop(vault);
    let vault = self::vault(&store, &records);
    for (cx, sealed) in [(&context(), &sealed), (&other, &sealed_other)] {
        let failure = vault.open(cx, sealed).await.unwrap_err();
        assert_eq!(failure.class(), FailureClass::Busy);
        assert_eq!(failure.code(), &codes::KEY_STORE_ENTRY_MISSING);
        let failure = vault.seal(cx, PLAIN).await.unwrap_err();
        assert_eq!(failure.code(), &codes::KEY_STORE_ENTRY_MISSING);
    }
    let failure = vault.keyed_digest().await.err().unwrap();
    assert_eq!(failure.code(), &codes::KEY_STORE_ENTRY_MISSING);
}

#[tokio::test]
async fn a_keyed_digest_matches_equal_inputs_and_nothing_else() {
    let (store, records) = durable();
    let vault = vault(&store, &records);
    let digest = vault.keyed_digest().await.unwrap();
    let of = |workspace: u8, purpose, bytes: &[u8]| {
        digest.keyed_digest(&self::workspace(workspace), purpose, bytes)
    };
    let name = of(1, DigestPurpose::BlobName, PLAIN);
    assert_eq!(name, of(1, DigestPurpose::BlobName, PLAIN));
    assert_ne!(name, of(2, DigestPurpose::BlobName, PLAIN));
    assert_ne!(name, of(1, DigestPurpose::RestrictedValue, PLAIN));
    assert_ne!(name, of(1, DigestPurpose::BlobName, b"other"));
    // A guess cannot be confirmed by hashing it.
    assert_ne!(name, Digest::of(PLAIN));

    // Another installation has another key, so another name.
    let (other_store, other_records) = durable();
    let other = self::vault(&other_store, &other_records)
        .keyed_digest()
        .await
        .unwrap();
    assert_ne!(
        name,
        other.keyed_digest(&workspace(1), DigestPurpose::BlobName, PLAIN)
    );

    // The digest key is one record under the root, beside the workspace keys.
    let all = records.all();
    assert_eq!(all.len(), 1);
    assert_eq!((all[0].id, all[0].layer), (KeyId::digest(), Layer::Digest));
    assert_eq!(all[0].wrapped_by.id, KeyId::root());
}

/// Key records as a second engine sees them when another engine writes the
/// same key at the same moment: the first look finds nothing, and the insert
/// that follows finds the other engine's record already there.
struct Racing {
    records: Arc<MemoryKeyRecords>,
    lies_left: Mutex<u32>,
}

impl KeyRecords for Racing {
    fn get<'a>(
        &'a self,
        id: &'a KeyId,
        generation: Generation,
    ) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>> {
        self.records.get(id, generation)
    }

    fn newest<'a>(&'a self, id: &'a KeyId) -> BoxFuture<'a, Result<Option<KeyRecord>, Failure>> {
        let lie = {
            let mut left = self
                .lies_left
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let lie = *left > 0;
            *left = left.saturating_sub(1);
            lie
        };
        if lie {
            Box::pin(async { Ok(None) })
        } else {
            self.records.newest(id)
        }
    }

    fn insert(&self, record: KeyRecord) -> BoxFuture<'_, Result<(), Failure>> {
        self.records.insert(record)
    }

    fn replace<'a>(
        &'a self,
        record: KeyRecord,
        expected: &'a Wrapped,
    ) -> BoxFuture<'a, Result<(), Failure>> {
        self.records.replace(record, expected)
    }
}

#[tokio::test]
async fn two_engines_that_make_the_same_key_end_with_one() {
    let (store, records) = durable();
    let first = vault(&store, &records);
    let sealed = first.seal(&context(), PLAIN).await.unwrap();
    let before = records.all();

    let racing = Arc::new(Racing {
        records: Arc::clone(&records),
        lies_left: Mutex::new(2),
    });
    let second = Vault::new(Arc::clone(&store) as Arc<dyn KeyStore>, racing);
    let theirs = second.seal(&context(), b"second").await.unwrap();

    // The loser took the winner's keys: no record changed, and each engine
    // opens what the other sealed.
    assert_eq!(records.all(), before);
    assert_eq!(theirs.owner_key(), sealed.owner_key());
    assert_eq!(
        first.open(&context(), &theirs).await.unwrap().expose(),
        b"second"
    );
    assert_eq!(
        second.open(&context(), &sealed).await.unwrap().expose(),
        PLAIN
    );
}

#[tokio::test]
async fn a_key_record_moved_or_damaged_opens_nothing() {
    let (store, records) = durable();
    let vault = vault(&store, &records);
    let ben = SealContext {
        owner: person("ben"),
        ..context()
    };
    let sealed = vault.seal(&context(), PLAIN).await.unwrap();
    vault.seal(&ben, PLAIN).await.unwrap();

    // Someone who can write the store puts one owner's wrapped key in the
    // other owner's record, then a damaged one.
    let all = records.all();
    let record_of = |cx: &SealContext| {
        let id = KeyId::owner(&cx.workspace, &cx.owner);
        all.iter().find(|record| record.id == id).unwrap().clone()
    };
    let (anas, bens) = (record_of(&context()), record_of(&ben));
    let mut damaged = anas.wrapped.as_bytes().to_vec();
    damaged[30] ^= 1;
    for wrapped in [bens.wrapped.clone(), Wrapped::from_bytes(damaged)] {
        let tampered = Arc::new(MemoryKeyRecords::new());
        for record in &all {
            let mut record = record.clone();
            if record.id == anas.id {
                record.wrapped = wrapped.clone();
            }
            tampered.insert(record).await.unwrap();
        }
        let fresh = self::vault(&store, &tampered);
        let failure = fresh.open(&context(), &sealed).await.unwrap_err();
        assert_eq!(failure.code(), &codes::SEALED_ITEM_MISMATCH);
    }
}

#[tokio::test]
async fn a_sealed_item_reads_back_from_text_and_from_bytes() {
    let (store, records) = durable();
    let vault = vault(&store, &records);
    let sealed = vault.seal(&context(), PLAIN).await.unwrap();
    let json = serde_json::to_string(&sealed).unwrap();
    assert_eq!(json.len(), sealed.len() * 2 + 2);
    let back: Sealed = serde_json::from_str(&json).unwrap();
    assert_eq!(back, sealed);
    assert_eq!(vault.open(&context(), &back).await.unwrap().expose(), PLAIN);
    assert!(serde_json::from_str::<Sealed>("\"00ff\"").is_err());
    assert!(serde_json::from_str::<Sealed>("\"not hex\"").is_err());

    let unknown = {
        let mut bytes = sealed.to_bytes();
        bytes[4] = 2;
        Sealed::from_bytes(&bytes).unwrap_err()
    };
    assert_eq!(unknown.code(), &codes::SEALED_FORMAT_UNSUPPORTED);
    assert_eq!(unknown.class(), FailureClass::Input);
}

#[tokio::test]
async fn the_vault_and_its_futures_cross_threads() {
    fn crosses_threads<T: Send>(value: T) -> T {
        value
    }
    fn shared<T: Send + Sync>() {}
    shared::<Vault>();
    shared::<a2m_vault::Redactor>();
    shared::<a2m_vault::VaultDigest>();
    shared::<Sealed>();

    let (store, records) = durable();
    let vault = Arc::new(vault(&store, &records));
    let sealing = Arc::clone(&vault);
    let sealed = tokio::spawn(crosses_threads(async move {
        sealing.seal(&context(), PLAIN).await
    }))
    .await
    .unwrap_or_else(|error| panic!("{error}"))
    .unwrap();
    let opened = crosses_threads(vault.open(&context(), &sealed))
        .await
        .unwrap();
    assert_eq!(opened.expose(), PLAIN);
    drop(crosses_threads(vault.open_secret(&context(), &sealed)));
    drop(crosses_threads(vault.keyed_digest()));
    drop(crosses_threads(vault.health()));
}
