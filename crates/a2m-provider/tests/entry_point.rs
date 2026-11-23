//! Provider packages found through a Python entry point (package F3),
//! proven against a real pip: a test provider package is built as a wheel,
//! installed into a throwaway virtual environment with no network, found
//! through its entry point, listed as present and not enabled, and
//! started under the supervisor's rules. Nothing of the package is run or
//! imported by the search, and an entry that is not the name of a package
//! is refused, never loaded.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use a2m_core::{Availability, Digest};
use a2m_provider::{
    ENTRY_POINT_GROUP, Exit, OneShot, ProviderKind, ProviderSettings, RegistryBuilder, Roots,
    Supervisor, SupervisorLimits, find_python_providers,
};
use common::{Folder, host, manifest};
use example_cli::harness::Stand;

const MAKE_WHEEL: &str = include_str!("common/make_wheel.py");
const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

fn python() -> String {
    ["python3", "python"]
        .into_iter()
        .find(|name| {
            Command::new(name)
                .arg("--version")
                .output()
                .is_ok_and(|out| out.status.success())
        })
        .expect("a Python interpreter is needed for this test")
        .to_owned()
}

fn run(program: &str, args: &[&str]) -> String {
    let out = Command::new(program).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{program} {args:?}: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// The plug-in's manifest text: kind plug-in, the digest of its program, and
/// the one fixed command line it is started with.
fn plugin_manifest(name: &str, digest: Option<&Digest>) -> String {
    let digest_line = digest.map_or_else(String::new, |d| format!("\ndigest = \"sha256:{d}\""));
    manifest(name, "artifact", "")
        .replace(
            r#"kind = "pack""#,
            &format!("kind = \"plugin\"{digest_line}"),
        )
        .replace(
            "[[modes]]",
            &format!(
                "[run]\nprogram = \"bin/example-sheet{EXE}\"\nargs = [\"item\", \"show\", \"SK-1001\", \"--format\", \"json\"]\nprotocol = [1]\n\n[[modes]]"
            ),
        )
}

struct Venv {
    python: PathBuf,
    site: PathBuf,
}

/// Makes a virtual environment and installs wheels into it with no network.
fn venv_with(folder: &Folder, wheels: &[PathBuf]) -> Venv {
    let root = folder.path().join("venv");
    run(&python(), &["-m", "venv", root.to_str().unwrap()]);
    let python = root
        .join(if cfg!(windows) { "Scripts" } else { "bin" })
        .join(format!("python{EXE}"));
    for wheel in wheels {
        run(
            python.to_str().unwrap(),
            &[
                "-m",
                "pip",
                "install",
                "--quiet",
                "--no-index",
                "--no-deps",
                wheel.to_str().unwrap(),
            ],
        );
    }
    let site = run(
        python.to_str().unwrap(),
        &[
            "-c",
            "import sysconfig; print(sysconfig.get_paths()['purelib'])",
        ],
    );
    Venv {
        python,
        site: PathBuf::from(site),
    }
}

#[allow(clippy::too_many_arguments)]
fn wheel(
    folder: &Folder,
    distribution: &str,
    entry: &str,
    value: &str,
    package: &str,
    marker: &Path,
    manifest_text: &str,
    program: Option<&Path>,
) -> PathBuf {
    let scripts = folder.path().join("scripts");
    std::fs::create_dir_all(&scripts).unwrap();
    let script = scripts.join("make_wheel.py");
    std::fs::write(&script, MAKE_WHEEL).unwrap();
    let text = scripts.join(format!("{distribution}.toml"));
    std::fs::write(&text, manifest_text).unwrap();
    let out = folder.path().join("wheels");
    std::fs::create_dir_all(&out).unwrap();
    let mut args = vec![
        script.to_str().unwrap().to_owned(),
        out.to_str().unwrap().to_owned(),
        distribution.to_owned(),
        entry.to_owned(),
        value.to_owned(),
        package.to_owned(),
        marker.to_str().unwrap().to_owned(),
        text.to_str().unwrap().to_owned(),
    ];
    if let Some(program) = program {
        args.push(program.to_str().unwrap().to_owned());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    PathBuf::from(run(&python(), &refs))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_provider_package_installed_with_pip_is_found_listed_not_enabled_and_started_under_the_supervisor()
 {
    let folder = Folder::new("entry-point");
    let marker = folder.path().join("imported-marker");
    let stand = Stand::start().unwrap();

    // The program the package ships: the command-line test stand's.
    let program_file = folder.path().join(format!("example-sheet{EXE}"));
    std::fs::copy(stand.program(), &program_file).unwrap();
    let digest = Digest::of(&std::fs::read(&program_file).unwrap());

    let good = wheel(
        &folder,
        "example_sheet_provider",
        "example_sheet",
        "example_sheet_provider",
        "example_sheet_provider",
        &marker,
        &plugin_manifest("example_sheet", Some(&digest)),
        Some(&program_file),
    );
    // An entry that is a loadable name of code, not a package: refused.
    let loadable = wheel(
        &folder,
        "example_loader",
        "example_loader",
        "example_loader_pkg:start",
        "example_loader_pkg",
        &marker,
        &plugin_manifest("example_loader", None),
        None,
    );
    // An entry whose name is not the name its manifest gives.
    let renamed = wheel(
        &folder,
        "example_renamed",
        "example_other",
        "example_renamed",
        "example_renamed",
        &marker,
        &plugin_manifest("example_renamed", None),
        None,
    );
    let venv = venv_with(&folder, &[good, loadable, renamed]);
    assert!(
        !marker.exists(),
        "installing a wheel does not import its package"
    );

    // The search names where it looked and runs nothing.
    let search = find_python_providers(std::slice::from_ref(&venv.site));
    assert_eq!(search.searched.len(), 1);
    assert_eq!(search.searched[0], venv.site);
    assert!(
        !marker.exists(),
        "the search imported or ran a package: the marker was written"
    );
    assert_eq!(search.found.len(), 1, "{search:?}");
    let found = &search.found[0];
    assert_eq!(found.entry, "example_sheet");
    assert_eq!(found.package, "example_sheet_provider");
    assert!(found.distribution.starts_with("example_sheet_provider-"));
    assert_eq!(search.problems.len(), 2, "{:?}", search.problems);
    let statement = search.statement();
    assert!(statement.contains(ENTRY_POINT_GROUP), "{statement}");
    assert!(statement.contains("No package code was run"), "{statement}");
    assert!(
        statement.contains(venv.site.to_str().unwrap()),
        "{statement}"
    );

    // Listed as present and not enabled, and it registers nothing.
    let mut builder =
        RegistryBuilder::new(ProviderSettings::default(), host(&folder, Roots::none()));
    for one in &search.found {
        builder.add_plugin(&one.found);
    }
    let (registry, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    let listing = registry.listing();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].name.as_str(), "example_sheet");
    assert_eq!(listing[0].kind, ProviderKind::Plugin);
    assert_eq!(listing[0].availability, Availability::NotEnabled);
    assert!(!listing[0].registered);

    // Started under the supervisor: the program inside the package, by its
    // declared digest, with the manifest's fixed arguments and nothing else.
    let mut spec = found.launch_spec().unwrap();
    assert!(
        spec.program
            .path
            .starts_with(found.found.folder.canonicalize().unwrap())
    );
    assert_eq!(spec.program.digest, Some(digest));
    assert!(spec.env.pass.is_empty());
    spec.env.set.push((
        "EXAMPLE_CLI_HOME".to_owned(),
        stand.home().as_os_str().to_owned(),
    ));
    let finished = Supervisor::new(SupervisorLimits::default())
        .run(
            spec,
            OneShot {
                max_output_bytes: 64 * 1024,
                time_limit: Duration::from_secs(60),
                stdin: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(finished.exit, Exit::Code(0));
    assert!(finished.verified, "the declared digest was the program's");
    assert!(String::from_utf8_lossy(&finished.stdout).contains("SK-1001"));
    assert!(!marker.exists(), "no Python ran at any step");

    // The venv's own Python sees the same entry without loading it, and the
    // search finds the same folder the interpreter reports.
    let seen = run(
        venv.python.to_str().unwrap(),
        &[
            "-c",
            "from importlib.metadata import entry_points as e; print(sorted(p.name for p in e(group='anything2mcp.providers')))",
        ],
    );
    assert!(seen.contains("example_sheet"), "{seen}");
    assert!(
        !marker.exists(),
        "listing entry points does not import them"
    );
}

#[test]
fn no_site_folder_or_a_folder_with_no_package_finds_nothing_and_says_where_it_looked() {
    let folder = Folder::new("entry-point-empty");
    let none = find_python_providers(&[]);
    assert!(none.found.is_empty() && none.problems.is_empty());
    assert!(none.statement().contains("no folder"));
    let empty = find_python_providers(&[folder.path().to_path_buf(), folder.path().join("gone")]);
    assert!(empty.found.is_empty() && empty.problems.is_empty());
    assert_eq!(empty.searched.len(), 2);
}
