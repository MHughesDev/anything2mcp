//! A download a step declares, into a folder of the engine's own.
//!
//! Every download is denied when a browser starts: it writes no file, in any
//! tab. A step that declares a download lifts that for the browser's context
//! of one profile, for the length of the step: [`ActTab::allow_download`]
//! names the folder and the most bytes, the step presses what offers the
//! file, [`ActTab::downloaded`] waits for the one file, and
//! [`ActTab::forbid_downloads`] puts the refusal back, whatever happened.
//!
//! The page decides nothing about where the file goes. The browser is told to
//! save it under a name of its own making (the number it gives the
//! download), so the name the page suggests is never part of a path; it is
//! kept, cleaned of everything but a plain name, as a thing to show. A file
//! larger than the bound is cancelled as soon as its size is known to be, or
//! reached, and what the browser wrote is deleted. One download only: a
//! second one that begins while the lift holds is cancelled.
//!
//! The browser writes into the writing folder, beside the folder it was
//! given, and a file is moved into that folder only once the browser says it
//! is whole. The browser may make its half-written file of a cancelled
//! download after it has said the download ended, and clears it a moment
//! later, with nothing it reports in between; so the folder a step names
//! holds whole files alone, whenever it is read, and what is left in the
//! writing folder is the browser's to clear, or the next lift's.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{ActTab, BrowserError, Deadline, Page};
use crate::protocol::{CancelDownload, SetDownloadBehavior};

/// The longest a suggested name is kept.
const MOST_NAME: usize = 80;

/// What a declared download came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// The file the browser wrote, in the folder it was given, under the
    /// name the browser made.
    pub path: PathBuf,
    /// The name the page suggested, cleaned to a plain name. For showing.
    pub suggested: String,
    /// How many bytes the file holds.
    pub bytes: u64,
}

/// Why no file came.
#[derive(Debug)]
pub enum DownloadError {
    /// No download began before the deadline.
    NoneBegan,
    /// The file is larger than the bound: it was cancelled, and nothing is
    /// kept.
    TooLarge {
        /// The bound, in bytes.
        limit: u64,
    },
    /// The browser ended the download before it was whole.
    Cancelled,
    /// The browser could not be asked.
    Browser(BrowserError),
}

impl From<BrowserError> for DownloadError {
    fn from(error: BrowserError) -> Self {
        Self::Browser(error)
    }
}

/// The folder the browser writes a declared download into, beside the folder
/// the file is kept in: its name with [`WRITING`] after it.
fn writing_folder(folder: &Path) -> PathBuf {
    let mut name = folder.file_name().unwrap_or_default().to_os_string();
    name.push(WRITING);
    folder.with_file_name(name)
}

/// What the name of the writing folder adds to the name of the folder a
/// file is kept in.
const WRITING: &str = "-writing";

/// What the tab holds of a declared download.
#[derive(Default)]
pub(super) struct DownloadState {
    /// The writing folder and the bound, while the refusal is lifted.
    allowed: Option<(PathBuf, u64)>,
    /// The folder a whole file is moved into.
    into: Option<PathBuf>,
    /// The download that began, by the browser's number for it.
    guid: Option<String>,
    suggested: String,
    /// How the download stands: bytes received, whether it is done, and
    /// whether the browser ended it.
    received: u64,
    done: bool,
    cancelled: bool,
    /// Whether the bound was crossed.
    too_large: bool,
}

/// A suggested name, cleaned to a plain name: what follows the last
/// separator, with no control character, cut short.
fn plain_name(suggested: &str) -> String {
    let last = suggested
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(MOST_NAME)
        .collect::<String>();
    if last.is_empty() || last == "." || last == ".." {
        "download".to_owned()
    } else {
        last
    }
}

impl Page {
    fn downloads(&self) -> std::sync::MutexGuard<'_, DownloadState> {
        self.download
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether a download that begins is one a step declared. When it is,
    /// it is the one, and it is remembered; when it is not, it is refused
    /// as before.
    pub(super) fn download_begins(&self, params: &Value) -> bool {
        let mut state = self.downloads();
        if state.allowed.is_none() || state.guid.is_some() {
            return false;
        }
        let Some(guid) = params.get("guid").and_then(Value::as_str) else {
            return false;
        };
        state.guid = Some(guid.to_owned());
        state.suggested = plain_name(
            params
                .get("suggestedFilename")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        drop(state);
        self.download_changes.send_modify(|count| *count += 1);
        true
    }

    /// The browser told how a download stands.
    pub(super) fn download_progress(&self, params: &Value) {
        let mut state = self.downloads();
        let Some((_, limit)) = state.allowed.clone() else {
            return;
        };
        if params.get("guid").and_then(Value::as_str) != state.guid.as_deref() {
            return;
        }
        let number = |name: &str| params.get(name).and_then(Value::as_u64).unwrap_or(0);
        state.received = number("receivedBytes");
        if number("totalBytes") > limit || state.received > limit {
            state.too_large = true;
        }
        match params.get("state").and_then(Value::as_str) {
            Some("completed") => state.done = true,
            Some("canceled") => state.cancelled = true,
            _ => {}
        }
        drop(state);
        self.download_changes.send_modify(|count| *count += 1);
    }

    /// After a cancel: waits for the browser to say the download ended, one
    /// way or the other, and then deletes what the browser wrote in the
    /// writing folder, under either name it uses while it writes. A
    /// half-written file the browser makes after it said so is never in the
    /// folder a step names, and the browser clears it itself.
    async fn forget_download(&self, writing: &Path, guid: &str) {
        let mut changes = self.download_changes.subscribe();
        let until = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let ended = {
                let state = self.downloads();
                state.cancelled || state.done
            };
            if ended {
                break;
            }
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                () = tokio::time::sleep_until(until) => break,
            }
        }
        for name in [guid.to_owned(), format!("{guid}.crdownload")] {
            let _ = std::fs::remove_file(writing.join(name));
        }
    }

    async fn download_behavior(
        &self,
        behavior: &str,
        folder: Option<&Path>,
        deadline: Deadline,
    ) -> Result<(), BrowserError> {
        let path = folder.map(|folder| folder.to_string_lossy().into_owned());
        self.call(
            &SetDownloadBehavior {
                behavior,
                browser_context_id: Some(self.browser.context.as_str()),
                download_path: path.as_deref(),
                events_enabled: path.is_some().then_some(true),
            },
            deadline,
        )
        .await
        .map(|_| ())
    }
}

impl ActTab {
    /// Lifts the refusal of downloads for the browser's context of this
    /// profile: the next download of the tab is written, under a name the
    /// browser makes, into the writing folder beside `folder`, made when it
    /// is not there and emptied of what an earlier step left; it may be at
    /// most `most_bytes`, and is moved into `folder` once it is whole.
    /// Lifted until [`ActTab::forbid_downloads`].
    ///
    /// # Errors
    ///
    /// The browser did not take it, or the writing folder could not be made.
    pub async fn allow_download(
        &self,
        folder: &Path,
        most_bytes: u64,
        deadline: Deadline,
    ) -> Result<(), BrowserError> {
        let page = &self.page;
        let writing = writing_folder(folder);
        std::fs::create_dir_all(&writing).map_err(|error| {
            BrowserError::Unexpected(format!("the writing folder of downloads: {error}"))
        })?;
        if let Ok(entries) = std::fs::read_dir(&writing) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        *page.downloads() = DownloadState {
            allowed: Some((writing.clone(), most_bytes)),
            into: Some(folder.to_path_buf()),
            ..DownloadState::default()
        };
        page.download_behavior("allowAndName", Some(&writing), deadline)
            .await
    }

    /// Waits for the one download to be whole, moves it from the writing
    /// folder into the folder named, and gives it. A file larger than the
    /// bound is cancelled and deleted.
    ///
    /// # Errors
    ///
    /// [`DownloadError::NoneBegan`] when nothing was offered before the
    /// deadline, [`DownloadError::TooLarge`] and [`DownloadError::Cancelled`]
    /// as above.
    pub async fn downloaded(&self, deadline: Deadline) -> Result<Download, DownloadError> {
        /// What the state says now, read with the lock held and no await.
        enum Now {
            Wait,
            Whole {
                written: PathBuf,
                download: Download,
            },
            Over {
                guid: String,
                path: PathBuf,
                limit: u64,
            },
            Ended {
                path: PathBuf,
            },
        }
        let page = &self.page;
        let mut changes = page.download_changes.subscribe();
        let cancel = |guid: String| async move {
            let _ = page
                .call(
                    &CancelDownload {
                        guid: &guid,
                        browser_context_id: Some(page.browser.context.as_str()),
                    },
                    Deadline::after(std::time::Duration::from_secs(5)),
                )
                .await;
        };
        loop {
            let now = {
                let state = page.downloads();
                match (
                    state.guid.clone(),
                    state.allowed.clone(),
                    state.into.clone(),
                ) {
                    (Some(guid), Some((writing, limit)), Some(into)) => {
                        let path = writing.join(&guid);
                        if state.too_large {
                            Now::Over { guid, path, limit }
                        } else if state.cancelled {
                            Now::Ended { path }
                        } else if state.done {
                            Now::Whole {
                                written: path,
                                download: Download {
                                    path: into.join(&guid),
                                    suggested: state.suggested.clone(),
                                    bytes: state.received,
                                },
                            }
                        } else {
                            Now::Wait
                        }
                    }
                    _ => Now::Wait,
                }
            };
            match now {
                Now::Whole { written, download } => {
                    // The browser said the file is whole: it has its
                    // final name, and is moved into the folder named.
                    return match std::fs::rename(&written, &download.path) {
                        Ok(()) => Ok(download),
                        Err(error) => {
                            let _ = std::fs::remove_file(&written);
                            Err(DownloadError::Browser(BrowserError::Unexpected(format!(
                                "the whole file could not be moved out of the writing folder: {error}"
                            ))))
                        }
                    };
                }
                Now::Over { guid, path, limit } => {
                    cancel(guid.clone()).await;
                    if let Some(folder) = path.parent() {
                        page.forget_download(folder, &guid).await;
                    }
                    return Err(DownloadError::TooLarge { limit });
                }
                Now::Ended { path } => {
                    let _ = std::fs::remove_file(&path);
                    return Err(DownloadError::Cancelled);
                }
                Now::Wait => {}
            }
            tokio::select! {
                changed = changes.changed() => {
                    if changed.is_err() {
                        return Err(DownloadError::Browser(BrowserError::Disconnected {
                            dispatch: super::Dispatch::Sent,
                        }));
                    }
                }
                () = tokio::time::sleep_until(deadline.at()) => {
                    // It began and did not end in time: it is cancelled and
                    // nothing is kept. Or it never began.
                    let began = {
                        let state = page.downloads();
                        state.guid.clone().zip(state.allowed.clone())
                    };
                    return match began {
                        Some((guid, (folder, _))) => {
                            cancel(guid.clone()).await;
                            page.forget_download(&folder, &guid).await;
                            Err(DownloadError::Cancelled)
                        }
                        None => Err(DownloadError::NoneBegan),
                    };
                }
            }
        }
    }

    /// Puts the refusal of downloads back, and forgets the declared one.
    ///
    /// # Errors
    ///
    /// The browser did not take it. The tab is then not to be used again:
    /// the refusal that holds a browser's downloads off is not in place.
    pub async fn forbid_downloads(&self, deadline: Deadline) -> Result<(), BrowserError> {
        let page = &self.page;
        *page.downloads() = DownloadState::default();
        page.download_behavior("deny", None, deadline).await
    }
}
