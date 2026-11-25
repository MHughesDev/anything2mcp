//! The neutral providers that hold a credential: what each places into a
//! driver's client, and the planted-secret test run against every one.

use std::sync::Arc;

use a2m_core::Redact;

use super::support::{
    STORED_HELD, World, account_key, assert_clean, block, logged, placed, reference, request,
};
use crate::encode::base64;
use crate::{AuthEntry, ProviderName, ProviderSet, Wait, codes, providers};

/// One provider's entry, the credentials it is stored, and what a driver's
/// client must be given.
struct Case {
    provider: &'static str,
    settings: &'static [(&'static str, &'static str)],
    stored: &'static [(&'static str, &'static str)],
    places: Vec<(String, String)>,
}

fn cases() -> Vec<Case> {
    let pair = |place: &str, value: &str| (place.to_owned(), value.to_owned());
    vec![
        Case {
            provider: "api_key",
            settings: &[("header", "X-Api-Key"), ("prefix", "Key ")],
            stored: &[("key", "PLANTED-api-key-0001")],
            places: vec![pair("header X-Api-Key", "Key PLANTED-api-key-0001")],
        },
        Case {
            provider: "bearer_static",
            settings: &[],
            stored: &[("token", "PLANTED-bearer-0002")],
            places: vec![pair("header Authorization", "Bearer PLANTED-bearer-0002")],
        },
        Case {
            provider: "http_basic",
            settings: &[("user", "robot")],
            stored: &[("password", "PLANTED-basic-0003")],
            places: vec![pair(
                "header Authorization",
                &format!("Basic {}", base64(b"robot:PLANTED-basic-0003")),
            )],
        },
        Case {
            provider: "http_digest",
            settings: &[("user", "robot")],
            stored: &[("password", "PLANTED-digest-0004")],
            places: vec![pair("challenge digest robot", "PLANTED-digest-0004")],
        },
        Case {
            provider: "windows_integrated",
            settings: &[("user", "EXAMPLE\\robot"), ("scheme", "ntlm")],
            stored: &[("password", "PLANTED-windows-0005")],
            places: vec![pair(
                "challenge ntlm EXAMPLE\\robot",
                "PLANTED-windows-0005",
            )],
        },
        Case {
            provider: "client_certificate",
            settings: &[],
            stored: &[
                (
                    "certificate",
                    "-----BEGIN CERTIFICATE-----PLANTED-cert-0006",
                ),
                ("key", "PLANTED-private-key-0007"),
            ],
            places: vec![pair(
                "tls -----BEGIN CERTIFICATE-----PLANTED-cert-0006",
                "PLANTED-private-key-0007",
            )],
        },
    ]
}

fn entry_of(case: &Case) -> AuthEntry {
    let mut entry = AuthEntry::new(ProviderName::new(case.provider).unwrap());
    for (name, value) in case.settings {
        entry = entry.setting(name, value);
    }
    for (role, _) in case.stored {
        entry = entry.credential(role, reference(&format!("{}/{role}", case.provider)));
    }
    entry.published()
}

#[test]
fn every_neutral_provider_is_built_and_named_as_the_documents_name_it() {
    let names: Vec<String> = ProviderSet::neutral()
        .names()
        .iter()
        .map(|name| name.as_str().to_owned())
        .collect();
    assert_eq!(names, providers::NEUTRAL);
    for name in providers::NEUTRAL {
        assert!(ProviderName::new(name).is_ok(), "{name}");
    }
}

#[test]
fn a_held_credential_reaches_only_the_drivers_client_and_appears_nowhere_else() {
    let world = block(World::new(STORED_HELD));
    let mut texts = Vec::new();
    let mut planted = vec!["PLANTED".to_owned()];

    logged(&world.logs_and_traces, async {
        for case in cases() {
            let key = account_key(case.provider);
            let entry = Arc::new(entry_of(&case));

            // Before anything is stored: the failure names the reference
            // and nothing else, and nothing is tried twice.
            let missing = world
                .keeper
                .lease(request(&key, &entry, Wait::DEFAULT))
                .await
                .unwrap_err();
            assert_eq!(
                missing.code(),
                &codes::CREDENTIAL_MISSING,
                "{}",
                case.provider
            );
            texts.push(format!("{missing:?} {missing}"));

            for (role, value) in case.stored {
                world
                    .store_credential(&key, &format!("{}/{role}", case.provider), value)
                    .await;
            }
            let lease = world
                .keeper
                .lease(request(&key, &entry, Wait::DEFAULT))
                .await
                .unwrap();

            // The driver's client is given exactly the credential.
            let given = placed(&lease);
            assert_eq!(given, case.places, "{}", case.provider);

            // Every secret that was opened or placed is registered with the
            // redactor, in the form it was placed in.
            for (_, value) in case.stored.iter().copied() {
                assert!(world.is_registered(value), "{}", case.provider);
            }
            for (_, value) in &given {
                assert!(world.is_registered(value), "{}", case.provider);
                assert_eq!(
                    world.vault.redactor().redact(value),
                    "[redacted]",
                    "{}",
                    case.provider
                );
                planted.push(value.clone());
            }

            // Nothing that can be printed or sent holds one.
            texts.push(format!("{lease:?} {:?}", lease.grant()));
            texts.push(serde_json::to_string(lease.grant()).unwrap());
            texts.push(format!(
                "{:?}",
                world.keeper.status(&key.workspace, None).await.unwrap()
            ));
            texts.push(format!(
                "{:?}",
                world.keeper.timeline(lease.session()).await.unwrap()
            ));
            let sealed = world.store.saved(lease.session()).await.unwrap().unwrap();
            texts.push(format!("{sealed:?}"));
            texts.push(String::from_utf8_lossy(&sealed.to_bytes()).into_owned());
        }
        texts.push(format!("{:?} {:?}", world.keeper, world.worker));
    });

    let lines = world.logs_and_traces.log_lines();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("session state changed")),
        "the keeper's log lines reach the test: {lines:?}"
    );
    texts.extend(lines);
    let planted: Vec<&str> = planted.iter().map(String::as_str).collect();
    assert_clean(&texts, &planted);
}

#[test]
fn an_entry_a_provider_cannot_use_is_refused_before_anything_is_opened() {
    let world = block(World::new(STORED_HELD));
    let set = ProviderSet::neutral();
    let refused = |entry: AuthEntry| {
        let provider = set.get(&entry.provider).unwrap();
        let failure = provider.check(&entry).unwrap_err();
        assert_eq!(failure.code(), &codes::AUTH_SETTINGS_INVALID, "{failure}");
        failure.to_string()
    };
    for case in cases() {
        let good = entry_of(&case);
        set.get(&good.provider).unwrap().check(&good).unwrap();
        // A setting nobody knows, and a missing credential.
        refused(good.clone().setting("surprise", "x"));
        let mut bare = good.clone();
        bare.credentials.clear();
        refused(bare);
    }
    let api_key = entry_of(&cases()[0]);
    refused(api_key.clone().setting("header", "X-Key: injected"));
    refused(api_key.setting("prefix", "Key\r\nX-Other: 1"));
    let basic = entry_of(&cases()[2]);
    refused(basic.setting("user", "ro:bot"));
    let windows = entry_of(&cases()[4]);
    refused(windows.setting("scheme", "kerberos_only"));

    // A provider nobody registered is a failure of its own, by name.
    let unknown = Arc::new(AuthEntry::new(ProviderName::new("carrier_pigeon").unwrap()));
    let failure = logged(&world.logs_and_traces, async {
        world
            .keeper
            .lease(request(&account_key("robot"), &unknown, Wait::NoWait))
            .await
            .unwrap_err()
    });
    assert_eq!(failure.code(), &codes::AUTH_PROVIDER_UNKNOWN);
}

/// The source check: a provider's `sign_in`, `refresh`, and `sign_out` are
/// called in the worker and nowhere else in this crate, and `inject` and
/// `detect_expiry` only by a lease. The layering keeps every other crate
/// from holding a provider to call.
#[test]
fn a_provider_is_called_only_by_the_keepers_two_halves() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut seen = Vec::new();
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        for call in [
            "provider.sign_in(",
            "provider.refresh(",
            "provider.sign_out(",
            "provider.is_signed_in(",
            "provider.inject(",
            "provider.detect_expiry(",
        ] {
            if text.contains(call) {
                seen.push((name.clone(), call));
            }
        }
    }
    seen.sort();
    assert_eq!(
        seen,
        [
            ("lease.rs".to_owned(), "provider.detect_expiry("),
            ("lease.rs".to_owned(), "provider.inject("),
            ("work.rs".to_owned(), "provider.refresh("),
            ("work.rs".to_owned(), "provider.sign_in("),
            ("work.rs".to_owned(), "provider.sign_out("),
        ]
    );
}
