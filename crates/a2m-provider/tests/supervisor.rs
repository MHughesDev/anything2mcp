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
    // The handle's supervisor adds a published compiled target's programs
    // only where the manifest says `compiled_target_programs`, and this one
    // does not.
    let handle = registry.driver(&common::mode()).unwrap();
    let with_compiled_target = handle.supervisor(vec![undeclared.program.clone()]);
    let failure = with_compiled_target
        .run(undeclared, one_shot(60))
        .await
        .unwrap_err();
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

/// A group whose leader has exited is ended completely, whatever the leader
/// left. First a program that starts a child and exits at once: the leader
/// is gone and its child is left in the group, holding the pipe. Then a
/// program stopped in the very moment it ends by itself, the end of its pipe
/// seen and nothing more, as a browser that was asked to close is stopped:
/// on macOS such a leader is in the middle of its exit, so the group answers
/// that nothing in it can be signalled while the leader cannot be waited for
/// yet. That moment is not hit every time, so it is tried many times.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_group_whose_leader_has_exited_is_ended_completely_whatever_it_left() {
    use a2m_core::SubsystemState;
    use a2m_provider::ForeignPipe;
    use tokio::io::AsyncReadExt;

    let supervisor = supervisor();
    let shell = |script: &str| LaunchSpec {
        program: ProgramRef {
            path: "/bin/sh".into(),
            digest: None,
        },
        args: vec!["-c".into(), script.into()],
        env: EnvAllowlist::default(),
        cwd: None,
        kind: SubsystemKind::new("browser").unwrap(),
        place: None,
    };
    let pipe = ForeignPipe {
        child_reads_fd: 3,
        child_writes_fd: 4,
        windows_argument: "--io-pipes={read},{write}".to_owned(),
    };

    // The child inherits the pipe and would sleep for ten minutes.
    let mut link = supervisor
        .start_foreign(shell("/bin/sleep 600 & exit 0"), pipe.clone())
        .await
        .unwrap();
    let started = tokio::time::Instant::now();
    while supervisor.list()[0].state != SubsystemState::Exited {
        assert!(started.elapsed() < WITHIN, "the leader never exited");
        tokio::task::yield_now().await;
    }
    let cleanup = supervisor.stop(link.id).await;
    assert!(cleanup.complete, "{cleanup:?}");
    // The child held the pipe, so the end of the pipe is the end of the child.
    let mut rest = Vec::new();
    tokio::time::timeout(WITHIN, link.from_child.read_to_end(&mut rest))
        .await
        .expect("the child the leader left was not ended")
        .unwrap();

    for _ in 0..50 {
        let mut link = supervisor
            .start_foreign(shell("exit 0"), pipe.clone())
            .await
            .unwrap();
        let mut rest = Vec::new();
        link.from_child.read_to_end(&mut rest).await.unwrap();
        let cleanup = supervisor.stop(link.id).await;
        assert!(cleanup.complete, "{cleanup:?}");
    }
    assert!(supervisor.list().is_empty());
    assert!(supervisor.leftovers().is_empty());
}

/// A process that is not in the program's group holds the program's output
/// pipe open after the program has ended: here one that left the group, and
/// on a system that cannot make a pipe already marked close-on-exec it
/// could be a child started elsewhere in the same moment. The run must not
/// wait for the end of the pipe, must keep everything the program wrote,
/// and must not call its own cleanup incomplete.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn a_holder_of_the_output_outside_the_group_neither_delays_a_run_nor_spoils_its_verdict() {
    let setsid = std::path::Path::new("/usr/bin/setsid");
    if !setsid.is_file() {
        // Without the tool there is no way here to leave a group.
        return;
    }
    let supervisor = supervisor();
    // The shell starts, in a session of its own, a sleeper that keeps the
    // shell's standard output, prints the sleeper's process number, and
    // ends. Descriptor 3 carries the output past the command substitution.
    let script = "exec 3>&1; \
                  pid=$(/usr/bin/setsid /bin/sh -c 'echo $$; exec /bin/sleep 600 >&3 3>&-' &); \
                  echo \"holder $pid\"";
    let spec = LaunchSpec {
        program: ProgramRef {
            path: "/bin/sh".into(),
            digest: None,
        },
        args: vec!["-c".into(), script.into()],
        env: EnvAllowlist::default(),
        cwd: None,
        kind: SubsystemKind::new("command").unwrap(),
        place: None,
    };
    // An hour's limit: if the run waited on the holder, the test's own
    // limit would end it long before.
    let finished = tokio::time::timeout(WITHIN, supervisor.run(spec.clone(), one_shot(3600)))
        .await
        .expect("the run waited for a process that is not the program's")
        .unwrap();
    let stdout = String::from_utf8(finished.stdout.clone()).unwrap();
    let holder: i32 = stdout
        .trim()
        .strip_prefix("holder ")
        .and_then(|pid| pid.parse().ok())
        .unwrap_or_else(|| panic!("the program's output was not kept: {stdout:?}"));
    let holder = rustix::process::Pid::from_raw(holder).unwrap();
    // The holder is alive, outside the group, and still holds the pipe.
    let alive = rustix::process::test_kill_process(holder).is_ok();
    let verdict = (
        finished.exit,
        finished.cleanup.complete,
        supervisor.leftovers().is_empty(),
    );
    // A second run of the same kind is not blocked by the first.
    let again = tokio::time::timeout(WITHIN, supervisor.run(spec, one_shot(3600))).await;
    // End both holders before anything is asserted, so that none is left.
    let _ = rustix::process::kill_process(holder, rustix::process::Signal::KILL);
    if let Ok(Ok(second)) = &again {
        let second = String::from_utf8_lossy(&second.stdout);
        let second = second
            .trim()
            .strip_prefix("holder ")
            .and_then(|pid| pid.parse().ok())
            .and_then(rustix::process::Pid::from_raw);
        if let Some(pid) = second {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
        }
    }
    assert!(
        alive,
        "the holder was not alive, so the test proved nothing"
    );
    assert_eq!(verdict, (Exit::Code(0), true, true));
    assert!(again.expect("the second run waited").is_ok());
}

/// Has a copy of the stand's program, kept somewhere else, report to the
/// stand: the program looks for the stand beside itself unless it is told.
fn record_with(stand: &Stand, spec: &mut LaunchSpec) {
    spec.env.set.push((
        example_cli::CONTROL_ENV.to_owned(),
        stand
            .root()
            .join(example_cli::CONTROL_FOLDER)
            .into_os_string(),
    ));
}

/// The scoped supervisor of a pack whose manifest is `declares`.
fn scoped_for(folder: &Folder, declares: String) -> a2m_provider::ScopedSupervisor {
    let pack = TestPack::with_manifest("alpha", declares);
    let mut builder =
        RegistryBuilder::new(ProviderSettings::default(), host(folder, Roots::none()));
    builder.add_pack(&pack);
    let (_registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    pack.context
        .lock()
        .unwrap()
        .clone()
        .unwrap()
        .supervisor()
        .clone()
}

/// The audit's finding: a requirement's known digests were compared when
/// the registry was built and never again, so a program replaced at its
/// path afterwards was started all the same. They are compared at every
/// start, on the file that is started.
#[tokio::test(flavor = "multi_thread")]
async fn a_required_program_replaced_after_the_registry_was_built_is_not_started() {
    let stand = Stand::start().unwrap();
    let folder = Folder::new("required");
    // The program the pack requires: a copy of the stand's, at a place of
    // its own, with the digest a publisher would list.
    let name = stand.program().file_name().unwrap();
    let required = folder.path().join(name);
    std::fs::copy(stand.program(), &required).unwrap();
    let reviewed = std::fs::read(&required).unwrap();
    let digest = Digest::of(&reviewed);
    // TOML literal strings, so that a path of any operating system is taken as written.
    let declares = manifest(
        "alpha",
        "request",
        &format!(
            "\n[[requirements]]\nname = \"example-runtime\"\ndetect = {{ paths = ['{}'] }}\nknown_digests = [\"sha256:{digest}\"]\nmissing = \"Install it.\"\n",
            required.display()
        ),
    );
    let scoped = scoped_for(&folder, declares);
    let mut spec = spec(&stand, &["version"]);
    spec.program.path.clone_from(&required);
    record_with(&stand, &mut spec);

    let finished = scoped.run(spec.clone(), one_shot(60)).await.unwrap();
    assert_eq!(finished.exit, Exit::Code(0));
    assert!(finished.verified, "a known digest was declared and found");
    assert_eq!(finished.program_digest, digest);
    let before = stand.control().invocations().unwrap().len();

    // The program is replaced, as an update or an attacker would: a new
    // file takes the name. The registry is not built again.
    let mut other = reviewed.clone();
    other.extend_from_slice(b"\nnot the reviewed program\n");
    let incoming = folder.path().join("incoming");
    std::fs::write(&incoming, &other).unwrap();
    std::fs::rename(&incoming, &required).unwrap();
    let failure = scoped.run(spec.clone(), one_shot(60)).await.unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code(), failure.ran),
        (
            FailureClass::Policy,
            &codes::EFFECT_NOT_DECLARED,
            Ran::NotStarted
        )
    );
    // The stand is asked: nothing was started.
    assert_eq!(stand.control().invocations().unwrap().len(), before);

    // Changed in place, with the same length: refused too.
    let mut same_length = reviewed.clone();
    let last = same_length.len() - 1;
    same_length[last] ^= 0xff;
    std::fs::write(&required, &same_length).unwrap();
    let failure = scoped.run(spec.clone(), one_shot(60)).await.unwrap_err();
    assert_eq!(failure.failure.code(), &codes::EFFECT_NOT_DECLARED);
    assert_eq!(stand.control().invocations().unwrap().len(), before);

    // The reviewed program back at its place starts again.
    std::fs::remove_file(&required).unwrap();
    std::fs::copy(stand.program(), &required).unwrap();
    let finished = scoped.run(spec, one_shot(60)).await.unwrap();
    assert!(finished.verified);
    assert_eq!(stand.control().invocations().unwrap().len(), before + 1);
}

/// A declared path that is a link: the link is resolved at every start, the
/// file it leads to is the one digested, and repointing it to another
/// program is seen.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_link_repointed_between_two_starts_does_not_start_another_program() {
    let stand = Stand::start().unwrap();
    let folder = Folder::new("repointed");
    let name = stand.program().file_name().unwrap();
    let reviewed = folder.path().join(name);
    std::fs::copy(stand.program(), &reviewed).unwrap();
    let digest = Digest::of(&std::fs::read(&reviewed).unwrap());
    let other = folder.path().join("other-program");
    let mut bytes = std::fs::read(&reviewed).unwrap();
    bytes.extend_from_slice(b"\nanother program\n");
    std::fs::write(&other, bytes).unwrap();
    let link = folder.path().join("current");
    std::os::unix::fs::symlink(&reviewed, &link).unwrap();

    let supervisor = supervisor();
    let mut spec = spec(&stand, &["version"]);
    spec.program = ProgramRef {
        path: link.clone(),
        digest: Some(digest),
    };
    record_with(&stand, &mut spec);
    let finished = supervisor.run(spec.clone(), one_shot(60)).await.unwrap();
    assert!(finished.verified);
    let before = stand.control().invocations().unwrap().len();

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&other, &link).unwrap();
    let failure = supervisor.run(spec, one_shot(60)).await.unwrap_err();
    assert_eq!(
        (failure.failure.code(), failure.ran),
        (&codes::EFFECT_NOT_DECLARED, Ran::NotStarted)
    );
    assert_eq!(stand.control().invocations().unwrap().len(), before);
}

/// On Linux a program is started from the very handle it was digested
/// through. The system names a process after the last part of the path it
/// was started by, which for an open file is the handle's number; the
/// program itself still sees the name it was asked for by.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn on_linux_a_program_is_started_from_the_handle_it_was_digested_through() {
    if !std::path::Path::new("/proc/self/fd").is_dir() {
        // Without the process file system a program is started by its path.
        return;
    }
    let supervisor = supervisor();
    let spec = LaunchSpec {
        program: ProgramRef {
            path: "/bin/sh".into(),
            digest: None,
        },
        args: vec![
            "-c".into(),
            "read started_by < /proc/$$/comm; echo \"$started_by\"; echo \"$0\"".into(),
        ],
        env: EnvAllowlist::default(),
        cwd: None,
        kind: SubsystemKind::new("command").unwrap(),
        place: None,
    };
    let finished = supervisor.run(spec, one_shot(60)).await.unwrap();
    assert_eq!(finished.exit, Exit::Code(0), "{finished:?}");
    let stdout = String::from_utf8(finished.stdout).unwrap();
    let mut lines = stdout.lines();
    let started_by = lines.next().unwrap();
    assert!(
        started_by.parse::<u32>().is_ok(),
        "the program was started by a path and not from its handle: {started_by:?}"
    );
    assert_eq!(lines.next(), Some("/bin/sh"));
}

/// A file that starts with `#!` is handed to an interpreter by name, so it
/// cannot be started from a handle. It is digested like any program, and
/// started by its path after a last look that the path still leads to the
/// file that was digested.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_script_is_digested_and_started_by_its_path() {
    use std::os::unix::fs::PermissionsExt;

    let folder = Folder::new("script");
    let script = folder.path().join("report");
    let text = b"#!/bin/sh\necho \"ran as $0\"\n";
    std::fs::write(&script, text).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let supervisor = supervisor();
    let mut spec = LaunchSpec {
        program: ProgramRef {
            path: script.clone(),
            digest: Some(Digest::of(text)),
        },
        args: Vec::new(),
        env: EnvAllowlist::default(),
        cwd: None,
        kind: SubsystemKind::new("command").unwrap(),
        place: None,
    };
    let finished = supervisor.run(spec.clone(), one_shot(60)).await.unwrap();
    assert_eq!(finished.exit, Exit::Code(0), "{finished:?}");
    assert!(finished.verified);
    assert_eq!(
        String::from_utf8(finished.stdout).unwrap().trim(),
        format!("ran as {}", script.display())
    );
    // Another digest than the script's: nothing runs.
    spec.program.digest = Some(Digest::of(b"another script"));
    let failure = supervisor.run(spec, one_shot(60)).await.unwrap_err();
    assert_eq!(failure.failure.code(), &codes::EFFECT_NOT_DECLARED);
}

/// The variable the parent test sets in the child test process: the test
/// crate may not call `set_var`, which is unsafe, so it starts itself.
const ENGINE_VARIABLE_FOR_TEST: &str = "A2M_SOMETHING";

#[test]
fn a_name_of_the_engine_is_known_in_any_letter_case() {
    assert!(a2m_provider::is_engine_variable("A2M_HOME"));
    assert!(a2m_provider::is_engine_variable("a2m_vault_passphrase"));
    assert!(a2m_provider::is_engine_variable("A2m_x"));
    assert!(!a2m_provider::is_engine_variable("A2M"));
    assert!(!a2m_provider::is_engine_variable("EXAMPLE_A2M_X"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_launch_that_names_a_variable_of_the_engine_is_refused_and_nothing_starts() {
    let stand = Stand::start().unwrap();
    let supervisor = supervisor();
    for (pass, set) in [
        (vec!["a2m_vault_passphrase".to_owned()], Vec::new()),
        (
            Vec::new(),
            vec![("A2M_HOME".to_owned(), OsString::from("elsewhere"))],
        ),
    ] {
        let mut launch = spec(&stand, &["item", "show", "SK-1001"]);
        launch.env.pass = pass;
        launch.env.set.extend(set);
        let refused = supervisor.run(launch, one_shot(60)).await.unwrap_err();
        assert_eq!(refused.ran, Ran::NotStarted);
        assert_eq!(refused.failure.class(), FailureClass::Policy);
        assert_eq!(refused.failure.code(), &codes::ENGINE_VARIABLE_REFUSED);
    }
    assert!(stand.control().invocations().unwrap().is_empty());
}

/// Starts this test binary again with an engine variable set, running only
/// the test below, which does its work only then.
#[test]
fn a_variable_of_the_engine_does_not_reach_a_program() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "the_program_started_while_a_variable_of_the_engine_is_set",
            "--nocapture",
        ])
        .env(ENGINE_VARIABLE_FOR_TEST, "engine-value-for-tests")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("1 passed"),
        "the inner test did not run: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_program_started_while_a_variable_of_the_engine_is_set() {
    if std::env::var_os(ENGINE_VARIABLE_FOR_TEST).is_none() {
        return;
    }
    let stand = Stand::start().unwrap();
    let mut launch = spec(&stand, &["item", "show", "SK-1001"]);
    // A `PATH` of the system's own folders, which pass the check of every
    // folder a `PATH` names; the test's own may hold any.
    launch
        .env
        .set
        .push(("PATH".to_owned(), system_folder().into_os_string()));
    let finished = supervisor().run(launch, one_shot(60)).await.unwrap();
    assert_eq!(finished.exit, Exit::Code(0));
    let started = stand.control().invocations().unwrap().pop().unwrap();
    assert!(
        !started
            .env
            .keys()
            .any(|name| name.to_ascii_uppercase().starts_with("A2M_")),
        "{:?}",
        started.env.keys().collect::<Vec<_>>()
    );
}

/// The fifth safety audit's PR-2: a launch that sets or passes a variable
/// that makes a program load other code is refused before anything starts,
/// whatever the pack in front of the supervisor checked (decision 0250).
#[tokio::test(flavor = "multi_thread")]
async fn a_launch_that_names_a_variable_that_loads_other_code_is_refused_and_nothing_starts() {
    let stand = Stand::start().unwrap();
    let supervisor = supervisor();
    for (pass, set) in [
        (
            Vec::new(),
            vec![("LD_PRELOAD".to_owned(), OsString::from("/x.so"))],
        ),
        (
            Vec::new(),
            vec![("NODE_OPTIONS".to_owned(), OsString::from("--require=/x.js"))],
        ),
        (vec!["PYTHONPATH".to_owned()], Vec::new()),
        (vec!["dyld_insert_libraries".to_owned()], Vec::new()),
    ] {
        let mut launch = spec(&stand, &["item", "show", "SK-1001"]);
        launch.env.pass = pass;
        launch.env.set.extend(set);
        let refused = supervisor.run(launch, one_shot(60)).await.unwrap_err();
        assert_eq!(refused.ran, Ran::NotStarted);
        assert_eq!(
            (refused.failure.class(), refused.failure.code()),
            (FailureClass::Policy, &codes::VARIABLE_REFUSED)
        );
    }
    assert!(stand.control().invocations().unwrap().is_empty());
}

/// A folder of the system's own, closed to other users on every system.
fn system_folder() -> std::path::PathBuf {
    if cfg!(windows) {
        std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32")
    } else {
        std::path::PathBuf::from("/usr/bin")
    }
}

/// The program that prints its working folder: the shell on Linux and
/// macOS, the command interpreter on Windows. The supervisor itself
/// refuses no program by its name; the packs do.
fn shell_that_prints_its_folder() -> (std::path::PathBuf, Vec<OsString>) {
    if cfg!(windows) {
        (
            system_folder().join("cmd.exe"),
            vec!["/d".into(), "/c".into(), "cd".into()],
        )
    } else {
        ("/bin/sh".into(), vec!["-c".into(), "pwd; ls -ld .".into()])
    }
}

/// Finding D5 of the sixth safety audit (decision 0271): a program whose
/// launch names no working folder works in a folder made for that start
/// alone, which only the engine's user may write, and which is gone when
/// the run has ended. Before, it worked in the system's temporary folder,
/// which on Linux every user may write.
#[tokio::test(flavor = "multi_thread")]
async fn a_program_with_no_working_folder_works_in_a_private_one_that_is_removed_after() {
    let (program, args) = shell_that_prints_its_folder();
    let launch = LaunchSpec {
        program: ProgramRef {
            path: program,
            digest: None,
        },
        args,
        env: EnvAllowlist::default(),
        cwd: None,
        kind: SubsystemKind::new("command").unwrap(),
        place: None,
    };
    let finished = supervisor().run(launch, one_shot(60)).await.unwrap();
    assert_eq!(finished.exit, Exit::Code(0), "{finished:?}");
    let text = String::from_utf8(finished.stdout).unwrap();
    let mut lines = text.lines();
    let folder = std::path::PathBuf::from(lines.next().unwrap().trim());
    let temporary = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    assert_ne!(
        std::fs::canonicalize(folder.parent().unwrap()).ok(),
        None,
        "{}",
        folder.display()
    );
    assert_eq!(
        std::fs::canonicalize(folder.parent().unwrap()).unwrap(),
        temporary,
        "it was made in the temporary folder: {}",
        folder.display()
    );
    assert_ne!(
        std::fs::canonicalize(&folder).ok(),
        Some(temporary),
        "it is not the temporary folder itself"
    );
    #[cfg(unix)]
    {
        let listed = lines.next().unwrap_or_default();
        assert!(
            listed.starts_with("drwx------"),
            "only the engine's user may use it: {listed}"
        );
    }
    assert!(!folder.exists(), "it is removed: {}", folder.display());
}

/// Finding D1 of the sixth safety audit (decision 0271): a `PATH` given to a
/// program, passed or set, names folders it loads code from, and on Windows
/// the system loads from them the libraries a program asks for by name.
/// One that names a folder another user may change, or a name that is not
/// absolute, is refused, and nothing starts. A folder that is not there
/// passes where nobody else could make it.
#[tokio::test(flavor = "multi_thread")]
async fn a_path_that_names_a_folder_others_may_change_is_refused_and_nothing_starts() {
    let stand = Stand::start().unwrap();
    let supervisor = supervisor();
    let root = std::env::temp_dir().join(format!("a2m-provider-path-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let open = root.join("open");
    std::fs::create_dir_all(&open).unwrap();
    open_to_everyone(&open);
    let closed = root.join("closed");
    std::fs::create_dir_all(&closed).unwrap();
    close_to_others(&root);
    close_to_others(&closed);
    let joined = |folders: &[std::path::PathBuf]| std::env::join_paths(folders).unwrap();
    for value in [
        joined(&[system_folder(), open.clone()]),
        joined(&[open.join("not-there")]),
        OsString::from("relative-folder"),
    ] {
        let mut launch = spec(&stand, &["item", "show", "SK-1001"]);
        launch.env.set.push(("PATH".to_owned(), value.clone()));
        let refused = supervisor.run(launch, one_shot(60)).await.unwrap_err();
        assert_eq!(refused.ran, Ran::NotStarted, "{value:?}");
        assert_eq!(refused.failure.class(), FailureClass::Policy, "{value:?}");
        assert_eq!(
            refused.failure.code(),
            &codes::VARIABLE_REFUSED,
            "{value:?}"
        );
    }
    assert!(stand.control().invocations().unwrap().is_empty());
    let mut launch = spec(&stand, &["item", "show", "SK-1001"]);
    launch.env.set.push((
        "PATH".to_owned(),
        joined(&[system_folder(), closed.clone(), closed.join("not-there")]),
    ));
    let ran = supervisor.run(launch, one_shot(60)).await;
    let _ = std::fs::remove_dir_all(&root);
    let ran = ran.unwrap();
    assert_eq!(ran.exit, Exit::Code(0));
}

#[cfg(unix)]
fn open_to_everyone(folder: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o777)).unwrap();
}

#[cfg(unix)]
fn close_to_others(folder: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Everyone may add a file to it.
#[cfg(windows)]
fn open_to_everyone(folder: &std::path::Path) {
    let made = std::process::Command::new(system_folder().join("icacls.exe"))
        .arg(folder)
        .args(["/grant", "*S-1-1-0:(W)"])
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
}

/// Folders under the temporary folder are the user's own on Windows.
#[cfg(windows)]
fn close_to_others(_folder: &std::path::Path) {}
