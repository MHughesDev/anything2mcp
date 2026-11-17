//! The supervisor, proven against the command-line test stand: a run to
//! completion with captured output, a time limit that ends the whole
//! group, an output limit, an environment that holds only what was listed,
//! a scope that starts only what is declared, and no process left behind.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::ffi::OsString;
use std::time::Duration;

use a2m_core::{Digest, FailureClass};
use a2m_provider::{
    EnvAllowlist, Exit, LaunchSpec, OWNER_MARK_VARIABLE, OneShot, ProgramRef, ProviderSettings,
    Ran, RegistryBuilder, Roots, SubsystemKind, Supervisor, SupervisorLimits, codes,
};
use common::{Folder, TestPack, host, manifest};
use example_cli::faults::{Fault, FaultKind, Stream};
use example_cli::harness::Stand;

/// Long enough for a loaded machine to end a group; the tests wait on the
/// condition and not for this long.
const WITHIN: Duration = Duration::from_secs(20);

fn supervisor() -> Supervisor {
    Supervisor::new(SupervisorLimits::default())
}

fn spec(stand: &Stand, args: &[&str]) -> LaunchSpec {
    LaunchSpec {
        program: ProgramRef {
            path: stand.program().to_path_buf(),
            digest: None,
        },
        args: args.iter().map(OsString::from).collect(),
        env: EnvAllowlist {
            pass: Vec::new(),
            set: vec![(
                "EXAMPLE_CLI_HOME".to_owned(),
                stand.home().as_os_str().to_owned(),
            )],
        },
        cwd: None,
        kind: SubsystemKind::new("command").unwrap(),
        place: None,
    }
}

fn one_shot(seconds: u64) -> OneShot {
    OneShot {
        max_output_bytes: 64 * 1024,
        time_limit: Duration::from_secs(seconds),
        stdin: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_program_runs_to_its_end_with_its_output_captured_and_only_the_environment_listed() {
    let stand = Stand::start().unwrap();
    let supervisor = supervisor();
    let finished = supervisor
        .run(
            spec(&stand, &["item", "show", "SK-1001", "--format", "json"]),
            one_shot(60),
        )
        .await
        .unwrap();
    assert_eq!(finished.exit, Exit::Code(0));
    let stdout = String::from_utf8(finished.stdout.clone()).unwrap();
    assert!(stdout.contains("SK-1001"), "{stdout}");
    assert!(!finished.truncated());
    assert!(!finished.verified, "no digest was declared");
    assert!(finished.cleanup.complete);
    assert!(supervisor.list().is_empty());

    // The stand recorded what the program was started with.
    let started = stand.control().invocations().unwrap().pop().unwrap();
    assert_eq!(
        started.args,
        ["item", "show", "SK-1001", "--format", "json"]
    );
    assert_eq!(started.exit, Some(0));
    assert!(started.env.contains_key("EXAMPLE_CLI_HOME"));
    assert_eq!(
        started.env.get(OWNER_MARK_VARIABLE).map(String::len),
        Some(32)
    );
    assert!(
        std::env::var_os("PATH").is_some(),
        "the test itself has a search path"
    );
    assert!(
        !started
            .env
            .keys()
            .any(|name| name.eq_ignore_ascii_case("PATH")),
        "the engine's environment reached the program: {:?}",
        started.env.keys().collect::<Vec<_>>()
    );

    // A status the program chose is given back, with what it wrote.
    let refused = supervisor
        .run(spec(&stand, &["item", "show", "SK-9999"]), one_shot(60))
        .await
        .unwrap();
    assert_eq!(refused.exit, Exit::Code(3));
    assert!(!refused.stderr.is_empty());

    // What is written to standard input arrives, and the input is closed.
    let mut answered = one_shot(60);
    answered.stdin = Some(b"y\n".to_vec());
    let set = supervisor
        .run(
            spec(&stand, &["item", "set", "SK-1001", "--quantity=12"]),
            answered,
        )
        .await
        .unwrap();
    assert_eq!(set.exit, Exit::Code(0));
    assert_eq!(stand.control().write_count("SK-1001").unwrap(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_time_limit_ends_the_program_and_everything_it_started() {
    let stand = Stand::start().unwrap();
    let supervisor = supervisor();
    stand
        .control()
        .set_fault(&Fault::once(FaultKind::SpawnChild {
            lifetime_ms: 600_000,
        }))
        .unwrap();
    // A report that would take hours, with a child that would outlive it.
    let failure = supervisor
        .run(
            spec(
                &stand,
                &["report", "stock", "--rows", "100000", "--pause-ms", "200"],
            ),
            one_shot(2),
        )
        .await
        .unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code(), failure.ran),
        (FailureClass::Transient, &codes::TIMEOUT, Ran::Unknown)
    );
    stand.control().wait_until_no_children(WITHIN).unwrap();
    assert!(stand.control().living_children().unwrap().is_empty());
    let started = stand.control().invocations().unwrap().pop().unwrap();
    assert_eq!(
        started.exit, None,
        "the program was ended and did not exit by itself"
    );
    assert!(supervisor.list().is_empty());
    assert!(supervisor.leftovers().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn no_process_is_left_after_a_program_that_exits_and_leaves_a_child() {
    let stand = Stand::start().unwrap();
    let supervisor = supervisor();
    stand
        .control()
        .set_fault(&Fault::once(FaultKind::SpawnChild {
            lifetime_ms: 600_000,
        }))
        .unwrap();
    let finished = supervisor
        .run(spec(&stand, &["item", "list"]), one_shot(60))
        .await
        .unwrap();
    assert_eq!(finished.exit, Exit::Code(0));
    assert!(finished.cleanup.complete, "{:?}", finished.cleanup);
    stand.control().wait_until_no_children(WITHIN).unwrap();
    assert!(stand.control().living_children().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn output_beyond_the_bound_is_counted_and_dropped() {
    let stand = Stand::start().unwrap();
    let supervisor = supervisor();
    stand
        .control()
        .set_fault(&Fault::once(FaultKind::Flood {
            stream: Stream::Stdout,
            bytes: 300_000,
        }))
        .unwrap();
    let mut link = one_shot(60);
    link.max_output_bytes = 1000;
    let finished = supervisor
        .run(spec(&stand, &["item", "list"]), link)
        .await
        .unwrap();
    assert_eq!(finished.exit, Exit::Code(0));
    assert_eq!(finished.stdout.len(), 1000);
    assert!(
        finished.stdout_bytes >= 300_000,
        "{}",
        finished.stdout_bytes
    );
    assert!(finished.truncated());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_program_is_identified_before_it_starts() {
    let stand = Stand::start().unwrap();
    let supervisor = supervisor();
    // Not there.
    let mut missing = spec(&stand, &["version"]);
    missing.program.path = stand.root().join("no-such-program");
    let failure = supervisor.run(missing, one_shot(60)).await.unwrap_err();
    assert_eq!(
        (failure.failure.code(), failure.ran),
        (&codes::PROGRAM_NOT_FOUND, Ran::NotStarted)
    );
    // A name to search for is never started.
    let mut bare = spec(&stand, &["version"]);
    bare.program.path = "example-cli".into();
    let failure = supervisor.run(bare, one_shot(60)).await.unwrap_err();
    assert_eq!(failure.failure.code(), &codes::PROGRAM_NOT_FOUND);
    // Another digest than the one declared: refused, and nothing started.
    let before = stand.control().invocations().unwrap().len();
    let mut other = spec(&stand, &["version"]);
    other.program.digest = Some(Digest::of(b"another program"));
    let failure = supervisor.run(other, one_shot(60)).await.unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code(), failure.ran),
        (
            FailureClass::Policy,
            &codes::EFFECT_NOT_DECLARED,
            Ran::NotStarted
        )
    );
    assert_eq!(stand.control().invocations().unwrap().len(), before);
    // The declared digest: started, and marked verified.
    let digest = Digest::of(&std::fs::read(stand.program()).unwrap());
    let mut declared = spec(&stand, &["version"]);
    declared.program.digest = Some(digest);
    let finished = supervisor.run(declared, one_shot(60)).await.unwrap();
    assert!(finished.verified);
    assert_eq!(finished.program_digest, digest);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pack_starts_only_the_programs_its_manifest_declares() {
    let stand = Stand::start().unwrap();
    let folder = Folder::new("scoped");
    // A TOML literal string, so that a path of any operating system is taken as written.
    let declares = manifest("alpha", "request", "").replace(
        "[effects]",
        &format!(
            "[effects]\nprocesses = [{{ path = '{}' }}]",
            stand.program().display()
        ),
    );
    let pack = TestPack::with_manifest("alpha", declares);
    let mut builder =
        RegistryBuilder::new(ProviderSettings::default(), host(&folder, Roots::none()));
    builder.add_pack(&pack);
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    let scoped = pack
        .context
        .lock()
        .unwrap()
        .clone()
        .unwrap()
        .supervisor()
        .clone();

    let finished = scoped
        .run(spec(&stand, &["version"]), one_shot(60))
        .await
        .unwrap();
    assert_eq!(finished.exit, Exit::Code(0));

    // Any other program: refused before anything starts. The test's own
    // program stands for one the manifest does not name.
    let mut undeclared = spec(&stand, &["version"]);
    undeclared.program.path = std::env::current_exe().unwrap();
    let failure = scoped
        .run(undeclared.clone(), one_shot(60))
        .await
        .unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code(), failure.ran),
        (
            FailureClass::Policy,
            &codes::EFFECT_NOT_DECLARED,
            Ran::NotStarted
        )
    );
    // The handle's supervisor adds a published plan's programs only where
    // the manifest says `plan_programs`, and this one does not.
    let handle = registry.driver(&common::mode()).unwrap();
    let with_plan = handle.supervisor(vec![undeclared.program.clone()]);
    let failure = with_plan.run(undeclared, one_shot(60)).await.unwrap_err();
    assert_eq!(failure.failure.code(), &codes::EFFECT_NOT_DECLARED);
}

#[tokio::test(flavor = "multi_thread")]
async fn counts_are_kept_and_nothing_starts_after_shutdown() {
    let stand = Stand::start().unwrap();
    let none = Supervisor::new(SupervisorLimits {
        per_kind: 0,
        kinds: std::collections::BTreeMap::new(),
    });
    let failure = none
        .run(spec(&stand, &["version"]), one_shot(60))
        .await
        .unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code(), failure.ran),
        (FailureClass::Busy, &codes::SUBSYSTEM_LIMIT, Ran::NotStarted)
    );

    let supervisor = supervisor();
    assert!(supervisor.shutdown().await.complete);
    assert!(
        supervisor.shutdown().await.complete,
        "a second caller joins the same cleanup"
    );
    let failure = supervisor
        .run(spec(&stand, &["version"]), one_shot(60))
        .await
        .unwrap_err();
    assert_eq!(failure.failure.code(), &codes::SHUTTING_DOWN);
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_ends_a_program_that_is_running() {
    let stand = Stand::start().unwrap();
    let supervisor = supervisor();
    stand
        .control()
        .set_fault(&Fault::once(FaultKind::SpawnChild {
            lifetime_ms: 600_000,
        }))
        .unwrap();
    let running = {
        let supervisor = supervisor.clone();
        let spec = spec(
            &stand,
            &["report", "stock", "--rows", "100000", "--pause-ms", "200"],
        );
        tokio::spawn(async move { supervisor.run(spec, one_shot(3600)).await })
    };
    // Wait until the stand says the program has started its child.
    let started = tokio::time::Instant::now();
    while stand.control().living_children().unwrap().is_empty() {
        assert!(started.elapsed() < WITHIN, "the program never started");
        tokio::task::yield_now().await;
    }
    assert_eq!(supervisor.list().len(), 1);
    assert!(supervisor.shutdown().await.complete);
    let finished = running.await.unwrap().unwrap();
    assert_eq!(finished.exit, Exit::Killed);
    stand.control().wait_until_no_children(WITHIN).unwrap();
    assert!(supervisor.list().is_empty());
}

/// A foreign link: the child reads on descriptor 3 and writes on
/// descriptor 4. The system's shell stands for a program with a protocol of
/// its own; it copies what it reads to what it writes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_foreign_link_carries_bytes_both_ways_and_stop_ends_the_group() {
    use a2m_core::SubsystemState;
    use a2m_provider::ForeignPipe;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let supervisor = supervisor();
    let spec = LaunchSpec {
        program: ProgramRef {
            path: "/bin/sh".into(),
            digest: None,
        },
        args: vec!["-c".into(), "/bin/cat <&3 >&4".into()],
        env: EnvAllowlist::default(),
        cwd: None,
        kind: SubsystemKind::new("browser").unwrap(),
        place: Some("profile-one".to_owned()),
    };
    let pipe = ForeignPipe {
        child_reads_fd: 3,
        child_writes_fd: 4,
        windows_argument: "--io-pipes={read},{write}".to_owned(),
    };
    let mut link = supervisor
        .start_foreign(spec.clone(), pipe.clone())
        .await
        .unwrap();
    link.to_child.write_all(b"ping\n").await.unwrap();
    let mut reply = [0_u8; 5];
    link.from_child.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"ping\n");

    let listed = supervisor.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].link, listed[0].state),
        ("foreign", SubsystemState::Busy)
    );
    // The place is held: a second start for it is refused.
    let failure = supervisor.start_foreign(spec, pipe).await.unwrap_err();
    assert_eq!(failure.code(), &codes::SUBSYSTEM_LIMIT);

    assert!(supervisor.stop(link.id).await.complete);
    assert!(supervisor.list().is_empty());
    // The end of the child is the end of the pipe.
    let mut rest = Vec::new();
    link.from_child.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty());
}
