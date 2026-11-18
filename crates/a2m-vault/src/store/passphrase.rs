//! The passphrase route: a root derived from a passphrase given at start.
//!
//! For a machine where this build has no operating-system key store, Linux
//! among them (decision 0123), and no key service. It is weaker than the
//! operating system's store, because anyone who learns the passphrase and
//! reads the store can open everything; `doctor` reports it as weaker.
//! Where the build has an operating-system key store, choosing this route
//! instead is an explicit opt-in that only the installation's settings can
//! make; where the build has none, it is the default, because there is
//! nothing stronger to move off (decision 0220). The derivation is
//! Argon2id, which costs memory as well as time for each guess (decision
//! 0074).
//!
//! What the passphrase protects against: a copy of the store, its key
//! records, and the salt, taken away from the machine or read while no
//! engine runs, opens nothing without it. What it does not: while the
//! engine runs, its process holds the derived root in memory, and a program
//! of the same user that can read that memory, or the environment the
//! passphrase was given in, can learn it.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use a2m_core::{Failure, Secret};
use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

use super::creation::{self, CreationLock};
use super::{BoxFuture, KeyStore, LocalRoot, StoreDescription, StoreHealth, StoreStrength};
use crate::failure;
use crate::material::{KeyMaterial, OsRandom, RandomSource, Wrapped};
use crate::names::Binding;

/// The value of the setting `vault.backend` that chooses the operating
/// system's key store.
pub const BACKEND_OS_KEY_STORE: &str = "os_keystore";

/// The value of the setting `vault.backend` that chooses the passphrase
/// route.
pub const BACKEND_PASSPHRASE: &str = "passphrase";

/// What `vault.backend` is when no settings file sets it: the operating
/// system's key store where this build has one, and the passphrase route
/// where it has none (decision 0220). A fact of the build, from the same
/// `cfg` that chooses the key store's back end.
pub const DEFAULT_BACKEND: &str = if super::OsKeyStore::IN_THIS_BUILD {
    BACKEND_OS_KEY_STORE
} else {
    BACKEND_PASSPHRASE
};

/// Whether the passphrase route is chosen. The caller reads it from the
/// setting `vault.backend`, which only the installation's settings file
/// can change, and passes it here, so that the route is taken only where
/// the settings, or the build's default, choose it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassphraseRoute {
    /// The route is off. Asking for it is refused.
    NotEnabled,
    /// The route is chosen.
    Enabled,
}

impl PassphraseRoute {
    /// The route that the value of `vault.backend` chooses: enabled for
    /// [`BACKEND_PASSPHRASE`] alone. `None`, a setting nobody could read,
    /// enables nothing.
    #[must_use]
    pub fn of_backend(backend: Option<&str>) -> Self {
        if backend == Some(BACKEND_PASSPHRASE) {
            Self::Enabled
        } else {
            Self::NotEnabled
        }
    }
}

/// The salt of the derivation: random, not secret, one per installation. It
/// is kept beside the key records, and the same one is given at every start.
/// [`PassphraseSalt::load_or_create`] keeps it in a file and makes it once.
#[derive(Clone, PartialEq, Eq)]
pub struct PassphraseSalt([u8; PassphraseSalt::LENGTH]);

impl PassphraseSalt {
    /// The number of bytes in a salt.
    pub const LENGTH: usize = 16;

    /// A new salt, for a new installation.
    ///
    /// # Errors
    ///
    /// `busy`/`randomness_unavailable` when the source fails.
    pub fn generate(random: &dyn RandomSource) -> Result<Self, Failure> {
        let mut salt = [0u8; Self::LENGTH];
        random.fill(&mut salt)?;
        Ok(Self(salt))
    }

    /// A salt read back from where it was kept.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    /// The salt's bytes, for keeping.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.0
    }

    /// The salt kept in `file`, made and written there when the file is
    /// missing. The directory is made when it is missing too.
    ///
    /// Two engines that start at the same moment on a fresh installation
    /// must end with one salt: two salts are two roots, and what was wrapped
    /// under the one that is overwritten never opens again. So a new salt is
    /// made under the creation lock, a lock file beside `file`; it is
    /// written whole under another name and moved into place, so that no
    /// reader sees half of one; and every engine uses what the file then
    /// holds. The lock file is `file` with `.lock` added.
    ///
    /// # Errors
    ///
    /// `busy`/`randomness_unavailable` when the source fails;
    /// `busy`/`key_store_unreachable` when the file cannot be read, made, or
    /// locked, or holds something that is not a salt.
    pub async fn load_or_create(file: &Path, random: &dyn RandomSource) -> Result<Self, Failure> {
        let unusable =
            || failure::unreachable("the salt of its passphrase could not be read or kept");
        if let Some(salt) = read_salt(file).map_err(|_| unusable())? {
            return Ok(salt);
        }
        // Made before the lock is taken, and used only if the file is still
        // missing once it is held.
        let made = Self::generate(random)?;
        let file = file.to_path_buf();
        tokio::task::spawn_blocking(move || make_salt(&file, &made))
            .await
            .map_err(|_| unusable())?
            .map_err(|_| unusable())
    }
}

/// The salt in `file`, or `None` when there is no such file.
fn read_salt(file: &Path) -> io::Result<Option<PassphraseSalt>> {
    match fs::read(file) {
        Ok(bytes) => <[u8; PassphraseSalt::LENGTH]>::try_from(bytes.as_slice())
            .map(|salt| Some(PassphraseSalt(salt)))
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Under the creation lock: the salt another engine wrote meanwhile, or
/// `made`, written whole and moved into place.
fn make_salt(file: &Path, made: &PassphraseSalt) -> io::Result<PassphraseSalt> {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder)?;
    }
    let _held = CreationLock::take(&creation::lock_file_beside(file))?;
    if let Some(salt) = read_salt(file)? {
        return Ok(salt);
    }
    let mut partial = file.as_os_str().to_owned();
    partial.push(".new");
    let mut writing = fs::File::create(&partial)?;
    writing.write_all(&made.0)?;
    writing.sync_all()?;
    drop(writing);
    fs::rename(&partial, file)?;
    read_salt(file)?.ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
}

impl fmt::Debug for PassphraseSalt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PassphraseSalt(16 bytes)")
    }
}

/// What one derivation costs.
#[derive(Debug, Clone, Copy)]
struct Cost {
    memory_kib: u32,
    passes: u32,
}

/// 64 MiB and three passes on one thread: the second recommended setting of
/// the Argon2 standard, with one lane so that the cost is the same on every
/// machine.
const STANDARD: Cost = Cost {
    memory_kib: 64 * 1024,
    passes: 3,
};

/// The root back end for a passphrase.
pub struct PassphraseKeyStore {
    root: LocalRoot,
    random: Arc<dyn RandomSource>,
}

impl PassphraseKeyStore {
    /// The fewest bytes a passphrase may have: 16.
    ///
    /// A length is all the engine can check, and it is no measure of
    /// strength. It refuses what falls to a short search however it was
    /// chosen, an empty passphrase first of all. A guess costs one
    /// derivation and can be checked away from this machine, against the
    /// root's identifier that every wrapped key carries, so nothing slows a
    /// search but the passphrase itself. Sixteen bytes lets through four
    /// unrelated words or sixteen random characters, and no short password.
    pub const SHORTEST_PASSPHRASE: usize = 16;

    /// The failure for a route that is chosen and given no passphrase:
    /// `input`/`vault_passphrase_missing`. It names `variable`, the
    /// environment variable a person sets, and says what the passphrase is
    /// for. The passphrase comes from a person or the person's environment,
    /// handed to the engine by the command that starts it, and never from a
    /// tool call, a request, or a project file.
    #[must_use]
    pub fn not_given(variable: &str) -> Failure {
        failure::passphrase_missing(variable)
    }

    /// Derives the root from `passphrase` and `salt`. This takes on the order
    /// of a second and 64 MiB of memory, on a blocking thread.
    ///
    /// # Errors
    ///
    /// `policy`/`vault_passphrase_not_enabled` when `route` is not
    /// [`PassphraseRoute::Enabled`]. `input`/`vault_passphrase_too_short`
    /// when the passphrase has fewer than
    /// [`PassphraseKeyStore::SHORTEST_PASSPHRASE`] bytes; nothing is derived
    /// from it. `busy`/`key_store_unreachable` when the derivation cannot
    /// run, as when the memory for it cannot be had.
    pub async fn derive(
        route: PassphraseRoute,
        passphrase: &Secret,
        salt: &PassphraseSalt,
    ) -> Result<Self, Failure> {
        Self::derive_at(route, passphrase, salt, STANDARD, Arc::new(OsRandom)).await
    }

    async fn derive_at(
        route: PassphraseRoute,
        passphrase: &Secret,
        salt: &PassphraseSalt,
        cost: Cost,
        random: Arc<dyn RandomSource>,
    ) -> Result<Self, Failure> {
        if route != PassphraseRoute::Enabled {
            return Err(failure::passphrase_not_enabled());
        }
        if passphrase.expose().len() < Self::SHORTEST_PASSPHRASE {
            return Err(failure::passphrase_too_short());
        }
        let passphrase = Zeroizing::new(passphrase.expose().as_bytes().to_vec());
        let salt = salt.0;
        let root = tokio::task::spawn_blocking(move || derive_root(&passphrase, &salt, cost))
            .await
            .map_err(|_| failure::unreachable("the passphrase derivation did not finish"))??;
        Ok(Self { root, random })
    }
}

/// Argon2id over the passphrase and salt: 32 bytes of key, then 16 bytes that
/// identify the root, so that a wrong passphrase is recognized as such.
fn derive_root(passphrase: &[u8], salt: &[u8], cost: Cost) -> Result<LocalRoot, Failure> {
    let cannot = || failure::unreachable("the passphrase derivation could not run");
    let mut derived = Zeroizing::new([0u8; KeyMaterial::LENGTH + 16]);
    let params =
        Params::new(cost.memory_kib, cost.passes, 1, Some(derived.len())).map_err(|_| cannot())?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase, salt, derived.as_mut_slice())
        .map_err(|_| cannot())?;
    LocalRoot::from_derived(derived.as_slice()).ok_or_else(cannot)
}

impl KeyStore for PassphraseKeyStore {
    fn describe(&self) -> StoreDescription {
        StoreDescription {
            name: "passphrase given at start".to_owned(),
            strength: StoreStrength::Passphrase,
        }
    }

    fn wrap<'a>(
        &'a self,
        key: &'a KeyMaterial,
        bound_to: &'a Binding,
    ) -> BoxFuture<'a, Result<Wrapped, Failure>> {
        Box::pin(async move { self.root.wrap(key, bound_to, self.random.as_ref()) })
    }

    fn unwrap<'a>(
        &'a self,
        wrapped: &'a Wrapped,
        bound_to: &'a Binding,
    ) -> BoxFuture<'a, Result<KeyMaterial, Failure>> {
        Box::pin(async move {
            self.root
                .unwrap(wrapped, bound_to, failure::passphrase_mismatch)
        })
    }

    fn health(&self) -> BoxFuture<'_, StoreHealth> {
        Box::pin(async { StoreHealth::Ok })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::{Generation, KeyId, Layer};
    use crate::store::creation::tests::Folder;

    /// The smallest cost the derivation accepts, so the tests are quick.
    const QUICK: Cost = Cost {
        memory_kib: 8,
        passes: 1,
    };

    fn binding() -> Binding {
        Binding {
            key: KeyId::root(),
            layer: Layer::Workspace,
            generation: Generation::FIRST,
        }
    }

    async fn store(passphrase: &str, salt: &PassphraseSalt) -> PassphraseKeyStore {
        PassphraseKeyStore::derive_at(
            PassphraseRoute::Enabled,
            &Secret::new(passphrase),
            salt,
            QUICK,
            Arc::new(OsRandom),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn the_route_must_be_switched_on() {
        let salt = PassphraseSalt::from_bytes([7; 16]);
        let refused = PassphraseKeyStore::derive(
            PassphraseRoute::NotEnabled,
            &Secret::new("a long passphrase"),
            &salt,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(refused.code(), &failure::VAULT_PASSPHRASE_NOT_ENABLED);
        assert_eq!(refused.class(), a2m_core::FailureClass::Policy);
    }

    #[tokio::test]
    async fn the_same_passphrase_and_salt_give_the_same_root_and_no_other_does() {
        let salt = PassphraseSalt::from_bytes([7; 16]);
        let key = KeyMaterial::generate(&OsRandom).unwrap();
        let first = store("a long passphrase", &salt).await;
        assert_eq!(first.describe().strength, StoreStrength::Passphrase);
        assert!(first.describe().strength.is_weaker());
        let wrapped = first.wrap(&key, &binding()).await.unwrap();
        drop(first);

        let again = store("a long passphrase", &salt).await;
        let unwrapped = again.unwrap(&wrapped, &binding()).await.unwrap();
        assert_eq!(unwrapped.expose(), key.expose());

        let wrong = store("another passphrase", &salt).await;
        let refused = wrong.unwrap(&wrapped, &binding()).await.unwrap_err();
        assert_eq!(refused.code(), &failure::VAULT_PASSPHRASE_MISMATCH);

        let other_salt = store("a long passphrase", &PassphraseSalt::from_bytes([8; 16])).await;
        let refused = other_salt.unwrap(&wrapped, &binding()).await.unwrap_err();
        assert_eq!(refused.code(), &failure::VAULT_PASSPHRASE_MISMATCH);
    }

    #[test]
    fn the_standard_cost_is_accepted_by_the_derivation() {
        assert!(Params::new(STANDARD.memory_kib, STANDARD.passes, 1, Some(48)).is_ok());
    }

    /// Audit finding V-4. An empty passphrase, and any shorter than the
    /// floor, is refused by name before anything is derived from it.
    #[tokio::test]
    async fn a_passphrase_that_is_empty_or_short_is_refused_by_name() {
        let salt = PassphraseSalt::from_bytes([7; 16]);
        for short in ["", "x", "fifteen bytes ."] {
            assert!(short.len() < PassphraseKeyStore::SHORTEST_PASSPHRASE);
            let refused =
                PassphraseKeyStore::derive(PassphraseRoute::Enabled, &Secret::new(short), &salt)
                    .await
                    .err()
                    .unwrap();
            assert_eq!(refused.code(), &failure::VAULT_PASSPHRASE_TOO_SHORT);
            assert_eq!(refused.class(), a2m_core::FailureClass::Input);
            assert!(short.len() < 2 || !refused.message().contains(short));
        }
        // The floor itself passes, and the switch is still asked first.
        store("sixteen bytes ..", &salt).await;
        let off = PassphraseKeyStore::derive(PassphraseRoute::NotEnabled, &Secret::new(""), &salt)
            .await
            .err()
            .unwrap();
        assert_eq!(off.code(), &failure::VAULT_PASSPHRASE_NOT_ENABLED);
    }

    /// Audit finding V-1 on the passphrase route. The root is derived, so
    /// two engines can only differ by their salt. Eight engines start at
    /// once on a fresh installation: they end with one salt and one root.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn engines_that_start_at_once_on_the_passphrase_route_end_with_one_root() {
        let folder = Folder::new();
        let file = folder.0.join("fresh").join("vault.salt");
        let engines: Vec<_> = (0..8)
            .map(|_| {
                let file = file.clone();
                tokio::spawn(async move {
                    let salt = PassphraseSalt::load_or_create(&file, &OsRandom)
                        .await
                        .unwrap();
                    let engine = store("a long passphrase", &salt).await;
                    let key = KeyMaterial::generate(&OsRandom).unwrap();
                    let wrapped = engine.wrap(&key, &binding()).await.unwrap();
                    (salt, engine, key, wrapped)
                })
            })
            .collect();
        let mut started = Vec::new();
        for engine in engines {
            started.push(engine.await.unwrap());
        }
        let kept = fs::read(&file).unwrap();
        for (salt, engine, _, _) in &started {
            assert_eq!(salt.as_bytes().as_slice(), kept.as_slice());
            for (_, _, key, wrapped) in &started {
                let unwrapped = engine.unwrap(wrapped, &binding()).await;
                assert!(unwrapped.is_ok_and(|opened| opened.expose() == key.expose()));
            }
        }
        // A later start reads the same salt and writes nothing.
        let again = PassphraseSalt::load_or_create(&file, &OsRandom)
            .await
            .unwrap();
        assert_eq!(again.as_bytes().as_slice(), kept.as_slice());
    }

    #[tokio::test]
    async fn a_file_that_is_not_a_salt_is_refused_and_left_as_it_is() {
        let folder = Folder::new();
        let file = folder.0.join("vault.salt");
        fs::write(&file, b"not sixteen bytes").unwrap();
        let refused = PassphraseSalt::load_or_create(&file, &OsRandom)
            .await
            .err()
            .unwrap();
        assert_eq!(refused.code(), &failure::KEY_STORE_UNREACHABLE);
        assert_eq!(fs::read(&file).unwrap(), b"not sixteen bytes");
    }

    /// The environment variable that turns the test below it into the child
    /// of the test below that: it names the salt file to read or make.
    const CHILD_SALT_FILE: &str = "A2M_VAULT_TEST_CHILD_SALT_FILE";

    /// Waits for `met`, which another process brings about. A condition that
    /// never comes fails the test instead of hanging it.
    fn until(met: impl Fn() -> bool) {
        let give_up = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while !met() {
            assert!(
                std::time::Instant::now() < give_up,
                "the condition the test waits for never came"
            );
            std::thread::yield_now();
        }
    }

    /// One engine of the two-process test. Run on its own it does nothing.
    #[tokio::test]
    async fn child_reads_or_makes_the_salt() {
        let Some(file) = std::env::var_os(CHILD_SALT_FILE) else {
            return;
        };
        let file = std::path::PathBuf::from(file);
        let folder = file.parent().unwrap().to_path_buf();
        fs::write(folder.join(format!("ready-{}", std::process::id())), b"").unwrap();
        until(|| folder.join("go").exists());
        let salt = PassphraseSalt::load_or_create(&file, &OsRandom)
            .await
            .unwrap();
        println!("salt={}", crate::hex::encode(salt.as_bytes()));
    }

    /// The creation lock between two processes, which is what it is for:
    /// two engines started at the same moment on a fresh installation.
    #[test]
    fn two_processes_that_start_at_once_end_with_one_salt() {
        let this = std::env::current_exe().unwrap();
        for _ in 0..4 {
            let folder = Folder::new();
            let file = folder.0.join("vault.salt");
            let children: Vec<_> = (0..2)
                .map(|_| {
                    std::process::Command::new(&this)
                        .args([
                            "--exact",
                            "store::passphrase::tests::child_reads_or_makes_the_salt",
                            "--nocapture",
                        ])
                        .env(CHILD_SALT_FILE, &file)
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .unwrap()
                })
                .collect();
            let ready = || {
                fs::read_dir(&folder.0).is_ok_and(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .filter(|entry| entry.file_name().to_string_lossy().starts_with("ready-"))
                        .count()
                        == 2
                })
            };
            until(ready);
            fs::write(folder.0.join("go"), b"").unwrap();

            let kept = |output: std::process::Output| {
                assert!(output.status.success());
                let printed = String::from_utf8_lossy(&output.stdout).into_owned();
                let salt = printed.lines().find_map(|line| {
                    let at = line.find("salt=")?;
                    line.get(at + 5..at + 5 + 2 * PassphraseSalt::LENGTH)
                });
                salt.map(str::to_owned).unwrap()
            };
            let salts: Vec<String> = children
                .into_iter()
                .map(|child| kept(child.wait_with_output().unwrap()))
                .collect();
            assert_eq!(salts[0], salts[1]);
            assert_eq!(salts[0], crate::hex::encode(&fs::read(&file).unwrap()));
        }
    }
}
