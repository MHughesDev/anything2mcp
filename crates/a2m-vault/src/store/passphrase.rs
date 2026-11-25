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
///
/// It carries the cost its root is derived with (decision 0438): a root is
/// always derived with the cost the salt's file records, never with the
/// build's constant, which is the cost of a new salt only.
#[derive(Clone, PartialEq, Eq)]
pub struct PassphraseSalt {
    salt: [u8; PassphraseSalt::LENGTH],
    cost: Cost,
}

impl PassphraseSalt {
    /// The number of bytes in a salt.
    pub const LENGTH: usize = 16;

    /// A new salt, for a new installation, with this build's cost.
    ///
    /// # Errors
    ///
    /// `busy`/`randomness_unavailable` when the source fails.
    pub fn generate(random: &dyn RandomSource) -> Result<Self, Failure> {
        let mut salt = [0u8; Self::LENGTH];
        random.fill(&mut salt)?;
        Ok(Self {
            salt,
            cost: STANDARD,
        })
    }

    /// A salt read back from where it was kept as its bytes alone, as every
    /// build before decision 0438 kept it: its cost is that of those builds,
    /// 64 MiB and three passes on one lane.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::LENGTH]) -> Self {
        Self {
            salt: bytes,
            cost: STANDARD,
        }
    }

    /// The salt's bytes, for keeping.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; Self::LENGTH] {
        &self.salt
    }

    /// The salt kept in `file`, made and written there when the file is
    /// missing. The directory is made when it is missing too.
    ///
    /// The file is read in two forms (decision 0438). Sixteen bare bytes, as
    /// every earlier build wrote, are format 0, read with the cost of those
    /// builds and never rewritten. Anything else is one line of text that
    /// records the format, the algorithm and its version, the memory, the
    /// passes, the lanes, and the salt; a new file is always written that
    /// way, with this build's cost.
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
    /// locked, or holds something that is not a salt; and, with a next step
    /// that names a build that reads it, when it records a format, an
    /// algorithm, or a cost this build does not derive with. A refused file
    /// is left as it is.
    pub async fn load_or_create(file: &Path, random: &dyn RandomSource) -> Result<Self, Failure> {
        if let Some(salt) = read_salt(file)? {
            return Ok(salt);
        }
        // Made before the lock is taken, and used only if the file is still
        // missing once it is held.
        let made = Self::generate(random)?;
        let file = file.to_path_buf();
        tokio::task::spawn_blocking(move || make_salt(&file, &made))
            .await
            .map_err(|_| unusable())?
    }

    /// The file's text in the recorded form, format 1: one line, ended by a
    /// line feed.
    fn to_record(&self) -> String {
        format!(
            "{FIELD_FORMAT}={RECORDED_FORMAT} {FIELD_ALGORITHM}={ALGORITHM} {FIELD_VERSION}={VERSION} {FIELD_MEMORY}={} {FIELD_PASSES}={} {FIELD_LANES}={} {FIELD_SALT}={}\n",
            self.cost.memory_kib,
            self.cost.passes,
            self.cost.lanes,
            crate::hex::encode(&self.salt),
        )
    }

    /// What a salt file holds: format 0 for sixteen bare bytes, else the
    /// recorded form.
    fn from_file(bytes: &[u8]) -> Result<Self, Failure> {
        if let Ok(salt) = <[u8; Self::LENGTH]>::try_from(bytes) {
            return Ok(Self::from_bytes(salt));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| unusable())?;
        let line = text.strip_suffix('\n').ok_or_else(unusable)?;
        let mut fields = line.split(' ');
        let mut next = |name: &str| {
            fields
                .next()
                .and_then(|field| field.strip_prefix(name))
                .and_then(|field| field.strip_prefix('='))
                .ok_or_else(unusable)
        };
        // The format is read first: a later format may lay out the rest
        // another way, and is refused as one this build does not read.
        if next(FIELD_FORMAT)? != RECORDED_FORMAT {
            return Err(failure::salt_unsupported());
        }
        if next(FIELD_ALGORITHM)? != ALGORITHM || next(FIELD_VERSION)? != VERSION {
            return Err(failure::salt_unsupported());
        }
        let memory_kib = decimal(next(FIELD_MEMORY)?)?;
        let passes = decimal(next(FIELD_PASSES)?)?;
        let lanes = decimal(next(FIELD_LANES)?)?;
        let salt = next(FIELD_SALT)?;
        if fields.next().is_some() {
            return Err(unusable());
        }
        let salt = crate::hex::decode(salt)
            .and_then(|salt| <[u8; Self::LENGTH]>::try_from(salt).ok())
            .ok_or_else(unusable)?;
        let cost = Cost {
            memory_kib,
            passes,
            lanes,
        };
        if !cost.is_supported() {
            return Err(failure::salt_unsupported());
        }
        Ok(Self { salt, cost })
    }
}

/// The format a new salt file is written in.
const RECORDED_FORMAT: &str = "1";
/// The one algorithm this build derives with, and its version.
const ALGORITHM: &str = "argon2id";
const VERSION: &str = "0x13";
/// The fields of format 1, in the order they are written and read.
const FIELD_FORMAT: &str = "format";
const FIELD_ALGORITHM: &str = "algorithm";
const FIELD_VERSION: &str = "version";
const FIELD_MEMORY: &str = "memory_kib";
const FIELD_PASSES: &str = "passes";
const FIELD_LANES: &str = "lanes";
const FIELD_SALT: &str = "salt";

/// The failure for a salt file that cannot be read or kept, or holds
/// something that is not a salt.
fn unusable() -> Failure {
    failure::unreachable("the salt of its passphrase could not be read or kept")
}

/// A number in a salt file: decimal digits alone, at least one.
fn decimal(text: &str) -> Result<u32, Failure> {
    if text.is_empty() || !text.bytes().all(|digit| digit.is_ascii_digit()) {
        return Err(unusable());
    }
    text.parse().map_err(|_| unusable())
}

/// The salt in `file`, or `None` when there is no such file.
fn read_salt(file: &Path) -> Result<Option<PassphraseSalt>, Failure> {
    match fs::read(file) {
        Ok(bytes) => PassphraseSalt::from_file(&bytes).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(unusable()),
    }
}

/// Under the creation lock: the salt another engine wrote meanwhile, or
/// `made`, written whole and moved into place.
fn make_salt(file: &Path, made: &PassphraseSalt) -> Result<PassphraseSalt, Failure> {
    let written = || -> io::Result<()> {
        let mut partial = file.as_os_str().to_owned();
        partial.push(".new");
        let mut writing = fs::File::create(&partial)?;
        writing.write_all(made.to_record().as_bytes())?;
        writing.sync_all()?;
        drop(writing);
        fs::rename(&partial, file)
    };
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder).map_err(|_| unusable())?;
    }
    let _held = CreationLock::take(&creation::lock_file_beside(file)).map_err(|_| unusable())?;
    if let Some(salt) = read_salt(file)? {
        return Ok(salt);
    }
    written().map_err(|_| unusable())?;
    read_salt(file)?.ok_or_else(unusable)
}

impl fmt::Debug for PassphraseSalt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PassphraseSalt(16 bytes, {ALGORITHM} {} KiB, {} passes, {} lanes)",
            self.cost.memory_kib, self.cost.passes, self.cost.lanes
        )
    }
}

/// What one derivation costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cost {
    memory_kib: u32,
    passes: u32,
    lanes: u32,
}

impl Cost {
    /// The most memory a recorded cost may ask for: 4 GiB.
    const MOST_MEMORY_KIB: u32 = 4 * 1024 * 1024;
    /// The most passes, and the most lanes, a recorded cost may ask for.
    const MOST_PASSES: u32 = 64;
    const MOST_LANES: u32 = 16;

    /// Whether this build derives with this cost: one the algorithm accepts,
    /// and no larger than this build will spend on one start. A file that
    /// asks for more is refused, not derived with less.
    fn is_supported(self) -> bool {
        self.memory_kib <= Self::MOST_MEMORY_KIB
            && (1..=Self::MOST_PASSES).contains(&self.passes)
            && (1..=Self::MOST_LANES).contains(&self.lanes)
            && self.params().is_ok()
    }

    fn params(self) -> Result<Params, argon2::Error> {
        Params::new(
            self.memory_kib,
            self.passes,
            self.lanes,
            Some(KeyMaterial::LENGTH + 16),
        )
    }
}

/// 64 MiB and three passes on one thread: the second recommended setting of
/// the Argon2 standard, with one lane so that the cost is the same on every
/// machine. The cost of a new salt file, and of a file of sixteen bare
/// bytes, which every build before decision 0438 derived with. Changing it
/// changes new installations only.
const STANDARD: Cost = Cost {
    memory_kib: 64 * 1024,
    passes: 3,
    lanes: 1,
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

    /// Derives the root from `passphrase` and `salt`, with the cost `salt`
    /// records (decision 0438). At this build's cost this takes on the order
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
        if route != PassphraseRoute::Enabled {
            return Err(failure::passphrase_not_enabled());
        }
        if passphrase.expose().len() < Self::SHORTEST_PASSPHRASE {
            return Err(failure::passphrase_too_short());
        }
        let passphrase = Zeroizing::new(passphrase.expose().as_bytes().to_vec());
        let PassphraseSalt { salt, cost } = salt.clone();
        let root = tokio::task::spawn_blocking(move || derive_root(&passphrase, &salt, cost))
            .await
            .map_err(|_| failure::unreachable("the passphrase derivation did not finish"))??;
        Ok(Self {
            root,
            random: Arc::new(OsRandom),
        })
    }
}

/// Argon2id over the passphrase and salt, at `cost`: 32 bytes of key, then
/// 16 bytes that identify the root, so that a wrong passphrase is recognized
/// as such.
fn derive_root(passphrase: &[u8], salt: &[u8], cost: Cost) -> Result<LocalRoot, Failure> {
    let cannot = || failure::unreachable("the passphrase derivation could not run");
    let mut derived = Zeroizing::new([0u8; KeyMaterial::LENGTH + 16]);
    let params = cost.params().map_err(|_| cannot())?;
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
    use crate::names::{KeyId, KeyRotationNumber, Layer};
    use crate::store::creation::tests::Folder;

    /// The smallest cost the derivation accepts, so the tests are quick.
    const QUICK: Cost = Cost {
        memory_kib: 8,
        passes: 1,
        lanes: 1,
    };

    /// `salt` with the quick cost, so that a test that is about the salt
    /// does not spend a full derivation on it.
    fn quick(salt: &PassphraseSalt) -> PassphraseSalt {
        PassphraseSalt {
            salt: salt.salt,
            cost: QUICK,
        }
    }

    fn binding() -> Binding {
        Binding {
            key: KeyId::root(),
            layer: Layer::Workspace,
            rotation: KeyRotationNumber::FIRST,
        }
    }

    async fn store(passphrase: &str, salt: &PassphraseSalt) -> PassphraseKeyStore {
        PassphraseKeyStore::derive(
            PassphraseRoute::Enabled,
            &Secret::new(passphrase),
            &quick(salt),
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
        assert!(STANDARD.is_supported());
        assert!(QUICK.is_supported());
    }

    /// A fixed passphrase, and the salt 0, 1, ..., 15.
    const FIXED_PASSPHRASE: &str = "example fixed passphrase";
    const FIXED_SALT: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

    /// The root every build before decision 0438 derived from the fixed
    /// passphrase and salt at 64 MiB, three passes, one lane: its key
    /// store entry (format, identifier, key), taken from those builds'
    /// code before this package changed it.
    const ROOT_BEFORE_0438: &str = "01142ea25e001a34a41bb9ea835bcb1b41e81f6cd051b0e989c61f3eaff70a4dbe0f9a2a9dbd1dba1f3878f5476751e313";

    async fn root_of(salt: &PassphraseSalt) -> String {
        let store = PassphraseKeyStore::derive(
            PassphraseRoute::Enabled,
            &Secret::new(FIXED_PASSPHRASE),
            salt,
        )
        .await
        .unwrap();
        crate::hex::encode(&store.root.to_entry())
    }

    /// Decision 0438, point 3. A file of sixteen bare bytes, as earlier
    /// builds wrote it, gives the root those builds gave, and is not
    /// rewritten.
    #[tokio::test]
    async fn a_salt_of_sixteen_bare_bytes_gives_the_root_earlier_builds_gave() {
        let folder = Folder::new();
        let file = folder.0.join("vault.salt");
        fs::write(&file, FIXED_SALT).unwrap();
        let salt = PassphraseSalt::load_or_create(&file, &OsRandom)
            .await
            .unwrap();
        assert_eq!(salt.cost, STANDARD);
        assert_eq!(salt, PassphraseSalt::from_bytes(FIXED_SALT));
        assert_eq!(root_of(&salt).await, ROOT_BEFORE_0438);
        assert_eq!(fs::read(&file).unwrap(), FIXED_SALT);
    }

    /// Decision 0438, point 1. A new file is the recorded form, with this
    /// build's cost, and reads back as the salt it was made from.
    #[tokio::test]
    async fn a_new_salt_file_records_the_algorithm_and_the_cost() {
        let folder = Folder::new();
        let file = folder.0.join("vault.salt");
        let made = PassphraseSalt::load_or_create(&file, &OsRandom)
            .await
            .unwrap();
        let text = String::from_utf8(fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            text,
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=65536 passes=3 lanes=1 salt={}\n",
                crate::hex::encode(made.as_bytes())
            )
        );
        assert_eq!(made.cost, STANDARD);
        let again = PassphraseSalt::load_or_create(&file, &OsRandom)
            .await
            .unwrap();
        assert_eq!(again, made);
        // The same salt and cost in either form give the same root.
        let fixed = PassphraseSalt::from_bytes(FIXED_SALT);
        assert_eq!(
            PassphraseSalt::from_file(fixed.to_record().as_bytes()).unwrap(),
            fixed
        );
        assert_eq!(
            root_of(&PassphraseSalt::from_file(fixed.to_record().as_bytes()).unwrap()).await,
            ROOT_BEFORE_0438
        );
    }

    /// Decision 0438, point 2. A file whose cost is not the constant gives
    /// the root that cost gives, which is not the constant's.
    #[tokio::test]
    async fn a_root_is_derived_with_the_cost_its_file_records() {
        let folder = Folder::new();
        let file = folder.0.join("vault.salt");
        fs::write(
            &file,
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=32 passes=2 lanes=2 salt={}\n",
                crate::hex::encode(&FIXED_SALT)
            ),
        )
        .unwrap();
        let salt = PassphraseSalt::load_or_create(&file, &OsRandom)
            .await
            .unwrap();
        let recorded = Cost {
            memory_kib: 32,
            passes: 2,
            lanes: 2,
        };
        assert_eq!(salt.cost, recorded);
        let expected = derive_root(FIXED_PASSPHRASE.as_bytes(), &FIXED_SALT, recorded).unwrap();
        let got = root_of(&salt).await;
        assert_eq!(got, crate::hex::encode(&expected.to_entry()));
        assert_ne!(got, ROOT_BEFORE_0438);
        // A key wrapped under it opens under it again, from the same file.
        let key = KeyMaterial::generate(&OsRandom).unwrap();
        let first = PassphraseKeyStore::derive(
            PassphraseRoute::Enabled,
            &Secret::new(FIXED_PASSPHRASE),
            &salt,
        )
        .await
        .unwrap();
        let wrapped = first.wrap(&key, &binding()).await.unwrap();
        let reread = PassphraseSalt::load_or_create(&file, &OsRandom)
            .await
            .unwrap();
        let again = PassphraseKeyStore::derive(
            PassphraseRoute::Enabled,
            &Secret::new(FIXED_PASSPHRASE),
            &reread,
        )
        .await
        .unwrap();
        assert_eq!(
            again.unwrap(&wrapped, &binding()).await.unwrap().expose(),
            key.expose()
        );
    }

    /// Decision 0438, point 4. A format, an algorithm, a version, or a cost
    /// this build does not read is refused by name, with the fix to use a
    /// build that reads it; a file that is not a salt at all is refused as
    /// such. Neither is read as today's cost, and the file is left as it is.
    #[tokio::test]
    async fn a_salt_file_this_build_does_not_read_is_refused_and_derives_nothing() {
        let salt = crate::hex::encode(&FIXED_SALT);
        let unsupported = [
            format!(
                "format=2 algorithm=argon2id version=0x13 memory_kib=65536 passes=3 lanes=1 salt={salt}\n"
            ),
            "format=2 something else entirely\n".to_owned(),
            format!(
                "format=1 algorithm=scrypt version=0x13 memory_kib=65536 passes=3 lanes=1 salt={salt}\n"
            ),
            format!(
                "format=1 algorithm=argon2id version=0x10 memory_kib=65536 passes=3 lanes=1 salt={salt}\n"
            ),
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=4194305 passes=3 lanes=1 salt={salt}\n"
            ),
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=65536 passes=0 lanes=1 salt={salt}\n"
            ),
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=65536 passes=3 lanes=0 salt={salt}\n"
            ),
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=4 passes=3 lanes=1 salt={salt}\n"
            ),
        ];
        let malformed = [
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=65536 passes=3 lanes=1 salt={salt}"
            ),
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=+65536 passes=3 lanes=1 salt={salt}\n"
            ),
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=65536 passes=3 salt={salt}\n"
            ),
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=65536 passes=3 lanes=1 salt={salt} more=1\n"
            ),
            "format=1 algorithm=argon2id version=0x13 memory_kib=65536 passes=3 lanes=1 salt=00ff\n"
                .to_owned(),
            format!(
                "format=1 algorithm=argon2id version=0x13 memory_kib=99999999999 passes=3 lanes=1 salt={salt}\n"
            ),
            "algorithm=argon2id format=1\n".to_owned(),
            String::new(),
        ];
        let folder = Folder::new();
        let file = folder.0.join("vault.salt");
        for (text, by_name) in unsupported
            .iter()
            .map(|text| (text, true))
            .chain(malformed.iter().map(|text| (text, false)))
        {
            fs::write(&file, text).unwrap();
            let refused = PassphraseSalt::load_or_create(&file, &OsRandom)
                .await
                .err()
                .unwrap_or_else(|| panic!("read as a salt: {text:?}"));
            assert_eq!(refused.code(), &failure::KEY_STORE_UNREACHABLE, "{text:?}");
            assert_eq!(refused.class(), a2m_core::FailureClass::Busy);
            assert_eq!(
                refused.next_step().contains("a build that reads"),
                by_name,
                "{text:?}"
            );
            assert!(!refused.message().contains(&salt));
            assert_eq!(fs::read(&file).unwrap(), text.as_bytes());
        }
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
        let kept_salt = PassphraseSalt::from_file(&kept).unwrap();
        for (salt, engine, _, _) in &started {
            assert_eq!(salt, &kept_salt);
            for (_, _, key, wrapped) in &started {
                let unwrapped = engine.unwrap(wrapped, &binding()).await;
                assert!(unwrapped.is_ok_and(|opened| opened.expose() == key.expose()));
            }
        }
        // A later start reads the same salt and writes nothing.
        let again = PassphraseSalt::load_or_create(&file, &OsRandom)
            .await
            .unwrap();
        assert_eq!(again, kept_salt);
        assert_eq!(fs::read(&file).unwrap(), kept);
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
            let kept = PassphraseSalt::from_file(&fs::read(&file).unwrap()).unwrap();
            assert_eq!(salts[0], crate::hex::encode(kept.as_bytes()));
        }
    }
}
