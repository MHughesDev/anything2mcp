//! What the product's own registration of the pack offers (decision 0201).
//!
//! This test binary starts no test broker, so nothing in its process opens
//! the stand's protocol to the ordinary registration: what it registers is
//! what the shipped program registers.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{Environment, Epoch, RunId};
use a2m_ledger::Fence;
use a2m_logs::LogSettings;
use a2m_pack_stream::codes;
use a2m_provider::{
    CheckedPaths, Deadline, Destinations, DriverHealth, HostServices, Posture, PrepareContext,
    ProviderSettings, RegistryBuilder, Roots, Sent, Supervisor, SupervisorLimits, SurfaceRef,
};
use common::*;

#[tokio::test]
async fn the_ordinary_registration_offers_only_the_real_protocol_and_refuses_the_stands() {
    let folder = Folder::new("shipped");
    let logs = folder.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let host = HostServices {
        supervisor: Supervisor::new(SupervisorLimits::default()),
        roots: Roots::none(),
        redact: Arc::new(Planted),
        logs_and_traces: a2m_logs::start(
            &LogSettings::default(),
            Arc::new(a2m_core::NoSecrets),
            &logs,
        )
        .unwrap(),
    };
    let mut builder = RegistryBuilder::new(ProviderSettings::default(), host);
    a2m_pack_stream::register(&mut builder);
    let (shipped, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(a2m_pack_stream::PROTOCOLS, ["nats"]);

    // The example target names the stand's protocol: the compiler of the
    // ordinary registration refuses its surface.
    let refused = compile(&shipped, &fixture_root()).unwrap_err().to_string();
    assert!(
        refused.contains(
            "target.yaml:11:15: error kernel.unknown_value: `example-broker/1` is not one of the words this field takes. Fix: Use one of: nats."
        ),
        "{refused}"
    );
    // The same target compiles where a test registered the stand's
    // protocol, and names it.
    let tests = registry(&folder);
    let compiled_target = compiled_target(&tests);
    let surface = SurfaceRef {
        target: compiled_target.header.target.clone(),
        environment: Environment::new("test").unwrap(),
        surface: {
            let mut surface = compiled_target.surfaces[0].clone();
            surface.address = "127.0.0.1:9".to_owned();
            surface
        },
    };
    assert!(
        serde_json::to_string(&surface.surface.payload)
            .unwrap()
            .contains(a2m_pack_stream::TEST_BROKER_PROTOCOL)
    );

    // A compiled target made elsewhere that names it is refused by the driver
    // of the ordinary registration too, before anything is sent.
    let handle = shipped.driver(&mode()).unwrap();
    assert_eq!(
        handle.health(&surface),
        DriverHealth::Unavailable(codes::PROTOCOL_UNKNOWN)
    );
    let supervisor = handle.supervisor(Vec::new());
    let destinations = Destinations::new([surface.surface.address.clone()]);
    let fence = Fence {
        run: RunId::from_bytes([7; 16]),
        epoch: Epoch::new(1),
    };
    let cx = PrepareContext {
        run: fence.run,
        epoch: fence.epoch,
        surface: &surface,
        compiled_target: &compiled_target,
        posture: Posture::Live,
        deadline: Deadline::after(Duration::from_secs(20)),
        redact: &Planted,
        destinations: &destinations,
        paths: &CheckedPaths::none(),
        supervisor: &supervisor,
    };
    let failure = handle.prepare(&cx, None).await.unwrap_err();
    assert_eq!(failure.failure.code(), &codes::PROTOCOL_UNKNOWN);
    assert_eq!(failure.sent, Sent::NotSent);
    // The driver of a test's registration speaks it.
    assert_eq!(
        tests.driver(&mode()).unwrap().health(&surface),
        DriverHealth::Ready
    );
}
