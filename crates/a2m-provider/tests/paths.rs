//! A path outside its root can never become a `CheckedPath`. The lexical
//! refusals are the same on every operating system, so the forms that
//! escape on one system are tried, and refused, on all of them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use a2m_core::FailureClass;
use a2m_provider::{Access, EntryKind, ProviderSettings, RegistryBuilder, RootName, Roots, codes};
use common::{Folder, TestPack, host};

fn root(name: &str) -> RootName {
    RootName::new(name).unwrap()
}

/// A root named `project` with one file, and a folder beside it that is
/// outside the root.
fn scene() -> (Folder, Roots) {
    let folder = Folder::new("paths");
    let project = folder.path().join("project");
    std::fs::create_dir_all(project.join("orders")).unwrap();
    std::fs::write(project.join("orders").join("one.txt"), b"one").unwrap();
    std::fs::create_dir_all(folder.path().join("outside")).unwrap();
    std::fs::write(folder.path().join("outside").join("secret.txt"), b"secret").unwrap();
    let mut roots = Roots::none();
    roots.add(root("project"), &project, Access::Write).unwrap();
    (folder, roots)
}

#[test]
fn every_escape_form_is_refused_on_every_operating_system() {
    let (_folder, roots) = scene();
    let long = "a/".repeat(600);
    let escapes = [
        "",
        ".",
        "..",
        "../outside/secret.txt",
        "orders/../../outside/secret.txt",
        "orders/..",
        "./orders/one.txt",
        "orders/./one.txt",
        "orders//one.txt",
        "orders/",
        "/etc/passwd",
        "//server/share/file",
        "~/file",
        "C:/Windows/system.ini",
        "C:\\Windows\\system.ini",
        "c:file.txt",
        "C:",
        "\\\\server\\share\\file",
        "\\\\?\\C:\\Windows",
        "\\\\.\\pipe\\name",
        "orders\\one.txt",
        "..\\outside\\secret.txt",
        "orders/one.txt:stream",
        "orders/one.txt::$DATA",
        "nul",
        "NUL",
        "con.txt",
        "orders/AUX",
        "orders/com1.log",
        "LPT9",
        "orders./one.txt",
        "orders /one.txt",
        "orders/one.txt.",
        "orders/one.txt ",
        "orders/\u{0}one.txt",
        "orders/one\n.txt",
        long.as_str(),
    ];
    for escape in escapes {
        for access in [Access::Read, Access::Write] {
            let failure = roots.check(&root("project"), escape, access).unwrap_err();
            assert_eq!(
                (failure.class(), failure.code()),
                (FailureClass::Input, &codes::PATH_NOT_ALLOWED),
                "{escape:?}"
            );
        }
    }
    // The same forms are refused when joined to a checked folder.
    let orders = roots
        .check(&root("project"), "orders", Access::Read)
        .unwrap();
    for escape in ["..", "../../outside", "/etc", "C:\\x", "a\\b", ""] {
        assert_eq!(
            orders.join(escape).unwrap_err().code(),
            &codes::PATH_NOT_ALLOWED,
            "{escape:?}"
        );
    }
}

#[test]
fn a_path_inside_the_root_is_read_written_and_listed() {
    let (_folder, roots) = scene();
    let one = roots
        .check(&root("project"), "orders/one.txt", Access::Read)
        .unwrap();
    assert_eq!(one.read(1024).unwrap(), b"one");
    assert_eq!(one.size().unwrap(), 3);
    assert_eq!(one.to_string(), "project:orders/one.txt");
    assert_eq!(one.read(2).unwrap_err().code(), &codes::FILE_UNAVAILABLE);
    // A path checked for reading cannot write.
    let failure = one.write(b"two").unwrap_err();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Policy, &codes::EFFECT_NOT_DECLARED)
    );
    // A file that does not exist yet, in a folder that does not exist yet.
    let folder = roots
        .check(&root("project"), "new/deeper", Access::Write)
        .unwrap();
    folder.create_dir_all().unwrap();
    let new = folder.join("two.txt").unwrap();
    assert!(!new.exists());
    new.write(b"two").unwrap();
    assert_eq!(new.read(1024).unwrap(), b"two");
    assert_eq!(new.relative(), "new/deeper/two.txt");
    let (listed, left_out) = folder.list().unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|path| path.relative().to_owned())
            .collect::<Vec<_>>(),
        ["new/deeper/two.txt"]
    );
    assert_eq!(left_out, 0);
    new.remove_file().unwrap();
    assert!(!new.exists());
}

#[test]
fn a_root_that_is_not_declared_cannot_be_used() {
    let (folder, roots) = scene();
    let failure = roots
        .check(&root("sources"), "a.txt", Access::Read)
        .unwrap_err();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Policy, &codes::EFFECT_NOT_DECLARED)
    );
    // A pack receives only the roots its manifest declares, and for reading
    // when it declares reading.
    let mut all = roots.clone();
    all.add(
        root("sources"),
        &folder.path().join("outside"),
        Access::Write,
    )
    .unwrap();
    let pack = TestPack::new("alpha");
    let mut builder = RegistryBuilder::new(ProviderSettings::default(), host(&folder, all));
    builder.add_pack(&pack);
    let (_, problems) = builder.build();
    assert!(problems.is_empty(), "{problems:?}");
    let context = pack.context.lock().unwrap().clone().unwrap();
    assert_eq!(
        context
            .roots()
            .names()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["project"]
    );
    assert!(
        context
            .folder(&root("project"), Access::Read)
            .unwrap()
            .is_dir()
    );
    assert_eq!(
        context
            .folder(&root("project"), Access::Write)
            .unwrap_err()
            .code(),
        &codes::EFFECT_NOT_DECLARED
    );
    assert_eq!(
        context
            .roots()
            .check(&root("sources"), "secret.txt", Access::Read)
            .unwrap_err()
            .code(),
        &codes::EFFECT_NOT_DECLARED
    );
    // A root must be an existing folder named by an absolute path.
    let mut more = Roots::none();
    assert!(
        more.add(
            root("relative"),
            std::path::Path::new("somewhere"),
            Access::Read
        )
        .is_err()
    );
    assert!(
        more.add(
            root("missing"),
            &folder.path().join("no-such-folder"),
            Access::Read
        )
        .is_err()
    );
}

/// Makes a link to a folder. `None` where this machine does not let the
/// test make one, as Windows does without the privilege.
fn link_folder(target: &std::path::Path, link: &std::path::Path) -> Option<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).ok();
    #[cfg(windows)]
    return std::os::windows::fs::symlink_dir(target, link).ok();
}

#[test]
fn a_link_that_leaves_the_root_is_refused_and_one_that_stays_inside_is_followed() {
    let (folder, roots) = scene();
    let project = folder.path().join("project");
    let outside = folder.path().join("outside");
    if link_folder(&outside, &project.join("way-out")).is_none() {
        // Without the right to make a link there is nothing to plant.
        return;
    }
    link_folder(&project.join("orders"), &project.join("same")).unwrap();

    for escape in ["way-out", "way-out/secret.txt", "way-out/new-file.txt"] {
        for access in [Access::Read, Access::Write] {
            let failure = roots.check(&root("project"), escape, access).unwrap_err();
            assert_eq!(failure.code(), &codes::PATH_NOT_ALLOWED, "{escape}");
        }
    }
    let inside = roots
        .check(&root("project"), "same/one.txt", Access::Read)
        .unwrap();
    assert_eq!(inside.read(16).unwrap(), b"one");
    // The folder's listing leaves the link out and counts it.
    let (listed, left_out) = roots
        .folder(&root("project"), Access::Read)
        .unwrap()
        .list()
        .unwrap();
    assert!(listed.iter().all(|path| path.relative() != "way-out"));
    assert_eq!(left_out, 1);

    // A link planted after the check is refused when the file is opened.
    let later = roots
        .check(&root("project"), "later/file.txt", Access::Write)
        .unwrap();
    link_folder(&outside, &project.join("later")).unwrap();
    assert_eq!(
        later.write(b"x").unwrap_err().code(),
        &codes::PATH_NOT_ALLOWED
    );
    assert_eq!(later.read(16).unwrap_err().code(), &codes::PATH_NOT_ALLOWED);
    assert!(
        !outside.join("file.txt").exists(),
        "a file was written outside the root"
    );
}

/// Makes a link to a file. `None` where this machine does not let the test
/// make one, as Windows does without the privilege.
fn link_file(target: &std::path::Path, link: &std::path::Path) -> Option<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(target, link).ok();
    #[cfg(windows)]
    return std::os::windows::fs::symlink_file(target, link).ok();
}

/// The audit's finding: a link planted at the name the temporary file used
/// to take, `<name>.a2m-new`, led a write to a file outside the root. The
/// temporary file now has a name nobody can plant a link at, and is made
/// new, so the planted link is neither followed nor moved.
#[test]
fn a_write_does_not_go_through_a_link_planted_where_its_temporary_file_used_to_be() {
    let (folder, roots) = scene();
    let project = folder.path().join("project");
    let secret = folder.path().join("outside").join("secret.txt");
    let planted = project.join("orders").join("report.csv.a2m-new");
    if link_file(&secret, &planted).is_none() {
        // Without the right to make a link there is nothing to plant.
        return;
    }
    let report = roots
        .check(&root("project"), "orders/report.csv", Access::Write)
        .unwrap();
    report.write(b"quantity,12\n").unwrap();

    assert_eq!(
        std::fs::read(&secret).unwrap(),
        b"secret",
        "a write inside the root changed a file outside it"
    );
    // The stand is asked what it stored, not the path's own read.
    assert_eq!(
        std::fs::read(project.join("orders").join("report.csv")).unwrap(),
        b"quantity,12\n"
    );
    assert!(
        std::fs::symlink_metadata(project.join("orders").join("report.csv"))
            .unwrap()
            .is_file(),
        "the planted link took the file's name"
    );
    // The planted link is where it was, and nothing else was left behind.
    assert!(
        std::fs::symlink_metadata(&planted)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let mut names: Vec<String> = std::fs::read_dir(project.join("orders"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["one.txt", "report.csv", "report.csv.a2m-new"]);
}

/// What `join` and `check` promise about links, and what a caller can ask
/// without following one. A link at the final name that stays inside the
/// root is followed for reading; `entry_kind` says it is a link; a write
/// and a removal never go through it.
#[test]
fn a_link_at_the_final_name_is_reported_and_never_written_or_removed_through() {
    let (folder, roots) = scene();
    let project = folder.path().join("project");
    std::fs::write(project.join("other.txt"), b"other").unwrap();
    if link_file(
        &project.join("other.txt"),
        &project.join("orders").join("alias.txt"),
    )
    .is_none()
    {
        // Without the right to make a link there is nothing to plant.
        return;
    }
    let orders = roots
        .check(&root("project"), "orders", Access::Write)
        .unwrap();
    assert_eq!(orders.entry_kind().unwrap(), EntryKind::Folder);
    assert_eq!(
        orders.join("one.txt").unwrap().entry_kind().unwrap(),
        EntryKind::File
    );
    assert_eq!(
        orders.join("none.txt").unwrap().entry_kind().unwrap(),
        EntryKind::Absent
    );
    assert_eq!(
        orders
            .join("none/deeper.txt")
            .unwrap()
            .entry_kind()
            .unwrap(),
        EntryKind::Absent
    );
    assert_eq!(
        roots
            .folder(&root("project"), Access::Read)
            .unwrap()
            .entry_kind()
            .unwrap(),
        EntryKind::Folder
    );

    // Followed for reading, because it stays inside the root.
    let alias = orders.join("alias.txt").unwrap();
    assert_eq!(alias.read(16).unwrap(), b"other");
    assert_eq!(alias.entry_kind().unwrap(), EntryKind::Link);
    // Never written through, and never removed through.
    for failure in [
        alias.write(b"changed").unwrap_err(),
        alias.remove_file().unwrap_err(),
    ] {
        assert_eq!(
            (failure.class(), failure.code()),
            (FailureClass::Input, &codes::PATH_NOT_ALLOWED)
        );
    }
    assert_eq!(std::fs::read(project.join("other.txt")).unwrap(), b"other");
    assert!(
        std::fs::symlink_metadata(project.join("orders").join("alias.txt"))
            .unwrap()
            .file_type()
            .is_symlink()
    );

    // A link put at the name after the check: the path was made for a file
    // that was not there yet.
    let later = orders.join("later.txt").unwrap();
    assert_eq!(later.entry_kind().unwrap(), EntryKind::Absent);
    link_file(
        &project.join("other.txt"),
        &project.join("orders").join("later.txt"),
    )
    .unwrap();
    assert_eq!(later.entry_kind().unwrap(), EntryKind::Link);
    assert_eq!(
        later.write(b"changed").unwrap_err().code(),
        &codes::PATH_NOT_ALLOWED
    );
    assert_eq!(
        later.remove_file().unwrap_err().code(),
        &codes::PATH_NOT_ALLOWED
    );
    assert_eq!(std::fs::read(project.join("other.txt")).unwrap(), b"other");
}

/// A file that was a file when it was checked and is a link when it is
/// opened: on Linux and macOS the link is not followed, wherever it leads.
#[cfg(unix)]
#[test]
fn a_file_swapped_for_a_link_after_the_check_is_not_opened() {
    let (folder, roots) = scene();
    let project = folder.path().join("project");
    std::fs::write(project.join("other.txt"), b"other").unwrap();
    let one = roots
        .check(&root("project"), "orders/one.txt", Access::Write)
        .unwrap();
    assert_eq!(one.read(16).unwrap(), b"one");
    std::fs::remove_file(project.join("orders").join("one.txt")).unwrap();
    // First to a file inside the root, then to one outside it.
    for target in [
        project.join("other.txt"),
        folder.path().join("outside").join("secret.txt"),
    ] {
        link_file(&target, &project.join("orders").join("one.txt")).unwrap();
        assert_eq!(one.read(16).unwrap_err().code(), &codes::PATH_NOT_ALLOWED);
        assert_eq!(
            one.write(b"x").unwrap_err().code(),
            &codes::PATH_NOT_ALLOWED
        );
        std::fs::remove_file(project.join("orders").join("one.txt")).unwrap();
    }
    // A folder on the way swapped for a link to a folder inside the root.
    std::fs::create_dir_all(project.join("elsewhere")).unwrap();
    std::fs::write(project.join("elsewhere").join("one.txt"), b"elsewhere").unwrap();
    std::fs::rename(project.join("orders"), project.join("orders-moved")).unwrap();
    link_folder(&project.join("elsewhere"), &project.join("orders")).unwrap();
    assert_eq!(one.read(16).unwrap_err().code(), &codes::PATH_NOT_ALLOWED);
    assert_eq!(
        one.write(b"x").unwrap_err().code(),
        &codes::PATH_NOT_ALLOWED
    );
    assert_eq!(
        std::fs::read(project.join("elsewhere").join("one.txt")).unwrap(),
        b"elsewhere"
    );
    assert_eq!(
        std::fs::read(folder.path().join("outside").join("secret.txt")).unwrap(),
        b"secret"
    );
}

/// A folder made new is the caller's own: the call fails when anything is
/// at the name already.
#[test]
fn a_folder_is_made_new_or_not_at_all() {
    let (folder, roots) = scene();
    let project = folder.path().join("project");
    let orders = roots
        .check(&root("project"), "orders", Access::Write)
        .unwrap();
    let run = orders.join("run-1").unwrap();
    run.create_dir().unwrap();
    assert_eq!(run.entry_kind().unwrap(), EntryKind::Folder);
    assert!(
        std::fs::read_dir(project.join("orders").join("run-1"))
            .unwrap()
            .next()
            .is_none()
    );
    // A second time, over a file, and over the folder that holds it.
    for taken in [run.clone(), orders.join("one.txt").unwrap(), orders.clone()] {
        assert_eq!(
            taken.create_dir().unwrap_err().code(),
            &codes::FILE_UNAVAILABLE,
            "{taken}"
        );
    }
    // The folder above it must be there, and the path must allow writing.
    assert_eq!(
        orders
            .join("none/run-2")
            .unwrap()
            .create_dir()
            .unwrap_err()
            .code(),
        &codes::FILE_UNAVAILABLE
    );
    assert_eq!(
        roots
            .check(&root("project"), "orders/run-3", Access::Read)
            .unwrap()
            .create_dir()
            .unwrap_err()
            .code(),
        &codes::EFFECT_NOT_DECLARED
    );
    // Over a link, whatever it leads to: nothing is made where it leads.
    let outside = folder.path().join("outside").join("new-folder");
    if link_folder(&outside, &project.join("orders").join("run-4")).is_some() {
        // The link leads nowhere yet, so the path can still be made.
        if let Ok(planted) = orders.join("run-4") {
            assert!(planted.create_dir().is_err());
        }
        assert!(!outside.exists(), "a folder was made outside the root");
    }
}

/// A verified save, one step at a time (decision 0162): a temporary file
/// made new beside the target and handed back open, written, flushed, read
/// back by the caller, and only then put in place, replacing the target and
/// nothing else; a save dropped before that leaves nothing behind.
#[test]
fn a_verified_save_is_made_in_steps_the_caller_checks_between() {
    let (folder, roots) = scene();
    let project = folder.path().join("project").join("orders");
    let one = roots
        .check(&root("project"), "orders/one.txt", Access::Write)
        .unwrap();
    let before = one.identity().unwrap();
    assert!(one.may_write().unwrap());
    assert_eq!(std::fs::read(project.join("one.txt")).unwrap(), b"one");

    let mut save = one.create_temporary().unwrap();
    save.write(b"two, ").unwrap();
    save.write(b"checked").unwrap();
    save.flush().unwrap();
    assert_eq!(save.read_back(1024).unwrap(), b"two, checked");
    // Nothing is replaced while the caller checks.
    assert_eq!(std::fs::read(project.join("one.txt")).unwrap(), b"one");
    save.replace().unwrap();
    one.flush_folder().unwrap();
    assert_eq!(
        std::fs::read(project.join("one.txt")).unwrap(),
        b"two, checked"
    );
    // A file put in its place is another file.
    assert_ne!(one.identity().unwrap(), before);
    assert_eq!(one.identity().unwrap(), one.identity().unwrap());
    let only = |folder: &std::path::Path| {
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    assert_eq!(only(&project), vec!["one.txt".to_owned()]);

    // Dropped before it was put in place: the target is as it was, and the
    // temporary file is gone.
    let mut dropped = one.create_temporary().unwrap();
    dropped.write(b"never").unwrap();
    assert!(dropped.read_back(2).is_err());
    drop(dropped);
    assert_eq!(
        std::fs::read(project.join("one.txt")).unwrap(),
        b"two, checked"
    );
    assert_eq!(only(&project), vec!["one.txt".to_owned()]);

    // A missing folder, a missing file, and a path checked for reading are
    // each told apart.
    let nowhere = roots
        .check(&root("project"), "gone/one.txt", Access::Write)
        .unwrap();
    assert_eq!(
        nowhere.create_temporary().unwrap_err().code(),
        &codes::FILE_MISSING
    );
    let absent = roots
        .check(&root("project"), "orders/none.txt", Access::Write)
        .unwrap();
    assert_eq!(absent.may_write().unwrap_err().code(), &codes::FILE_MISSING);
    assert_eq!(absent.identity().unwrap_err().code(), &codes::FILE_MISSING);
    let read_only = roots
        .check(&root("project"), "orders/one.txt", Access::Read)
        .unwrap();
    assert_eq!(
        read_only.create_temporary().unwrap_err().code(),
        &codes::EFFECT_NOT_DECLARED
    );
}

/// Whether the engine's user may write a file is answered without writing
/// it; a file the user may not write says so.
#[cfg(unix)]
#[test]
fn may_write_writes_nothing_and_tells_a_file_the_user_may_not_write() {
    use std::os::unix::fs::PermissionsExt as _;
    let (folder, roots) = scene();
    let file = folder.path().join("project").join("orders").join("one.txt");
    let one = roots
        .check(&root("project"), "orders/one.txt", Access::Write)
        .unwrap();
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    assert!(one.may_write().unwrap());
    assert_eq!(std::fs::read(&file).unwrap(), b"one");
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        modified
    );
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
    // A user who may write anything, such as the superuser, is told so.
    let anyone = std::fs::OpenOptions::new().write(true).open(&file).is_ok();
    assert_eq!(one.may_write().unwrap(), anyone);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
}

/// The project folder of the run is the caller's, handed in checked; a run
/// with none stops by name when a pack asks for it.
#[test]
fn the_project_folder_is_the_callers_or_a_named_stop() {
    let (_folder, roots) = scene();
    let none = a2m_provider::CheckedPaths::none();
    assert!(none.project().is_none());
    assert_eq!(
        none.project_folder().unwrap_err().code(),
        &codes::PROJECT_FOLDER_MISSING
    );
    let project = roots.folder(&root("project"), Access::Write).unwrap();
    let paths = a2m_provider::CheckedPaths::none().with_project(project);
    let file = paths.project_folder().unwrap().join("orders/one.txt").unwrap();
    assert_eq!(file.read(16).unwrap(), b"one");
}
