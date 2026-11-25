//! Helpers shared by this crate's tests of its public interface.
#![allow(
    dead_code,
    reason = "each test binary uses some of these helpers and not others"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use a2m_core::{
    Digest, DigestPurpose, KeyedDigest, REDACTED, Redact, WorkspaceId, keyed_digest_under,
};

/// A redactor for tests, standing in for the vault's: it masks exact matches
/// of the secrets planted in it. A secret can be registered later, which is
/// how a test plants one after text was recorded.
#[derive(Default)]
pub struct Planted(Mutex<Vec<String>>);

impl Planted {
    pub fn secrets(secrets: &[&str]) -> Self {
        Self(Mutex::new(
            secrets.iter().map(|secret| (*secret).to_owned()).collect(),
        ))
    }

    pub fn register(&self, secret: &str) {
        self.0.lock().unwrap().push(secret.to_owned());
    }
}

impl Redact for Planted {
    fn redact(&self, text: &str) -> String {
        self.0
            .lock()
            .unwrap()
            .iter()
            .fold(text.to_owned(), |text, secret| {
                text.replace(secret.as_str(), REDACTED)
            })
    }
}

/// A keyed digest for tests, under a fixed key, made the way the vault makes
/// its own.
pub struct TestKey(pub &'static [u8]);

impl KeyedDigest for TestKey {
    fn keyed_digest(
        &self,
        workspace: &WorkspaceId,
        purpose: DigestPurpose,
        bytes: &[u8],
    ) -> Digest {
        keyed_digest_under(self.0, workspace, purpose, bytes)
    }
}

/// A folder under the system's temporary folder, deleted when dropped.
pub struct TempFolder(PathBuf);

impl TempFolder {
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "a2m-logs-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        // Best effort: a folder left behind in the temporary folder is harmless.
        let _ = fs::remove_dir_all(&self.0);
    }
}
