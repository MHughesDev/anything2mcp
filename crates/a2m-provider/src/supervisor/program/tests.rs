//! The tests of the check of who may change a program, moved here with the
//! check from the command pack (decision 0250), and the tests of the closed
//! lists kept beside it.
use std::path::Path;

use super::{ProgramRefused as Refused, ProgramRole as Role, ProgramTrust as Trust};

/// The check of a program, with no group named.
fn check(path: &Path) -> Result<(), Refused> {
    super::check_program(path, &Trust::default(), Role::Program)
}

#[test]
fn a_relative_path_a_missing_file_and_a_shell_are_refused() {
    assert_eq!(check(Path::new("example-tool")), Err(Refused::NotAbsolute));
    let missing = std::env::temp_dir().join("a2m-provider-no-such-program");
    assert_eq!(check(&missing), Err(Refused::Missing));
    assert_eq!(check(&std::env::temp_dir()), Err(Refused::Missing));
    #[cfg(unix)]
    assert_eq!(check(Path::new("/bin/sh")), Err(Refused::Script));
}

#[cfg(unix)]
#[test]
fn a_script_and_a_file_others_can_write_are_refused() {
    use std::os::unix::fs::PermissionsExt as _;

    let folder =
        std::env::temp_dir().join(format!("a2m-provider-check-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let script = folder.join("example-tool");
    std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(check(&script), Err(Refused::Script));

    let open = folder.join("example-open");
    std::fs::write(&open, [0x7f, b'E', b'L', b'F']).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(
        matches!(check(&open), Err(Refused::Writable(path)) if path.ends_with("example-open"))
    );

    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(check(&open), Err(Refused::Missing), "it cannot be run");
    std::fs::remove_dir_all(&folder).unwrap();
}

/// A folder of the test's own with a program in `real/`, and a way to
/// make more folders with a mode.
#[cfg(unix)]
struct Place(std::path::PathBuf);

#[cfg(unix)]
impl Place {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "a2m-provider-check-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // As the system names it, so that a refusal's path can be
        // compared: the temporary folder is behind a link on macOS.
        let place = Self(std::fs::canonicalize(&root).unwrap());
        place.folder("", 0o755);
        place.folder("real", 0o755);
        let program = place.0.join("real").join("example-tool");
        std::fs::write(&program, [0x7f, b'E', b'L', b'F']).unwrap();
        Self::mode(&program, 0o755);
        place
    }

    fn mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn folder(&self, name: &str, mode: u32) -> std::path::PathBuf {
        let folder = self.0.join(name);
        std::fs::create_dir_all(&folder).unwrap();
        Self::mode(&folder, mode);
        folder
    }

    fn link(target: &Path, at: &Path) {
        std::os::unix::fs::symlink(target, at).unwrap();
    }
}

#[cfg(unix)]
impl Drop for Place {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[test]
fn a_link_in_a_folder_others_can_write_is_refused_and_a_closed_chain_is_followed() {
    let place = Place::new("links");
    let program = place.0.join("real").join("example-tool");
    assert_eq!(check(&program), Ok(()));

    // The declared path is a link in a folder anyone may write: whoever
    // can write the folder chooses where the link leads.
    let open = place.folder("open", 0o777);
    Place::link(&program, &open.join("example-tool"));
    assert_eq!(
        check(&open.join("example-tool")),
        Err(Refused::Writable(open.clone()))
    );
    // The same for an open folder that the path only passes through.
    Place::link(&place.0.join("real"), &open.join("through"));
    assert_eq!(
        check(&open.join("through").join("example-tool")),
        Err(Refused::Writable(open.clone()))
    );

    // A chain that nobody else can write is followed to the file: a
    // link to a link, one of them relative and through `..`.
    let closed = place.folder("closed", 0o755);
    Place::link(
        Path::new("../real/example-tool"),
        &closed.join("example-first"),
    );
    Place::link(&closed.join("example-first"), &closed.join("example-tool"));
    assert_eq!(check(&closed.join("example-tool")), Ok(()));
    // The folder the first link is in is opened afterwards: refused.
    Place::link(&closed.join("example-tool"), &open.join("example-far"));
    assert!(matches!(
        check(&open.join("example-far")),
        Err(Refused::Writable(_))
    ));

    // Where only an entry's owner may replace it, a link of ours holds.
    let shared = place.folder("shared", 0o1777);
    Place::link(&program, &shared.join("example-tool"));
    assert_eq!(check(&shared.join("example-tool")), Ok(()));

    // Links that lead in a circle lead nowhere, and a link to a shell
    // is a shell.
    Place::link(&closed.join("example-b"), &closed.join("example-a"));
    Place::link(&closed.join("example-a"), &closed.join("example-b"));
    assert_eq!(check(&closed.join("example-a")), Err(Refused::Missing));
    Place::link(Path::new("/bin/sh"), &closed.join("example-shell"));
    assert_eq!(check(&closed.join("example-shell")), Err(Refused::Script));
    // A file is the end of a path.
    assert_eq!(check(&program.join("more")), Err(Refused::Missing));
}

#[cfg(unix)]
#[test]
fn a_folder_or_a_file_a_group_can_write_is_refused_unless_the_group_counts() {
    let place = Place::new("group");
    let folder = place.folder("grouped", 0o775);
    let program = folder.join("example-tool");
    std::fs::copy(place.0.join("real").join("example-tool"), &program).unwrap();
    Place::mode(&program, 0o755);
    assert_eq!(check(&program), Err(Refused::Writable(folder.clone())));
    Place::mode(&folder, 0o755);
    assert_eq!(check(&program), Ok(()));
    Place::mode(&program, 0o775);
    assert_eq!(check(&program), Err(Refused::Writable(program.clone())));
}

/// A group-writable folder with a program in it, and the folder's group.
#[cfg(unix)]
fn grouped(place: &Place) -> (std::path::PathBuf, std::path::PathBuf, u32) {
    use std::os::unix::fs::MetadataExt as _;
    let folder = place.folder("grouped", 0o775);
    let program = folder.join("example-tool");
    std::fs::copy(place.0.join("real").join("example-tool"), &program).unwrap();
    Place::mode(&program, 0o775);
    let group = std::fs::metadata(&folder).unwrap().gid();
    (folder, program, group)
}

#[cfg(unix)]
#[test]
fn a_group_the_person_names_counts_as_the_administrators_and_no_other_does() {
    let place = Place::new("named");
    let (folder, program, group) = grouped(&place);
    let named =
        |groups: Vec<u32>| super::check_program(&program, &Trust::with_groups(groups), Role::Program);
    // Named: the folder and the file that group may write are started.
    assert_eq!(named(vec![group]), Ok(()));
    // Not named, or another group named: refused, as before.
    assert_eq!(named(vec![]), Err(Refused::Writable(folder.clone())));
    assert_eq!(
        named(vec![group.wrapping_add(1)]),
        Err(Refused::Writable(folder.clone()))
    );
    // A group's name never stands for it: only the system's own list
    // and the setting's numbers do. Others' write is refused whatever
    // group is named.
    Place::mode(&folder, 0o777);
    assert_eq!(named(vec![group]), Err(Refused::Writable(folder.clone())));
}

#[cfg(target_os = "linux")]
#[test]
fn on_linux_no_group_counts_by_default() {
    let place = Place::new("linux");
    let (folder, program, _) = grouped(&place);
    assert_eq!(check(&program), Err(Refused::Writable(folder)));
}

#[cfg(target_os = "macos")]
#[test]
fn on_macos_the_administrators_group_counts_and_staff_does_not() {
    let place = Place::new("macos");
    let (folder, program, _) = grouped(&place);
    // Giving a file to a group needs no privilege when the test's user
    // is in it; the runner's user is an administrator, but a machine
    // where it is not cannot prove this.
    for (group, path) in [(80, &folder), (80, &program)] {
        if let Err(error) = std::os::unix::fs::chown(path, None, Some(group)) {
            println!("skipped: this user cannot give a file to the group {group}: {error}");
            return;
        }
    }
    assert_eq!(check(&program), Ok(()));
    if let Err(error) = std::os::unix::fs::chown(&folder, None, Some(20)) {
        println!("skipped: this user cannot give a file to the group 20: {error}");
        return;
    }
    assert_eq!(check(&program), Err(Refused::Writable(folder)));
}

