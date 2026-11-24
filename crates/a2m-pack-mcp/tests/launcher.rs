//! A local server started through a launcher with its entry file pinned
//! (package L3, decision 0371): the JavaScript runtime, a launcher of the
//! provider's closed table, runs the stand's server as an installed entry
//! file, and a write through it is proven by asking the stand what it
//! stored. A changed entry file, a package runner, and a program that is
//! not a launcher of the table each stop before anything starts.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::{Path, PathBuf};

use a2m_core::{CompiledTarget, Digest, FailureClass, Values};
use a2m_pack_mcp::codes;
use a2m_provider::{Authority, Posture, Registry, Sent, StepOutcome};
use common::{
    Folder, compile, fence_of, input, key, prepare, record, registry, running, shown, steps, store,
    surface, token, write_target,
};
use example_mcp::harness::StdioStand;

const PLACEHOLDER_DIGEST: &str =
    "sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// A text as a YAML scalar in single quotes, in which nothing is an escape.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn digest_of(path: &Path) -> String {
    format!("sha256:{}", Digest::of(&std::fs::read(path).unwrap()))
}

/// The JavaScript runtime, found on the search path, as the file it leads
/// to. On a CI machine it is always there; elsewhere the test is skipped.
/// Where its own folder is not closed to other users, as the check
/// requires, a copy of it in a folder of the test's own is used: the
/// runtime is one file.
fn launcher() -> Option<(PathBuf, Option<Folder>)> {
    let file = format!("node{}", std::env::consts::EXE_SUFFIX);
    let found = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|folder| folder.join(&file))
            .find(|path| path.is_file())
            .and_then(|path| std::fs::canonicalize(path).ok())
    });
    let Some(found) = found else {
        assert!(
            std::env::var_os("CI").is_none(),
            "no `{file}` on the search path of this CI machine"
        );
        println!("skipped: no `{file}` on the search path of this machine");
        return None;
    };
    let trust = a2m_provider::ProgramTrust::default();
    if a2m_provider::check_program(&found, &trust, a2m_provider::ProgramRole::Interpreter).is_ok() {
        return Some((found, None));
    }
    let folder = Folder::new("node");
    let copy = folder.path().join(&file);
    std::fs::copy(&found, &copy).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    Some((copy, Some(folder)))
}

struct Scene {
    _folder: Folder,
    _launcher: Option<Folder>,
    registry: Registry,
    compiled_target: CompiledTarget,
    stand: StdioStand,
    entry: PathBuf,
}

/// The fixture target with the surface `orders_local` started through
/// `program` as a launcher running the stand's entry file.
fn target_text(
    stand: &StdioStand,
    program: &Path,
    entry: &Path,
    entry_digest: &str,
) -> impl Fn(&str, String) -> String {
    let state = quoted(&stand.state_folder().to_string_lossy());
    let program_digest = digest_of(program);
    let program = quoted(&program.to_string_lossy());
    let entry = quoted(&entry.to_string_lossy());
    let entry_digest = entry_digest.to_owned();
    let server = quoted(&stand.program().to_string_lossy());
    move |path: &str, text: String| {
        let local = |text: String, tool: &str| {
            text.replace(
                &format!("    tool: {tool}"),
                &format!("    surface: orders_local\n    tool: {tool}"),
            )
        };
        match path {
            "target.yaml" => text
                .replace("\"/opt/example/bin/example-mcp\"", &program)
                .replace(PLACEHOLDER_DIGEST, &program_digest)
                .replace(
                    "\"/var/lib/example-mcp\"}",
                    &format!(
                        "{state}}}\n    entry: {entry}\n    entry_digest: \"{entry_digest}\"\n    args: [{server}, from-the-launcher]"
                    ),
                ),
            "operations/order_note_update.yaml" => local(text, "orders_update"),
            "operations/order_get.yaml" => local(text, "orders_get"),
            "mappings/orders_local.yaml" => format!(
                "{text}  orders_update:\n    digest: \"sha256:4dae58ac57727e98baba35c455cb2dcffa9d03d5c3c205cbbfe05407966d0ce1\"\n    effect: write\n    read_back: orders_get\n    result: text\n"
            ),
            _ => text,
        }
    }
}

fn scene() -> Option<Scene> {
    let (program, launcher_folder) = launcher()?;
    let folder = Folder::new("launcher");
    let registry = registry(&folder);
    let stand = StdioStand::new().unwrap();
    let entry = stand.launcher_entry().unwrap();
    let root = folder.path().join("target");
    write_target(
        &root,
        &target_text(&stand, &program, &entry, &digest_of(&entry)),
    );
    let compiled_target = match compile(&registry, &root) {
        Ok(compiled) => compiled.compiled_target,
        Err(problems) => panic!(
            "the launcher target does not compile:\n{}",
            shown(&problems)
        ),
    };
    Some(Scene {
        _folder: folder,
        _launcher: launcher_folder,
        registry,
        compiled_target,
        stand,
        entry,
    })
}

/// The proof of package L3: the stand's server, started through the
/// JavaScript runtime with its entry file pinned, is wrapped, and a write
/// through it is verified by asking the stand what it stored.
#[tokio::test]
async fn a_server_started_through_a_launcher_is_wrapped_and_a_write_through_it_is_stored() {
    let Some(scene) = scene() else { return };
    let control = scene.stand.control();
    let local = surface(&scene.compiled_target, "orders_local", None);
    let (ledger, _vault) = store().await;
    let fence = running(&ledger, 1, &["A-100"]).await;
    let (handle, mut prepared) = prepare(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence,
        Posture::Live,
        None,
    )
    .await
    .unwrap();
    let update = steps(&scene.compiled_target, "example_order_note_update");
    let args = Values::new()
        .with("order_id", "A-100")
        .with("field", "note")
        .with("value", "Leave at the gate");
    let order = key("A-100");
    let intent = token(&ledger, fence, 0, "A-100").await;
    let written = handle
        .act(
            &mut prepared,
            &update.act,
            &input(&args, Some(record(&fence, 0)), Some(&order), None),
            Authority::Write(&intent),
        )
        .await;
    assert!(matches!(written, Ok(StepOutcome::Done(_))), "{written:?}");
    let read_back = handle
        .act(
            &mut prepared,
            update.read_back.as_ref().expect("the update reads back"),
            &input(
                &Values::new().with("order_id", "A-100"),
                None,
                Some(&order),
                None,
            ),
            Authority::Read,
        )
        .await;
    assert!(
        matches!(read_back, Ok(StepOutcome::Done(_))),
        "{read_back:?}"
    );
    assert!(handle.release(prepared, false).await.complete);

    // The stand says what it stored, and once.
    assert_eq!(control.write_count("A-100").unwrap(), 1);
    assert_eq!(
        control.stored_order("A-100").unwrap().unwrap().note,
        "Leave at the gate"
    );
    // The program the entry file started was given the arguments after
    // it, element for element: the launcher ran the entry file where it
    // lies, and the file beside it loaded.
    let starts = control.starts().unwrap();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].args, ["from-the-launcher"]);
}

/// An entry file changed after its digest was pinned stops with a named
/// code before the launcher starts.
#[tokio::test]
async fn an_entry_file_changed_after_its_pin_is_refused_before_anything_starts() {
    let Some(scene) = scene() else { return };
    let mut text = std::fs::read_to_string(&scene.entry).unwrap();
    text.push_str("\n// changed after it was reviewed\n");
    std::fs::write(&scene.entry, text).unwrap();
    let local = surface(&scene.compiled_target, "orders_local", None);
    let refused = prepare(
        &scene.registry,
        &scene.compiled_target,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(refused.sent, Sent::NotSent);
    assert_eq!(
        (refused.failure.class(), refused.failure.code()),
        (FailureClass::Drift, &codes::ENTRY_DIGEST_CHANGED)
    );
    assert!(scene.stand.control().starts().unwrap().is_empty());
    assert!(scene.stand.control().connections().unwrap().is_empty());
}

/// A surface whose program is not a launcher of the table, given an entry
/// file, does not compile; nor does one started through a package runner.
#[test]
fn a_package_runner_and_a_program_not_in_the_table_do_not_compile() {
    let folder = Folder::new("launcher-lint");
    let registry = registry(&folder);
    let stand = StdioStand::new().unwrap();
    let entry = stand.launcher_entry().unwrap();
    let digest = digest_of(&entry);
    let problems_with = |program: &str| {
        let root = folder.path().join("target");
        let _ = std::fs::remove_dir_all(&root);
        let text = target_text(&stand, Path::new(program), &entry, &digest);
        write_target(&root, &text);
        compile(&registry, &root).expect_err("the target must not compile")
    };
    // The stand's own program is a server, not a launcher of the table.
    let not_launcher = problems_with(&stand.program().to_string_lossy());
    assert!(
        not_launcher.find("mcp.launcher_form").is_some(),
        "{}",
        shown(&not_launcher)
    );

    // A package runner, by its name, with an entry file or without.
    let runner_folder = Folder::new("runner");
    let runner = runner_folder.path().join("npx");
    std::fs::copy(stand.program(), &runner).unwrap();
    let runner_problems = problems_with(&runner.to_string_lossy());
    assert!(
        runner_problems.find("mcp.package_runner").is_some(),
        "{}",
        shown(&runner_problems)
    );
}

/// A program declared by a name that passes, whose path leads to a package
/// runner, is refused by the driver by the name of the file it leads to,
/// with what to do instead, before anything starts.
#[cfg(unix)]
#[tokio::test]
async fn a_package_runner_behind_a_link_is_refused_at_start_with_what_to_do_instead() {
    let folder = Folder::new("runner-link");
    let registry = registry(&folder);
    let stand = StdioStand::new().unwrap();
    let runner = folder.path().join("npx");
    std::fs::copy(stand.program(), &runner).unwrap();
    let link = folder.path().join("example-server");
    std::os::unix::fs::symlink(&runner, &link).unwrap();
    let state = quoted(&stand.state_folder().to_string_lossy());
    let digest = digest_of(&runner);
    let program = quoted(&link.to_string_lossy());
    let root = folder.path().join("target");
    write_target(&root, &|path, text| match path {
        "target.yaml" => text
            .replace("\"/opt/example/bin/example-mcp\"", &program)
            .replace(PLACEHOLDER_DIGEST, &digest)
            .replace("\"/var/lib/example-mcp\"}", &format!("{state}}}")),
        "operations/order_get.yaml" => text.replace(
            "    tool: orders_get",
            "    surface: orders_local\n    tool: orders_get",
        ),
        _ => text,
    });
    let compiled_target = compile(&registry, &root)
        .unwrap_or_else(|problems| panic!("the target does not compile:\n{}", shown(&problems)))
        .compiled_target;
    let local = surface(&compiled_target, "orders_local", None);
    let refused = prepare(
        &registry,
        &compiled_target,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(refused.sent, Sent::NotSent);
    assert_eq!(
        (refused.failure.class(), refused.failure.code()),
        (FailureClass::Policy, &codes::PACKAGE_RUNNER_REFUSED)
    );
    assert!(refused.failure.next_step().contains("entry"));
    assert!(stand.control().starts().unwrap().is_empty());
}

/// The review shows a server a launcher runs as the fixed pair it is: the
/// launcher with its digest, the arguments of its form, and the entry file
/// with the digest that pins it, which the compiled target declares as a
/// program of the surface.
#[test]
fn the_review_shows_the_launcher_and_the_entry_file_it_runs() {
    let folder = Folder::new("launcher-review");
    let registry = registry(&folder);
    let stand = StdioStand::new().unwrap();
    let entry = stand.launcher_entry().unwrap();
    let digest = digest_of(&entry);
    // Any file serves as the launcher here: nothing is started.
    let node = folder.path().join("node");
    std::fs::copy(stand.program(), &node).unwrap();
    let text = target_text(&stand, &node, &entry, &digest);
    let root = folder.path().join("target");
    write_target(&root, &text);
    let compiled = compile(&registry, &root)
        .unwrap_or_else(|problems| panic!("the target does not compile:\n{}", shown(&problems)));
    let item = compiled
        .review
        .items
        .iter()
        .find(|item| item.tool.as_str() == "example_order_get")
        .unwrap();
    let a2m_files::BindingStates::Stated { runs, .. } = &item.bindings[0].states else {
        panic!("the binding states what it runs");
    };
    assert_eq!(
        runs[0].run,
        a2m_files::ProgramRun::Launcher {
            launcher: node.to_string_lossy().into_owned(),
            launcher_digests: vec![digest_of(&node)[7..].parse().unwrap()],
            arguments: Vec::new(),
            entry: entry.to_string_lossy().into_owned(),
            entry_digest: digest[7..].parse().unwrap(),
        }
    );
    assert_eq!(runs[0].notice, Some(a2m_files::LAUNCHER_NOTICE));
    // Both files are programs the compiled target declares for the surface.
    let declared = &item.bindings[0].surface_programs;
    assert!(
        declared
            .iter()
            .any(|program| program.path == entry.to_string_lossy()),
        "{declared:?}"
    );
}

/// A program declared as `node` whose path leads to a file that is not a
/// launcher of the table is refused by the driver by the name of the file
/// it leads to, before anything starts.
#[cfg(unix)]
#[tokio::test]
async fn a_launcher_whose_file_is_not_in_the_table_is_refused_at_start() {
    let folder = Folder::new("launcher-link");
    let registry = registry(&folder);
    let stand = StdioStand::new().unwrap();
    let entry = stand.launcher_entry().unwrap();
    let link = folder.path().join("node");
    std::os::unix::fs::symlink(stand.program(), &link).unwrap();
    let root = folder.path().join("target");
    write_target(
        &root,
        &target_text(&stand, &link, &entry, &digest_of(&entry)),
    );
    let compiled_target = compile(&registry, &root)
        .unwrap_or_else(|problems| panic!("the target does not compile:\n{}", shown(&problems)))
        .compiled_target;
    let local = surface(&compiled_target, "orders_local", None);
    let refused = prepare(
        &registry,
        &compiled_target,
        &local,
        fence_of(7),
        Posture::Live,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(refused.sent, Sent::NotSent);
    assert_eq!(
        (refused.failure.class(), refused.failure.code()),
        (FailureClass::Policy, &codes::LAUNCHER_NOT_IN_TABLE)
    );
    assert!(stand.control().starts().unwrap().is_empty());
}
