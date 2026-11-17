//! The creation lock: what two engines hold, one at a time, to make a thing
//! that must be made once.
//!
//! The operating systems' key stores have no way to write an entry only when
//! there is none: a write replaces what is there. Two engines that start at
//! the same moment on a fresh installation would each find no root, each
//! write one, and the root written first would be gone with everything
//! wrapped under it. So the root is made under a lock outside the key store:
//! an exclusive operating-system lock on a file. The engine that holds it
//! reads the key store again, and writes only when it is still empty; the
//! engine that waited reads what the first one wrote.
//!
//! The lock is the operating system's, on an open file. It is released when
//! the file is closed, and so when the process ends, however it ends; a lock
//! file that merely exists means nothing and is never removed. The file
//! holds no byte.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use a2m_core::Digest;

/// The creation lock, held. Dropping it releases it.
#[derive(Debug)]
pub(crate) struct CreationLock {
    _file: File,
}

impl CreationLock {
    /// Takes the lock on `file`, making the file when it is missing, and
    /// waits for an engine that holds it. This blocks: call it on a blocking
    /// thread.
    pub(crate) fn take(file: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(file)?;
        file.lock()?;
        Ok(Self { _file: file })
    }
}

/// Where the lock for a thing named `name` is: a file in this user's
/// temporary directory, which every engine of the user agrees on whatever
/// data directory it was started with. The name is digested, so that it may
/// hold any character.
pub(crate) fn lock_file_for(kind: &str, name: &str) -> PathBuf {
    let digest = Digest::of(name.as_bytes()).to_string();
    let short = digest.get(..16).unwrap_or(&digest);
    std::env::temp_dir().join(format!("anything2mcp-{kind}-{short}.lock"))
}

/// The lock file that stands beside `file`: the same name with `.lock`
/// added.
pub(crate) fn lock_file_beside(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    use super::*;

    /// A folder of the test's own under the machine's temporary folder,
    /// removed when the test is done with it.
    pub(crate) struct Folder(pub(crate) PathBuf);

    impl Folder {
        pub(crate) fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let name = format!(
                "a2m-vault-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            );
            let path = std::env::temp_dir().join(name);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn one_holder_at_a_time_and_a_dead_holder_holds_nothing() {
        let folder = Folder::new();
        let file = folder.0.join("made-once.lock");
        let inside = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (file, inside, most) = (file.clone(), Arc::clone(&inside), Arc::clone(&most));
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        let lock = CreationLock::take(&file).unwrap();
                        let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        std::thread::yield_now();
                        inside.fetch_sub(1, Ordering::SeqCst);
                        drop(lock);
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 1);

        // The file is still there, held by nobody: it is taken at once.
        assert!(file.exists());
        drop(CreationLock::take(&file).unwrap());
    }

    #[test]
    fn a_lock_file_has_a_plain_name_whatever_the_thing_is_called() {
        let odd = lock_file_for("root", "an installation / with: odd * characters");
        let name = odd.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("anything2mcp-root-") && name.ends_with(".lock"));
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        );
        assert_ne!(odd, lock_file_for("root", "another installation"));
        assert_eq!(odd, lock_file_for("root", "an installation / with: odd * characters"));
        let beside = lock_file_beside(&PathBuf::from("data").join("vault.salt"));
        assert_eq!(beside, PathBuf::from("data").join("vault.salt.lock"));
    }
}
