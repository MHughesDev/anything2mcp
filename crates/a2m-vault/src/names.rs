//! What a sealed item is and which keys stand above it: the names that are
//! authenticated with every ciphertext.

use std::fmt;
use std::str::FromStr;

use a2m_core::{Digest, Failure, Identity, TargetName, WorkspaceId};

use crate::cipher::BoundTo;
use crate::failure;

/// The longest a name in this module may be, in bytes.
const LONGEST: usize = 255;

fn check(what: &'static str, text: &str) -> Result<(), Failure> {
    if text.is_empty() || text.len() > LONGEST || text.chars().any(char::is_control) {
        return Err(failure::bad_name(what));
    }
    Ok(())
}

macro_rules! name {
    ($(#[$doc:meta])* $name:ident, $what:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Checks `text` and makes a name of it.
            ///
            /// # Errors
            ///
            /// `input`/`schema_invalid` when `text` is empty, longer than 255
            /// bytes, or holds a control character.
            pub fn new(text: impl Into<String>) -> Result<Self, Failure> {
                let text = text.into();
                check($what, &text)?;
                Ok(Self(text))
            }

            /// The name as text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

name! {
    /// Which item a sealed value is: a blob's content reference, a session's
    /// identifier, a vault entry's name. Never the value itself.
    ItemId, "item identifier"
}

name! {
    /// The name of this installation, which names the root's entry in the
    /// operating system's key store. Never the name of a target.
    InstallationName, "installation name"
}

/// Whose an item is: a person or a service identity. Never a credential.
///
/// Each owner has a key of their own, so an item sealed for one owner cannot
/// be opened as another.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OwnerRef {
    kind: OwnerKind,
    id: String,
}

/// Whether an owner is a person or a service identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OwnerKind {
    /// A person, who signs in themselves.
    Person,
    /// A service identity: a dedicated account, a runner, or the workspace
    /// itself for items that belong to no person.
    Service,
}

impl OwnerKind {
    /// The kind as it is written.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Service => "service",
        }
    }
}

impl OwnerRef {
    /// A person, by the identifier the workspace knows them by.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` when `id` is not a name this crate accepts.
    pub fn person(id: impl Into<String>) -> Result<Self, Failure> {
        Self::new(OwnerKind::Person, id)
    }

    /// A service identity, by its name.
    ///
    /// A name that begins with `account:` is a dedicated account's: it is
    /// the owner [`OwnerRef::of`] gives for that account, deliberately, so
    /// that a store can make an owner again from the kind and the
    /// identifier it kept. A service's name is therefore never built from
    /// text a caller chose: a service that could name itself `account:x`
    /// would hold that account's key.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` when `name` is not a name this crate accepts.
    pub fn service(name: impl Into<String>) -> Result<Self, Failure> {
        Self::new(OwnerKind::Service, name)
    }

    /// The owner of what belongs to an identity a system sees: the person,
    /// or the dedicated account as the service identity named `account:`
    /// and the account's name.
    #[must_use]
    pub fn of(identity: &Identity) -> Self {
        let (kind, id) = match identity {
            Identity::Person(person) => (OwnerKind::Person, person.to_string()),
            Identity::Account(account) => (OwnerKind::Service, format!("account:{account}")),
        };
        Self { kind, id }
    }

    fn new(kind: OwnerKind, id: impl Into<String>) -> Result<Self, Failure> {
        let id = id.into();
        check("owner", &id)?;
        Ok(Self { kind, id })
    }

    /// Whether this is a person or a service identity.
    #[must_use]
    pub fn kind(&self) -> OwnerKind {
        self.kind
    }

    /// The owner's identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// What kind of thing a sealed item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SealedKind {
    /// Saved session state: equivalent to being signed in as the owner.
    SessionState,
    /// A stored credential, a person's or a service identity's, for one
    /// target.
    Credential,
    /// An evidence blob.
    Blob,
    /// A value the ledger holds.
    LedgerValue,
    /// A runner's private key.
    RunnerKey,
}

impl SealedKind {
    /// Every kind, in the order the documents list them.
    pub const ALL: [Self; 5] = [
        Self::SessionState,
        Self::Credential,
        Self::Blob,
        Self::LedgerValue,
        Self::RunnerKey,
    ];

    /// The kind as it is written.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionState => "session_state",
            Self::Credential => "credential",
            Self::Blob => "blob",
            Self::LedgerValue => "ledger_value",
            Self::RunnerKey => "runner_key",
        }
    }
}

/// What a sealed item is. Every field is authenticated with the ciphertext,
/// so an item cannot be opened as another workspace's, another owner's,
/// another kind, or another item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealContext {
    /// The workspace the item belongs to.
    pub workspace: WorkspaceId,
    /// Whose the item is.
    pub owner: OwnerRef,
    /// What kind of thing it is.
    pub kind: SealedKind,
    /// Which item it is.
    pub item: ItemId,
}

impl SealContext {
    /// The context of a stored credential: the workspace, its owner, the
    /// target it was stored for, and its reference. The item is written
    /// `credential:<target>:<reference>`, which reads one way only because a
    /// target's name holds no colon. The target is authenticated with the
    /// ciphertext, so a credential stored for one target does not open
    /// under another, even under the same reference (decision 0248).
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` when the item would not be a name this
    /// crate accepts.
    pub fn credential(
        workspace: WorkspaceId,
        owner: OwnerRef,
        target: &TargetName,
        reference: &str,
    ) -> Result<Self, Failure> {
        Ok(Self {
            workspace,
            owner,
            kind: SealedKind::Credential,
            item: ItemId::new(format!("credential:{target}:{reference}"))?,
        })
    }

    /// The context's fields, added to bytes that will be authenticated.
    pub(crate) fn bind(&self, bound: BoundTo) -> BoundTo {
        bound
            .field("workspace", self.workspace.as_bytes())
            .field("owner_kind", self.owner.kind().as_str().as_bytes())
            .field("owner", self.owner.id().as_bytes())
            .field("kind", self.kind.as_str().as_bytes())
            .field("item", self.item.as_str().as_bytes())
    }
}

/// A layer of the key hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// The installation's root, held by the key store.
    Root,
    /// A workspace's key, wrapped by the root.
    Workspace,
    /// An owner's key, wrapped by the workspace's key.
    Owner,
    /// The installation's key for keyed digests, wrapped by the root. Each
    /// workspace's digest key is derived from it and never stored.
    Digest,
    /// An item's data key, wrapped by its owner's key and kept in the
    /// envelope; never in a key record.
    Data,
}

impl Layer {
    /// Every layer, from the top.
    pub const ALL: [Self; 5] = [
        Self::Root,
        Self::Workspace,
        Self::Owner,
        Self::Digest,
        Self::Data,
    ];

    /// The layer as it is written in a key record.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Workspace => "workspace",
            Self::Owner => "owner",
            Self::Digest => "digest",
            Self::Data => "data",
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Layer {
    type Err = Failure;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|layer| layer.as_str() == text)
            .ok_or_else(|| failure::bad_name("key layer"))
    }
}

/// Which generation of a key: 1 for the first, one more after each rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(u32);

impl Generation {
    /// The first generation of any key.
    pub const FIRST: Self = Self(1);

    /// A generation read back from a key record.
    #[must_use]
    pub const fn new(number: u32) -> Self {
        Self(number)
    }

    /// The generation as a number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The generation after this one, or `None` at the last number.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(number) => Some(Self(number)),
            None => None,
        }
    }
}

impl fmt::Display for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The identifier of one key of the hierarchy.
///
/// It is a digest of what the key is for, so the same workspace or owner has
/// the same key identifier on every start, and the identifier itself shows
/// no name. It is read and written as 64 lowercase hexadecimal characters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(Digest);

impl KeyId {
    /// The installation's root.
    #[must_use]
    pub fn root() -> Self {
        Self::of(BoundTo::new("a2m-vault/key-id/root"))
    }

    /// A workspace's key.
    #[must_use]
    pub fn workspace(workspace: &WorkspaceId) -> Self {
        Self::of(
            BoundTo::new("a2m-vault/key-id/workspace").field("workspace", workspace.as_bytes()),
        )
    }

    /// An owner's key within a workspace.
    #[must_use]
    pub fn owner(workspace: &WorkspaceId, owner: &OwnerRef) -> Self {
        Self::of(
            BoundTo::new("a2m-vault/key-id/owner")
                .field("workspace", workspace.as_bytes())
                .field("owner_kind", owner.kind().as_str().as_bytes())
                .field("owner", owner.id().as_bytes()),
        )
    }

    /// The installation's key for keyed digests.
    #[must_use]
    pub fn digest() -> Self {
        Self::of(BoundTo::new("a2m-vault/key-id/digest"))
    }

    fn of(what: BoundTo) -> Self {
        Self(Digest::of(what.as_bytes()))
    }

    /// An identifier read back from an envelope.
    pub(crate) fn from_bytes(bytes: [u8; Digest::LENGTH]) -> Self {
        Self(Digest::from_bytes(bytes))
    }

    /// The identifier's bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; Digest::LENGTH] {
        self.0.as_bytes()
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyId({})", self.0)
    }
}

impl FromStr for KeyId {
    type Err = Failure;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.parse::<Digest>()
            .map(Self)
            .map_err(|_| failure::bad_name("key identifier"))
    }
}

/// One generation of one key: what an envelope or a key record names as the
/// key above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyRef {
    /// Which key.
    pub id: KeyId,
    /// Which generation of it.
    pub generation: Generation,
}

/// What a key wrapped by the key store is: authenticated with the wrapped
/// bytes, so one wrapped key cannot stand in for another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// Which key is wrapped.
    pub key: KeyId,
    /// Its layer.
    pub layer: Layer,
    /// Its generation.
    pub generation: Generation,
}

impl Binding {
    /// The binding's fields, added to bytes that will be authenticated.
    pub(crate) fn bind(&self, bound: BoundTo) -> BoundTo {
        bound
            .field("key", self.key.as_bytes())
            .field("layer", self.layer.as_str().as_bytes())
            .number("generation", self.generation.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_text_without_control_characters() {
        assert!(OwnerRef::person("ana@example.test").is_ok());
        assert!(ItemId::new("x".repeat(255)).is_ok());
        for bad in [String::new(), "x".repeat(256), "a\nb".to_owned()] {
            let failure = ItemId::new(bad).unwrap_err();
            assert_eq!(failure.code().as_str(), "schema_invalid");
        }
    }

    #[test]
    fn a_key_identifier_is_stable_and_differs_by_what_it_is_for() {
        let main = WorkspaceId::from_bytes([1; 16]);
        let other = WorkspaceId::from_bytes([2; 16]);
        let ana = OwnerRef::person("ana").unwrap();
        let ana_service = OwnerRef::service("ana").unwrap();
        let ids = [
            KeyId::root(),
            KeyId::workspace(&main),
            KeyId::workspace(&other),
            KeyId::digest(),
            KeyId::owner(&main, &ana),
            KeyId::owner(&main, &ana_service),
            KeyId::owner(&other, &ana),
        ];
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(
                ids.iter().filter(|other| *other == id).count(),
                1,
                "{index}"
            );
            assert_eq!(id.to_string().parse::<KeyId>().unwrap(), *id);
        }
        assert_eq!(KeyId::workspace(&main), KeyId::workspace(&main));
        assert!(!format!("{:?}", KeyId::owner(&main, &ana)).contains("ana"));
    }

    /// Audit finding V-6. A dedicated account is a service identity with a
    /// reserved beginning to its name, and a store makes its owner again
    /// from the kind and the identifier it kept. This holds the rule in
    /// place: the two spellings are one owner with one key, and a person of
    /// the same text is another.
    #[test]
    fn an_accounts_owner_is_the_service_identity_named_for_it() {
        let main = WorkspaceId::from_bytes([1; 16]);
        let account = a2m_core::AccountName::new("robot").unwrap();
        let owner = OwnerRef::of(&Identity::Account(account));
        assert_eq!(owner.kind(), OwnerKind::Service);
        assert_eq!(owner.id(), "account:robot");
        let rebuilt = OwnerRef::service(owner.id()).unwrap();
        assert_eq!(rebuilt, owner);
        assert_eq!(KeyId::owner(&main, &rebuilt), KeyId::owner(&main, &owner));
        let person = OwnerRef::person("account:robot").unwrap();
        assert_ne!(KeyId::owner(&main, &person), KeyId::owner(&main, &owner));
        let other = OwnerRef::service("robot").unwrap();
        assert_ne!(KeyId::owner(&main, &other), KeyId::owner(&main, &owner));
    }

    #[test]
    fn a_layer_reads_back_what_it_writes() {
        for layer in Layer::ALL {
            assert_eq!(layer.as_str().parse::<Layer>().unwrap(), layer);
        }
        assert!("top".parse::<Layer>().is_err());
        assert_eq!(Generation::FIRST.next(), Some(Generation::new(2)));
        assert_eq!(Generation::new(u32::MAX).next(), None);
    }
}
