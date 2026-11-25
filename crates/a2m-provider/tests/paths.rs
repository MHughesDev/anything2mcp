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
        .check(&root("inputs"), "a.txt", Access::Read)
        .unwrap_err();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Policy, &codes::EFFECT_NOT_DECLARED)
    );
    // A pack receives only the roots its manifest declares, and for reading
    // when it declares reading.
    let mut all = roots.clone();
    all.add(
        root("inputs"),
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
            .check(&root("inputs"), "secret.txt", Access::Read)
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
        &codes::PATH_MISSING
    );
    let absent = roots
        .check(&root("project"), "orders/none.txt", Access::Write)
        .unwrap();
    assert_eq!(absent.may_write().unwrap_err().code(), &codes::PATH_MISSING);
    assert_eq!(absent.identity().unwrap_err().code(), &codes::PATH_MISSING);
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
    let file = paths
        .project_folder()
        .unwrap()
        .join("orders/one.txt")
        .unwrap();
    assert_eq!(file.read(16).unwrap(), b"one");
}

/// A save's temporary file is private to the engine's user, takes the
/// target's owner, group, and mode through its own handle, and is removed
/// only while its name still holds the file it made (decision 0176).
#[cfg(unix)]
#[test]
fn a_temporary_file_is_private_keeps_the_targets_mode_and_removes_only_itself() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let (folder, roots) = scene();
    let orders = folder.path().join("project").join("orders");
    let file = orders.join("one.txt");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
    let one = roots
        .check(&root("project"), "orders/one.txt", Access::Write)
        .unwrap();
    let temporaries = || -> Vec<std::path::PathBuf> {
        std::fs::read_dir(&orders)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".a2m-new-")
            })
            .collect()
    };

    let mut save = one.create_temporary().unwrap();
    let made = temporaries();
    assert_eq!(made.len(), 1);
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o7777;
    assert_eq!(mode(&made[0]), 0o600);
    // Its identity, from its handle, is the identity its name gives, and
    // an open handle's identity compares with a path's.
    let named = roots
        .check(
            &root("project"),
            &format!("orders/{}", made[0].file_name().unwrap().to_string_lossy()),
            Access::Read,
        )
        .unwrap();
    assert_eq!(save.identity().unwrap(), named.identity().unwrap());
    let open = std::fs::File::open(&file).unwrap();
    assert_eq!(
        a2m_provider::FileIdentity::of_open(&open).unwrap(),
        one.identity().unwrap()
    );
    assert_ne!(save.identity().unwrap(), one.identity().unwrap());

    assert!(save.keep_owner_and_mode().unwrap());
    save.write(b"kept").unwrap();
    assert!(save.keep_owner_and_mode().unwrap());
    let target = std::fs::metadata(&file).unwrap();
    let made_meta = std::fs::metadata(&made[0]).unwrap();
    assert_eq!(mode(&made[0]), 0o640);
    assert_eq!(made_meta.uid(), target.uid());
    assert_eq!(made_meta.gid(), target.gid());
    save.flush().unwrap();
    save.replace().unwrap();
    assert_eq!(mode(&file), 0o640);
    assert_eq!(std::fs::read(&file).unwrap(), b"kept");

    // A target with a special bit is not copied: the answer says so.
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o1644)).unwrap();
    let mut special = one.create_temporary().unwrap();
    assert!(!special.keep_owner_and_mode().unwrap());
    drop(special);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();

    // Another file put at the temporary file's name is left alone when the
    // save is dropped.
    let save = one.create_temporary().unwrap();
    let name = temporaries().pop().unwrap();
    std::fs::remove_file(&name).unwrap();
    std::fs::write(&name, b"someone else's").unwrap();
    drop(save);
    assert_eq!(std::fs::read(&name).unwrap(), b"someone else's");
    std::fs::remove_file(&name).unwrap();
    assert!(temporaries().is_empty());
}

/// Decision 0193: a root that stands for each request's project folder
/// resolves in the project of the run at hand. One set of roots, two
/// projects: each check reads its own project's file and never reaches the
/// other's, and every escape is refused as for any root.
#[test]
fn a_root_of_each_requests_project_resolves_in_that_project_and_never_another() {
    let folder = Folder::new("project-roots");
    let mut paths = Vec::new();
    for name in ["alpha", "beta"] {
        let project = folder.path().join(name);
        std::fs::create_dir_all(project.join("stock")).unwrap();
        std::fs::write(project.join("stock").join("count.csv"), name.as_bytes()).unwrap();
        // The kernel's checked project folder of a run of that project.
        let mut own = Roots::none();
        own.add(root("project"), &project, Access::Read).unwrap();
        let checked = own.folder(&root("project"), Access::Read).unwrap();
        paths.push(a2m_provider::CheckedPaths::none().with_project(checked));
    }
    let mut roots = Roots::none();
    roots.add_project(root("files"), Access::Write).unwrap();
    assert!(roots.add_project(root("files"), Access::Read).is_err());
    assert!(roots.is_project(&root("files")));
    assert!(roots.holds(&root("files"), Access::Write));
    assert!(!roots.holds(&root("elsewhere"), Access::Read));
    assert_eq!(roots.names().collect::<Vec<_>>(), [&root("files")]);

    for (paths, name) in paths.iter().zip(["alpha", "beta"]) {
        let file = roots
            .check_in(&root("files"), "stock/count.csv", Access::Read, paths)
            .unwrap();
        assert_eq!(file.read(16).unwrap(), name.as_bytes());
        let whole = roots
            .folder_in(&root("files"), Access::Read, paths)
            .unwrap();
        assert_eq!(
            whole.join("stock/count.csv").unwrap().read(16).unwrap(),
            name.as_bytes()
        );
        for escape in [
            "../alpha/stock/count.csv",
            "../beta/stock/count.csv",
            "/etc/hosts",
            "C:/Windows/win.ini",
            "stock/../../beta/stock/count.csv",
        ] {
            let failure = roots
                .check_in(&root("files"), escape, Access::Read, paths)
                .unwrap_err();
            assert_eq!(failure.code(), &codes::PATH_NOT_ALLOWED, "{escape}");
        }
    }
    // A link in one project that leads to the other is refused.
    let link = folder.path().join("alpha").join("theirs");
    if link_folder(&folder.path().join("beta").join("stock"), &link).is_some() {
        let failure = roots
            .check_in(&root("files"), "theirs/count.csv", Access::Read, &paths[0])
            .unwrap_err();
        assert_eq!(failure.code(), &codes::PATH_NOT_ALLOWED);
    }

    // Without a project folder the run stops by name; the plain check of
    // such a root is refused the same way and resolves nothing.
    let none = a2m_provider::CheckedPaths::none();
    for failure in [
        roots
            .check_in(&root("files"), "stock/count.csv", Access::Read, &none)
            .unwrap_err(),
        roots
            .check(&root("files"), "stock/count.csv", Access::Read)
            .unwrap_err(),
        roots.folder(&root("files"), Access::Read).unwrap_err(),
    ] {
        assert_eq!(failure.code(), &codes::PROJECT_FOLDER_MISSING);
    }
    // A provider's subset keeps it as a root of each request's project.
    let narrowed = roots.narrowed(&[root("files")], &[]).unwrap();
    assert!(narrowed.is_project(&root("files")));
    assert!(!narrowed.holds(&root("files"), Access::Write));
    assert_eq!(
        narrowed
            .check_in(&root("files"), "stock/count.csv", Access::Read, &paths[1])
            .unwrap()
            .read(16)
            .unwrap(),
        b"beta"
    );
    // Another root keeps its own folder, whatever the run's project.
    let (_scene, mut fixed) = scene();
    fixed.add_project(root("files"), Access::Read).unwrap();
    assert_eq!(
        fixed
            .check_in(&root("project"), "orders/one.txt", Access::Read, &paths[0])
            .unwrap()
            .read(16)
            .unwrap(),
        b"one"
    );
}

/// A folder made private is the engine's user's alone from the moment it
/// exists, on every system, and the shared check finds it closed: mode
/// `0700` on Linux and macOS, a protected access list on Windows
/// (decision 0250). A folder made the ordinary way is left as the
/// process's defaults make it.
#[test]
fn a_private_folder_is_made_closed_to_other_users() {
    let (_folder, roots) = scene();
    let project = roots.folder(&root("project"), Access::Write).unwrap();
    let one = project.join("private").unwrap();
    one.create_private_dir().unwrap();
    let many = project.join("deep/er/still").unwrap();
    many.create_private_dir_all().unwrap();
    // Made new or not at all.
    assert!(one.create_private_dir().is_err());
    for made in [one.as_path().to_path_buf(), many.as_path().to_path_buf()] {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&made).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{}", made.display());
        }
        // Whatever the folders above it are, the folder itself passes the
        // check of a folder the engine made when its parent is closed; on
        // Windows that reads the access list just set.
        let verdict = a2m_provider::check_program(
            &made,
            &a2m_provider::ProgramTrust::default(),
            a2m_provider::ProgramRole::Folder,
        );
        // The temporary folder of a machine may itself be open to others
        // (it is, by design, on Linux, with the sticky mark, which the
        // check accepts); a refusal names a folder above this one.
        if let Err(a2m_provider::ProgramRefused::Writable(at)) = &verdict {
            assert!(
                !at.starts_with(&made),
                "the private folder itself is open: {}",
                at.display()
            );
        } else {
            assert_eq!(verdict, Ok(()), "{}", made.display());
        }
    }
}

/// A file put at the name after the caller's look is never the one removed:
/// a removal held to the file the caller looked at leaves another file at
/// the name where it is, and removes that file only while the name holds it.
#[test]
fn a_held_folder_removes_a_file_only_while_its_name_holds_the_file_looked_at() {
    let (folder, roots) = scene();
    let orders = roots
        .check(&root("project"), "orders", Access::Write)
        .unwrap();
    let held = orders.hold_folder().unwrap();
    let looked_at = held.identity("one.txt").unwrap();
    // Another program replaces the file after the look.
    let on_disk = folder.path().join("project").join("orders");
    std::fs::rename(on_disk.join("one.txt"), on_disk.join("kept.txt")).unwrap();
    std::fs::write(on_disk.join("one.txt"), b"another").unwrap();
    assert_ne!(held.identity("one.txt").unwrap(), looked_at);
    assert_eq!(
        held.remove_file_if("one.txt", looked_at, |_| true).unwrap(),
        a2m_provider::Removal::NotThatFile { put_back: true }
    );
    assert_eq!(std::fs::read(on_disk.join("one.txt")).unwrap(), b"another");
    let mut names: Vec<_> = std::fs::read_dir(&on_disk)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["kept.txt", "one.txt"],
        "nothing moved aside is left"
    );
    let now = held.identity("one.txt").unwrap();
    let holds = |wanted: &'static [u8]| {
        move |file: &mut std::fs::File| {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(file, &mut bytes).unwrap();
            bytes == wanted
        }
    };
    // The same file, and bytes other than the caller's: it stays.
    assert_eq!(
        held.remove_file_if("one.txt", now, holds(b"one")).unwrap(),
        a2m_provider::Removal::NotThatFile { put_back: true }
    );
    assert_eq!(
        held.remove_file_if("one.txt", now, holds(b"another"))
            .unwrap(),
        a2m_provider::Removal::Removed
    );
    assert!(!on_disk.join("one.txt").exists());
    assert_eq!(std::fs::read(on_disk.join("kept.txt")).unwrap(), b"one");
    assert!(
        held.remove_file_if("..", now, |_| true).is_err(),
        "one name only"
    );
}

/// A folder held and then swapped for a link to a folder outside the root
/// still takes every call: a new file, a second name, and a removal each
/// land in the folder that was held, and the folder the link leads to is
/// never touched, not even for a moment. The holder can tell the folder is
/// no longer at its path.
#[cfg(unix)]
#[test]
fn a_held_folder_swapped_for_a_link_takes_every_call_and_the_link_none() {
    use std::time::{Duration, UNIX_EPOCH};
    let (folder, roots) = scene();
    let project = folder.path().join("project");
    let outside = folder.path().join("outside");
    std::fs::create_dir(project.join("orders").join("empty")).unwrap();
    std::fs::create_dir(outside.join("empty")).unwrap();
    let orders = roots
        .check(&root("project"), "orders", Access::Write)
        .unwrap();
    let held = orders.hold_folder().unwrap();
    assert!(held.still_at_its_path());
    // A moment long past, which anything made or removed in the folder
    // outside would replace.
    let past = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    std::fs::File::open(&outside)
        .unwrap()
        .set_modified(past)
        .unwrap();
    std::fs::rename(project.join("orders"), project.join("orders-moved")).unwrap();
    std::os::unix::fs::symlink(&outside, project.join("orders")).unwrap();
    assert!(!held.still_at_its_path());
    drop(held.create_new("new.txt").unwrap());
    held.link("one.txt", "two.txt").unwrap();
    held.remove_empty_folder("empty").unwrap();
    let moved = project.join("orders-moved");
    assert!(moved.join("new.txt").is_file());
    assert_eq!(std::fs::read(moved.join("two.txt")).unwrap(), b"one");
    assert!(!moved.join("empty").exists());
    assert!(outside.join("empty").is_dir());
    assert_eq!(
        std::fs::metadata(&outside).unwrap().modified().unwrap(),
        past
    );
    // A folder reached through the link now is refused.
    assert_eq!(
        orders.hold_folder().unwrap_err().code(),
        &codes::PATH_NOT_ALLOWED
    );
}

/// Each place in a pack crate that resolves a path itself, with how many
/// times it does and why it is not the reading of a file a person or an
/// agent names. Only the check in this crate makes a `CheckedPath`
/// (section 502); a new use in a pack is either added here, with its
/// reason, or is a pack resolving a path, which fails this test.
const RESOLVES_A_PATH: &[(&str, usize, &str)] = &[
    (
        "a2m-pack-command/src/driver.rs",
        2,
        "a program the published target declares, resolved to name the file it runs, so the provider's closed tables of interpreters and of programs that run their own code are matched by that name too (decisions 0250 and 0299); nothing is read",
    ),
    (
        "a2m-pack-mcp/src/driver.rs",
        1,
        "the program of a declared server, resolved to name the file it runs, so a package runner is refused by that name too (decision 0299, point 5); nothing is read",
    ),
    (
        "a2m-pack-desktop/src/backend/linux.rs",
        2,
        "the declared program of a desktop application and the program a running process of the engine's own user runs (`/proc/<id>/exe`), compared to prove which process is the application; nothing is read",
    ),
    (
        "a2m-pack-command/src/safety/before_0250.rs",
        1,
        "a frozen copy of the check from before decision 0250, used by one test alone to compare verdicts",
    ),
    (
        "a2m-pack-command/src/safety.rs",
        1,
        "that test's own temporary folder",
    ),
    (
        "a2m-pack-command/tests/script.rs",
        2,
        "a test that finds the shell the machine has",
    ),
    (
        "a2m-pack-desktop/tests/read.rs",
        1,
        "a test that names the program it starts as the driver will see it",
    ),
    (
        "a2m-pack-mcp/tests/launcher.rs",
        1,
        "a test that finds the launcher the machine has",
    ),
];

/// The calls that turn a path into the file it leads to.
const RESOLVERS: &[&str] = &[
    "canonicalize",
    "read_link",
    "realpath",
    "GetFinalPathNameByHandleW",
];

fn rust_files(folder: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "target") {
                rust_files(&path, found);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// How many times `line` calls one of the resolvers, as a whole word.
fn resolutions(line: &str) -> usize {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    RESOLVERS
        .iter()
        .map(|name| {
            line.match_indices(name)
                .filter(|(at, _)| {
                    let before = line[..*at].chars().next_back();
                    let after = line[at + name.len()..].chars().next();
                    !before.is_some_and(word) && !after.is_some_and(word)
                })
                .count()
        })
        .sum()
}

#[test]
fn no_pack_resolves_a_path_but_where_the_list_says_why() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut found: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut packs = 0;
    for entry in std::fs::read_dir(&crates).unwrap() {
        let folder = entry.unwrap().path();
        let Some(name) = folder.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("a2m-pack-") {
            continue;
        }
        packs += 1;
        let mut files = Vec::new();
        rust_files(&folder, &mut files);
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            let count: usize = text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .map(resolutions)
                .sum();
            if count > 0 {
                let relative = file
                    .strip_prefix(&crates)
                    .unwrap()
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                found.insert(relative, count);
            }
        }
    }
    assert!(packs >= 10, "the pack crates were not found: {packs}");
    let allowed: std::collections::BTreeMap<String, usize> = RESOLVES_A_PATH
        .iter()
        .map(|(file, count, why)| {
            assert!(!why.is_empty());
            ((*file).to_owned(), *count)
        })
        .collect();
    assert_eq!(
        found, allowed,
        "a pack resolves a path where the list does not say why, or the list names a use that is gone"
    );
}

#[test]
fn the_search_for_a_resolution_finds_each_call_and_no_longer_name() {
    assert_eq!(
        resolutions("let real = std::fs::canonicalize(&path).ok();"),
        1
    );
    assert_eq!(
        resolutions("path.canonicalize()?; std::fs::read_link(p)"),
        2
    );
    assert_eq!(resolutions("tokio::fs::canonicalize(p).await"), 1);
    assert_eq!(resolutions("fn canonicalize_name(x: &str)"), 0);
    assert_eq!(resolutions("let uncanonicalized = 1;"), 0);
}

/// Every way in to a file's bytes, for reading and for replacing it, and
/// what each gave.
#[cfg(any(windows, target_os = "macos"))]
fn ways_in(file: &a2m_provider::CheckedPath) -> Vec<(&'static str, Result<(), a2m_core::Failure>)> {
    vec![
        ("open", file.open().map(drop)),
        ("read", file.read(1024).map(drop)),
        ("write", file.write(b"two")),
        ("save", file.create_temporary().map(drop)),
        ("may_write", file.may_write().map(drop)),
    ]
}

/// On Windows a file given the offline attribute, which a program of the
/// user may set, is a file whose bytes are not on this machine: it is
/// refused by every read and every write before anything is read, and is
/// left as it was (decision 0430).
#[cfg(windows)]
#[test]
#[allow(
    unsafe_code,
    reason = "two calls into the system, each explained where it is made"
)]
fn an_offline_file_is_neither_read_nor_replaced() {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_OFFLINE, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
    };
    let (folder, roots) = scene();
    let orders = folder.path().join("project").join("orders");
    let wide: Vec<u16> = orders
        .join("one.txt")
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    // SAFETY: `wide` is a path ending in a nul, alive for both calls.
    let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
    assert_ne!(attributes, INVALID_FILE_ATTRIBUTES);
    // SAFETY: as above.
    let set = unsafe { SetFileAttributesW(wide.as_ptr(), attributes | FILE_ATTRIBUTE_OFFLINE) };
    assert_ne!(set, 0, "the offline attribute could not be set");
    let one = roots
        .check(&root("project"), "orders/one.txt", Access::Write)
        .unwrap();
    assert_eq!(one.entry_kind().unwrap(), EntryKind::NotOnThisMachine);
    let found = std::fs::symlink_metadata(orders.join("one.txt")).unwrap();
    assert_eq!(EntryKind::of_metadata(&found), EntryKind::NotOnThisMachine);
    for (way, given) in ways_in(&one) {
        let failure = given.expect_err(way);
        assert_eq!(
            (failure.class(), failure.code()),
            (FailureClass::Policy, &codes::FILE_NOT_ON_THIS_MACHINE),
            "{way}: {failure:?}"
        );
        assert!(failure.message().contains("orders/one.txt"), "{way}");
    }
    // SAFETY: as above.
    let cleared = unsafe { SetFileAttributesW(wide.as_ptr(), attributes) };
    assert_ne!(cleared, 0);
    // The file is as it was, and nothing was left beside it.
    assert_eq!(std::fs::read(orders.join("one.txt")).unwrap(), b"one");
    let names: Vec<_> = std::fs::read_dir(&orders)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["one.txt"]);
    assert_eq!(one.entry_kind().unwrap(), EntryKind::File);
}

/// On macOS a dataless file is made only by a file provider: the kernel
/// refuses `SF_DATALESS` to `chflags`, even to the superuser. Where it
/// refuses, this proves the flags are read and a file written here is a
/// file; where a system ever allows it, the file is refused as on Windows
/// (decision 0430).
#[cfg(target_os = "macos")]
#[test]
#[allow(
    unsafe_code,
    reason = "two calls into the system, each explained where it is made"
)]
fn a_dataless_file_is_neither_read_nor_replaced_where_one_can_be_made() {
    use std::os::macos::fs::MetadataExt as _;
    use std::os::unix::ffi::OsStrExt as _;
    const SF_DATALESS: u32 = 0x4000_0000;
    let (folder, roots) = scene();
    let path = folder.path().join("project").join("orders").join("one.txt");
    let before = std::fs::symlink_metadata(&path).unwrap().st_flags();
    assert_eq!(before & SF_DATALESS, 0);
    let one = roots
        .check(&root("project"), "orders/one.txt", Access::Write)
        .unwrap();
    assert_eq!(one.entry_kind().unwrap(), EntryKind::File);
    let text = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `text` is a path ending in a nul, alive for the call.
    let set = unsafe { libc::chflags(text.as_ptr(), before | SF_DATALESS) };
    // The kernel may answer the call and still keep the flag off, so the
    // flags read back decide the case, not the call's answer.
    let after = std::fs::symlink_metadata(&path).unwrap().st_flags();
    if set != 0 || after & SF_DATALESS == 0 {
        // Refused, as the kernel refuses it: the file is as it was.
        assert_eq!(one.entry_kind().unwrap(), EntryKind::File);
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        assert_eq!(one.read(1024).unwrap(), b"one");
        return;
    }
    assert_eq!(one.entry_kind().unwrap(), EntryKind::NotOnThisMachine);
    for (way, given) in ways_in(&one) {
        let failure = given.expect_err(way);
        assert_eq!(
            (failure.class(), failure.code()),
            (FailureClass::Policy, &codes::FILE_NOT_ON_THIS_MACHINE),
            "{way}: {failure:?}"
        );
    }
    // SAFETY: as above.
    assert_eq!(unsafe { libc::chflags(text.as_ptr(), before) }, 0);
}
