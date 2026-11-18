//! What is checked about a program's file before it is ever started. The
//! check itself is the provider contract's, shared with every pack that
//! starts a program (decision 0250 moved it there from here): that the
//! file is there, is a program and not a script or a shell, and that
//! nobody but the engine's user, or the system's administrator, can
//! replace it or a folder on the way to it. This module names it in the
//! pack's own words and keeps the digest of a file.

use std::io::Read as _;
use std::path::Path;

use a2m_core::Digest;
pub(crate) use a2m_provider::{
    ProgramRefused as Refused, ProgramRole as Role, ProgramTrust as Trust, check_program as check,
};
use sha2::{Digest as _, Sha256};

/// The SHA-256 of a file, read in pieces.
///
/// # Errors
///
/// When the file cannot be read.
pub fn digest_of(path: &Path) -> std::io::Result<Digest> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Digest::from_bytes(hasher.finalize().into()))
}

#[cfg(all(test, unix))]
#[path = "safety/before_0250.rs"]
mod before;

/// The old check and the shared one, given every case the pack's own tests
/// of the check held before it moved, give the same verdict (decision
/// 0250). The cases are built on disk here as those tests built them.
#[cfg(all(test, unix))]
mod same_verdict {
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};

    use super::{Refused, Role, Trust, before};

    /// The old verdict in the new type, so that the two compare.
    fn old_in_new(verdict: Result<(), before::Refused>) -> Result<(), Refused> {
        verdict.map_err(|why| match why {
            before::Refused::NotAbsolute => Refused::NotAbsolute,
            before::Refused::Missing => Refused::Missing,
            before::Refused::Script => Refused::Script,
            before::Refused::Writable(path) => Refused::Writable(path),
            before::Refused::Unreadable(path) => Refused::Unreadable(path),
            before::Refused::Link(path) => Refused::Link(path),
            before::Refused::Elsewhere => Refused::Elsewhere,
        })
    }

    fn mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    struct Place(PathBuf);

    impl Drop for Place {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_shared_check_gives_the_verdict_the_packs_own_gave() {
        let root = std::env::temp_dir().join(format!(
            "a2m-pack-command-same-verdict-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let place = Place(std::fs::canonicalize(&root).unwrap());
        let at = |name: &str| place.0.join(name);
        mode(&place.0, 0o755);
        for (folder, bits) in [
            ("real", 0o755),
            ("open", 0o777),
            ("closed", 0o755),
            ("shared", 0o1777),
            ("grouped", 0o775),
        ] {
            std::fs::create_dir_all(at(folder)).unwrap();
            mode(&at(folder), bits);
        }
        let program = at("real").join("example-tool");
        std::fs::write(&program, [0x7f, b'E', b'L', b'F']).unwrap();
        mode(&program, 0o755);
        let script = at("real").join("example-script");
        std::fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
        mode(&script, 0o755);
        let writable = at("real").join("example-open");
        std::fs::write(&writable, [0x7f, b'E', b'L', b'F']).unwrap();
        mode(&writable, 0o777);
        let not_run = at("real").join("example-not-run");
        std::fs::write(&not_run, [0x7f, b'E', b'L', b'F']).unwrap();
        mode(&not_run, 0o644);
        let grouped = at("grouped").join("example-tool");
        std::fs::copy(&program, &grouped).unwrap();
        mode(&grouped, 0o775);
        let link = |target: &Path, name: &Path| std::os::unix::fs::symlink(target, name).unwrap();
        link(&program, &at("open").join("example-tool"));
        link(&at("real"), &at("open").join("through"));
        link(
            Path::new("../real/example-tool"),
            &at("closed").join("example-first"),
        );
        link(
            &at("closed").join("example-first"),
            &at("closed").join("example-tool"),
        );
        link(
            &at("closed").join("example-tool"),
            &at("open").join("example-far"),
        );
        link(&program, &at("shared").join("example-tool"));
        link(&at("closed").join("example-b"), &at("closed").join("example-a"));
        link(&at("closed").join("example-a"), &at("closed").join("example-b"));
        link(Path::new("/bin/sh"), &at("closed").join("example-shell"));
        let group = {
            use std::os::unix::fs::MetadataExt as _;
            std::fs::metadata(at("grouped")).unwrap().gid()
        };

        let cases: Vec<PathBuf> = vec![
            PathBuf::from("example-tool"),
            std::env::temp_dir().join("a2m-pack-command-no-such-program"),
            std::env::temp_dir(),
            PathBuf::from("/bin/sh"),
            program.clone(),
            script,
            writable,
            not_run,
            at("open").join("example-tool"),
            at("open").join("through").join("example-tool"),
            at("closed").join("example-tool"),
            at("open").join("example-far"),
            at("shared").join("example-tool"),
            at("closed").join("example-a"),
            at("closed").join("example-shell"),
            program.join("more"),
            grouped,
        ];
        let trusts = [
            (Trust::default(), before::Trust::default()),
            (
                Trust::with_groups(vec![group]),
                before::Trust::with_groups(vec![group]),
            ),
            (
                Trust::with_groups(vec![group.wrapping_add(1)]),
                before::Trust::with_groups(vec![group.wrapping_add(1)]),
            ),
        ];
        let roles = [
            (Role::Program, before::Role::Program),
            (Role::Interpreter, before::Role::Interpreter),
        ];
        let mut compared = 0;
        for path in &cases {
            for (new_trust, old_trust) in &trusts {
                for (new_role, old_role) in roles {
                    let new = super::check(path, new_trust, new_role);
                    let old = old_in_new(before::check(path, old_trust, old_role));
                    assert_eq!(new, old, "{} as {new_role:?}", path.display());
                    compared += 1;
                }
            }
        }
        assert_eq!(compared, cases.len() * 6);
    }
}
