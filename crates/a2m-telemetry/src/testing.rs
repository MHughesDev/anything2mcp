//! Helpers for this crate's own tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A folder under the system's temporary folder, deleted when dropped.
pub(crate) struct TempFolder(PathBuf);

impl TempFolder {
    /// A new, empty folder whose name starts with `label`.
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let name = format!(
            "a2m-telemetry-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        // Best effort: a folder left behind in the temporary folder is harmless.
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A redactor for tests: it knows a fixed list of planted secrets and masks
/// exact matches, as the vault's redactor does. More can be registered
/// later, which is how a test plants a secret after text was recorded.
pub(crate) struct Planted(std::sync::Mutex<Vec<String>>);

impl Planted {
    pub(crate) fn none() -> Self {
        Self::secrets(&[])
    }

    pub(crate) fn secrets(secrets: &[&str]) -> Self {
        Self(std::sync::Mutex::new(
            secrets.iter().map(|secret| (*secret).to_owned()).collect(),
        ))
    }
}

impl a2m_core::Redact for Planted {
    fn redact(&self, text: &str) -> String {
        self.0
            .lock()
            .unwrap()
            .iter()
            .fold(text.to_owned(), |text, secret| {
                text.replace(secret.as_str(), a2m_core::REDACTED)
            })
    }
}
