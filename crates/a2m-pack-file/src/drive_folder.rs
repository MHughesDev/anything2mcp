//! The steps of a folder's bindings and of a growing file's reads, in the
//! `file` driver (decision 0246). One step at a time, as written, with no
//! retry; every write is done on a blocking thread and proven from disk.

use std::collections::BTreeMap;

use a2m_core::{Digest, Step, Value, Values};
use a2m_provider::{Access, FileIdentity, Observed, Posture, StepFailure, StepInput, StepOutcome};

use crate::codes::{self, compiled_target_unreadable, stop};
use crate::driver::FileLimits;
use crate::folder::{self, Entry, Kind};
use crate::model::{
    BEFORE_DIGEST, ENTRY_CONTENT, ENTRY_NAME, Expect, FolderChange, FolderSpec, INTENDED_DIGEST,
    LOG_FROM, LogSpec, ROWS, StepSpec, TRUNCATED, field_arg, key_arg,
};
use crate::open::{not_allowed, unavailable};
use crate::place::{Folder, Identity};
use crate::save::{self, SaveJob};

/// The name under which a list says how many entries it left out because
/// their names are not names the pack can use.
pub const SKIPPED: &str = "skipped";

/// A change built and not yet carried out.
enum Pending {
    Create {
        bytes: Vec<u8>,
    },
    Replace {
        bytes: Vec<u8>,
        before: Digest,
        intended: Digest,
        expected: Option<FileIdentity>,
    },
    Rename {
        to: String,
    },
    Delete {
        /// The digest of the file that was read; nothing for a folder.
        digest: Option<Digest>,
    },
}

/// The entry a binding is working on.
struct Current {
    folder: String,
    /// The name looked up.
    name: String,
    /// What is there, when something is.
    entry: Option<Entry>,
    /// The identity of the file found, by its name.
    identity: Option<Identity>,
    pending: Option<Pending>,
    /// What was carried out, for the confirm.
    done: Option<FolderChange>,
    /// The new name of a rename that was carried out.
    renamed_to: Option<String>,
    intended: Option<Digest>,
}

/// Where a step acts, and what the run may do there.
#[derive(Clone, Copy)]
pub(crate) struct Here<'a> {
    /// The surface's folder.
    pub folder: &'a Folder,
    /// The limits of `packs.file`.
    pub limits: FileLimits,
    /// What the run may do.
    pub posture: Posture,
}

/// What the driver holds for folders and growing files.
#[derive(Default)]
pub(crate) struct Entries {
    pub folders: BTreeMap<String, FolderSpec>,
    pub logs: BTreeMap<String, LogSpec>,
    current: Option<Current>,
}

fn out_of_order(what: &str) -> StepFailure {
    compiled_target_unreadable(&format!("{what}, which no lowering of the pack gives"))
}

fn name_of(input: &StepInput<'_>, arg: &str) -> Result<String, StepFailure> {
    match input.args.get(arg) {
        Some(Value::Text(name)) => Ok(name.clone()),
        Some(_) => Err(stop(
            &codes::row::VALUE_UNPARSABLE,
            "An entry's name is text.",
            "Give the entry's name as text.",
        )),
        None => Err(compiled_target_unreadable(
            "a folder's step lacks the entry's name",
        )),
    }
}

fn joined<T>(
    result: Result<Result<T, StepFailure>, tokio::task::JoinError>,
    sent: bool,
) -> Result<T, StepFailure> {
    result.unwrap_or_else(|_| {
        let failure = unavailable("was being worked on when the work stopped");
        Err(if sent {
            StepFailure::unknown(failure)
        } else {
            StepFailure::not_sent(failure)
        })
    })
}

impl Entries {
    fn spec(&self, name: &str) -> Result<&FolderSpec, StepFailure> {
        self.folders.get(name).ok_or_else(|| {
            compiled_target_unreadable("a step names a folder the surface does not have")
        })
    }

    fn current(&mut self, folder: &str) -> Result<&mut Current, StepFailure> {
        self.current
            .as_mut()
            .filter(|current| current.folder == folder)
            .ok_or_else(|| out_of_order("a step before its entry was looked up"))
    }

    /// One step of a folder's binding or a growing file's read.
    #[allow(clippy::too_many_lines)] // one step after another; each is a few lines
    pub(crate) async fn act(
        &mut self,
        here: Here<'_>,
        step: &Step,
        spec: StepSpec,
        name: &str,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        let Here {
            folder,
            limits,
            posture,
        } = here;
        let ceiling = limits.parse.rows as usize;
        let done = |values: Values| {
            Ok(StepOutcome::Done(Observed {
                values,
                ..Observed::default()
            }))
        };
        match spec {
            StepSpec::LogRead => {
                let log = self
                    .logs
                    .get(name)
                    .ok_or_else(|| {
                        compiled_target_unreadable(
                            "a step names a growing file the surface does not have",
                        )
                    })?
                    .clone();
                let from = match input.args.get(LOG_FROM) {
                    None | Some(Value::Null) => None,
                    Some(Value::Text(text)) => Some(text.clone()),
                    Some(_) => {
                        return Err(stop(
                            &codes::row::VALUE_UNPARSABLE,
                            "A position is the text a read of the file gave.",
                            "Give the position the last read returned, or none to read from the start.",
                        ));
                    }
                };
                let folder = folder.clone();
                let max = limits.max_bytes;
                let values = joined(
                    tokio::task::spawn_blocking(move || {
                        crate::log::read(&folder, &log, from.as_deref(), max)
                    })
                    .await,
                    false,
                )?;
                done(values)
            }
            StepSpec::FolderOpen { for_write } => {
                self.current = None;
                let spec = self.spec(name)?.clone();
                if for_write {
                    if spec.may_be_open != Some(false) {
                        return Err(compiled_target_unreadable(
                            "a write to a folder that does not say that no application holds its entries open",
                        ));
                    }
                    if folder.access() != Access::Write {
                        return Err(StepFailure::not_sent(a2m_provider::codes::row::EFFECT_NOT_DECLARED.failure(
                            "The surface's folder is in a root the file pack may read and not write.",
                            "Put the folder in a root named `files` that the settings make writable.",
                        )));
                    }
                    folder.reached_by_no_link()?;
                } else {
                    folder.listed_path()?;
                }
                self.current = Some(Current {
                    folder: name.to_owned(),
                    name: String::new(),
                    entry: None,
                    identity: None,
                    pending: None,
                    done: None,
                    renamed_to: None,
                    intended: None,
                });
                done(Values::new())
            }
            StepSpec::FolderList { limit, digest } => {
                let spec = self.spec(name)?.clone();
                self.current(name)?;
                let at = folder.clone();
                let listing = joined(
                    tokio::task::spawn_blocking(move || {
                        let listing = folder::list(&at, spec.depth, ceiling)?;
                        let mut rows = Vec::new();
                        for entry in listing.entries.iter().take(limit as usize) {
                            let mut values = entry.values();
                            if digest {
                                values.insert("digest", digest_of(&at, entry, &spec)?);
                            }
                            rows.push(values);
                        }
                        Ok((listing, rows))
                    })
                    .await,
                    false,
                );
                let (listing, rows) = listing?;
                let mut names = vec!["name", "kind", "size", "modified"];
                if digest {
                    names.push("digest");
                }
                let mut values = Values::new();
                for field in names {
                    let list = rows
                        .iter()
                        .map(|row| row.get(field).cloned().unwrap_or(Value::Null))
                        .collect();
                    values.insert(field, Value::List(list));
                }
                values.insert(TRUNCATED, listing.entries.len() > limit as usize);
                values.insert(SKIPPED, i64::try_from(listing.skipped).unwrap_or(i64::MAX));
                values.insert(
                    ROWS,
                    Value::List(rows.into_iter().map(Value::Record).collect()),
                );
                done(values)
            }
            StepSpec::FolderLocate { expect } => {
                let spec = self.spec(name)?.clone();
                let wanted = name_of(input, &key_arg(ENTRY_NAME))?;
                self.current(name)?;
                let at = folder.clone();
                let looked = wanted.clone();
                let (entry, identity) = joined(
                    tokio::task::spawn_blocking(move || {
                        let entry = folder::find(&at, &looked, spec.depth, ceiling)?;
                        let identity = match &entry {
                            Some(entry) if entry.kind == Kind::File => {
                                folder::identity(&at, &looked, spec.depth)?
                            }
                            _ => None,
                        };
                        Ok((entry, identity))
                    })
                    .await,
                    false,
                )?;
                let current = self.current(name)?;
                current.name = wanted;
                let outcome = match (expect, &entry) {
                    (Expect::All, _) | (Expect::Present, Some(_)) | (Expect::Absent, None) => {
                        StepOutcome::Done(Observed::default())
                    }
                    (Expect::Present, None) if step.lookup => {
                        StepOutcome::Absent(Observed::default())
                    }
                    (Expect::Present, None) => {
                        return Err(stop(
                            &codes::row::PLACE_NOT_FOUND,
                            "The folder holds no entry of the name given.",
                            "Check the name, or list the folder's entries.",
                        ));
                    }
                    (Expect::Absent, Some(_)) => {
                        return Err(stop(
                            &codes::row::CHANGED_SINCE_PREVIEW,
                            "The folder holds an entry of this name already, so none was created.",
                            "Read the entry, look at the new preview, and approve again.",
                        ));
                    }
                };
                current.entry = entry;
                current.identity = identity;
                Ok(outcome)
            }
            StepSpec::FolderRead {
                digest,
                content,
                also,
            } => {
                let spec = self.spec(name)?.clone();
                let current = self.current(name)?;
                let entry = current
                    .entry
                    .clone()
                    .ok_or_else(|| out_of_order("a read of an entry that was not found"))?;
                let at = folder.clone();
                let max = spec.content_max_bytes.min(limits.max_bytes);
                let mut values = joined(
                    tokio::task::spawn_blocking(move || {
                        let mut values = entry.values();
                        if digest {
                            values.insert("digest", digest_of(&at, &entry, &spec)?);
                        }
                        if content {
                            values.insert(ENTRY_CONTENT, content_of(&at, &entry, &spec, max)?);
                        }
                        Ok(values)
                    })
                    .await,
                    false,
                )?;
                for (value, input) in also {
                    if let Some(found) = values.get(&value).cloned() {
                        values.insert(input, found);
                    }
                }
                done(values)
            }
            StepSpec::FolderSet { change } => {
                let spec = self.spec(name)?.clone();
                let current = self.current(name)?;
                if !folder::is_entry_name(&current.name) {
                    return Err(StepFailure::not_sent(not_allowed(
                        "a write names an entry directly in the surface's folder, by one name",
                    )));
                }
                let max = spec.content_max_bytes.min(limits.max_bytes);
                let content = || -> Result<Vec<u8>, StepFailure> {
                    let bytes = match input.args.get(&field_arg(ENTRY_CONTENT)) {
                        Some(Value::Text(text)) => text.clone().into_bytes(),
                        Some(Value::Null) => Vec::new(),
                        Some(_) => {
                            return Err(stop(
                                &codes::row::VALUE_UNPARSABLE,
                                "A file's content is text.",
                                "Give the content as text.",
                            ));
                        }
                        None => {
                            return Err(compiled_target_unreadable("a `set` lacks the content"));
                        }
                    };
                    if bytes.len() as u64 > max {
                        return Err(stop(
                            &codes::row::FILE_TOO_LARGE,
                            "The content is larger than the most the folder's declaration lets the pack write.",
                            "Raise `content_max_bytes` in the folder's file, or write less.",
                        ));
                    }
                    Ok(bytes)
                };
                let mut reconcile = Values::new();
                let pending = match change {
                    FolderChange::Create => {
                        if current.entry.is_some() {
                            return Err(out_of_order("a create of an entry that is there"));
                        }
                        let bytes = content()?;
                        let intended = Digest::of(&bytes);
                        reconcile.insert(INTENDED_DIGEST, intended.to_string());
                        current.intended = Some(intended);
                        Pending::Create { bytes }
                    }
                    FolderChange::Replace => {
                        must_be_file(current.entry.as_ref())?;
                        let bytes = content()?;
                        let named = folder::named(folder, &current.name, 0)?;
                        let at = named.clone();
                        let read_max = limits.max_bytes;
                        let (old, expected) = joined(
                            tokio::task::spawn_blocking(move || {
                                let old = folder::read(&at, read_max)?;
                                Ok((old, at.checked().identity().ok()))
                            })
                            .await,
                            false,
                        )?;
                        let Some(old) = old else {
                            return Err(stop(
                                &codes::row::FILE_TOO_LARGE,
                                "The file is larger than the most bytes the file pack reads.",
                                "Raise `packs.file.max_bytes`.",
                            ));
                        };
                        let before = Digest::of(&old);
                        let intended = Digest::of(&bytes);
                        reconcile.insert(BEFORE_DIGEST, before.to_string());
                        reconcile.insert(INTENDED_DIGEST, intended.to_string());
                        current.intended = Some(intended);
                        Pending::Replace {
                            bytes,
                            before,
                            intended,
                            expected,
                        }
                    }
                    FolderChange::Rename => {
                        must_be_file(current.entry.as_ref())?;
                        let to = name_of(input, &field_arg(ENTRY_NAME))?;
                        if !folder::is_entry_name(&to) {
                            return Err(StepFailure::not_sent(not_allowed(
                                "the new name is not one name directly in the surface's folder",
                            )));
                        }
                        if to == current.name {
                            return Err(stop(
                                &codes::row::VALUE_UNPARSABLE,
                                "The new name is the name the entry has.",
                                "Give another name.",
                            ));
                        }
                        Pending::Rename { to }
                    }
                    FolderChange::Delete => {
                        match current.entry.as_ref().map(|entry| entry.kind) {
                            Some(Kind::File) => {
                                // The bytes of the file that was found, so the
                                // delete removes only a file that holds them.
                                let named = folder::named(folder, &current.name, 0)?;
                                let read_max = limits.max_bytes;
                                let (bytes, identity) = joined(
                                    tokio::task::spawn_blocking(move || {
                                        let bytes = folder::read(&named, read_max)?;
                                        Ok((bytes, named.checked().identity().ok()))
                                    })
                                    .await,
                                    false,
                                )?;
                                let Some(bytes) = bytes else {
                                    return Err(stop(
                                        &codes::row::FILE_TOO_LARGE,
                                        "The file is larger than the most bytes the file pack reads, and a file is removed only when its bytes are the ones that were read.",
                                        "Raise `packs.file.max_bytes`.",
                                    ));
                                };
                                if identity.is_none() || identity != current.identity {
                                    return Err(folder::changed());
                                }
                                current.pending = Some(Pending::Delete {
                                    digest: Some(Digest::of(&bytes)),
                                });
                                return Ok(StepOutcome::Done(Observed::default()));
                            }
                            Some(Kind::Folder) => {}
                            Some(_) => {
                                return Err(StepFailure::not_sent(not_allowed(
                                    "it is a link or a special file, and the file pack removes only a file or an empty folder",
                                )));
                            }
                            None => {
                                return Err(out_of_order(
                                    "a delete of an entry that was not found",
                                ));
                            }
                        }
                        Pending::Delete { digest: None }
                    }
                };
                current.pending = Some(pending);
                Ok(StepOutcome::Done(Observed {
                    for_reconcile: reconcile,
                    ..Observed::default()
                }))
            }
            StepSpec::FolderSave => {
                let allowed = match posture {
                    Posture::Live => step.commits,
                    Posture::Sandbox { test_records } => test_records && step.commits,
                    _ => false,
                };
                if !allowed {
                    return Err(StepFailure::not_sent(a2m_provider::codes::row::READ_ONLY_POSTURE.failure(
                        "A change to a folder was asked for where nothing may be written, or by a step that is not marked as the commit point, so nothing was changed.",
                        "Run the operation live, or against a declared test record, with the save as the binding's commit point.",
                    )));
                }
                let current = self.current(name)?;
                let pending = current
                    .pending
                    .take()
                    .ok_or_else(|| out_of_order("a save with nothing set"))?;
                let at = folder.clone();
                let entry_name = current.name.clone();
                let identity = current.identity;
                let kind = current
                    .entry
                    .as_ref()
                    .map_or(Kind::File, |entry| entry.kind);
                let max = limits.max_bytes;
                let change = match &pending {
                    Pending::Create { .. } => FolderChange::Create,
                    Pending::Replace { .. } => FolderChange::Replace,
                    Pending::Rename { .. } => FolderChange::Rename,
                    Pending::Delete { .. } => FolderChange::Delete,
                };
                let to = match &pending {
                    Pending::Rename { to } => Some(to.clone()),
                    _ => None,
                };
                joined(
                    tokio::task::spawn_blocking(move || match pending {
                        Pending::Create { bytes, .. } => {
                            folder::create(&at, &entry_name, &bytes, ceiling)
                        }
                        Pending::Replace {
                            bytes,
                            before,
                            intended,
                            expected,
                        } => save::save(&SaveJob {
                            target: at.file(&entry_name)?,
                            lock_signal: None,
                            bytes,
                            before,
                            intended,
                            max_bytes: max,
                            expected,
                        }),
                        Pending::Rename { to } => {
                            let Some(identity) = identity else {
                                return Err(StepFailure::not_sent(unavailable(
                                    "has no identity the system gives, so it is not renamed",
                                )));
                            };
                            folder::rename(&at, &entry_name, &to, identity, ceiling)
                        }
                        Pending::Delete { digest } => folder::delete(
                            &at,
                            &entry_name,
                            kind,
                            folder::WasRead {
                                identity,
                                digest,
                                max_bytes: max,
                            },
                        ),
                    })
                    .await,
                    true,
                )?;
                let current = self.current(name)?;
                current.done = Some(change);
                current.renamed_to = to;
                Ok(StepOutcome::Done(Observed::default()))
            }
            StepSpec::FolderConfirm => {
                let spec = self.spec(name)?.clone();
                let current = self.current(name)?;
                let Some(change) = current.done else {
                    return Err(out_of_order("a confirm with nothing saved"));
                };
                let at = folder.clone();
                let names = current.name.clone();
                let renamed_to = current.renamed_to.clone().unwrap_or_default();
                let intended = current.intended;
                let identity = current.identity;
                let max = limits.max_bytes;
                let landed = joined(
                    tokio::task::spawn_blocking(move || -> Result<bool, StepFailure> {
                        Ok(match change {
                            FolderChange::Create | FolderChange::Replace => {
                                let named = folder::named(&at, &names, 0)?;
                                folder::digest(&named, max)? == intended && intended.is_some()
                            }
                            FolderChange::Rename => {
                                folder::find(&at, &names, 0, ceiling)?.is_none()
                                    && folder::identity(&at, &renamed_to, 0)? == identity
                                    && identity.is_some()
                            }
                            FolderChange::Delete => {
                                folder::find(&at, &names, spec.depth, ceiling)?.is_none()
                            }
                        })
                    })
                    .await,
                    false,
                )
                .unwrap_or(false);
                if landed {
                    Ok(StepOutcome::Done(Observed::default()))
                } else {
                    Err(StepFailure::answered(codes::failure(
                        &codes::row::CHANGED_SINCE_PREVIEW,
                        "The folder does not hold what was saved: another writer changed it since.",
                        "Read the entry back to see what the folder holds now.",
                    )))
                }
            }
            _ => Err(out_of_order("a document's step on a folder")),
        }
    }
}

fn must_be_file(entry: Option<&Entry>) -> Result<(), StepFailure> {
    match entry.map(|entry| entry.kind) {
        Some(Kind::File) => Ok(()),
        Some(_) => Err(StepFailure::not_sent(not_allowed(
            "it is not a regular file, and only a file's content is written or a file renamed",
        ))),
        None => Err(out_of_order("a write of an entry that was not found")),
    }
}

/// A file's digest under the folder's bound; nothing for anything else, or
/// for a file larger than the bound.
fn digest_of(at: &Folder, entry: &Entry, spec: &FolderSpec) -> Result<Value, StepFailure> {
    if entry.kind != Kind::File || entry.size.is_none_or(|size| size > spec.digest_max_bytes) {
        return Ok(Value::Null);
    }
    let named = folder::named(at, &entry.name, spec.depth)?;
    Ok(folder::digest(&named, spec.digest_max_bytes)?
        .map_or(Value::Null, |digest| Value::Text(digest.to_string())))
}

/// A small text file's content, as one value.
fn content_of(
    at: &Folder,
    entry: &Entry,
    spec: &FolderSpec,
    max: u64,
) -> Result<Value, StepFailure> {
    if entry.kind != Kind::File {
        return Err(StepFailure::not_sent(not_allowed(
            "it is not a regular file, and only a file's content is read",
        )));
    }
    let named = folder::named(at, &entry.name, spec.depth)?;
    let Some(bytes) = folder::read(&named, max)? else {
        return Err(stop(
            &codes::row::FILE_TOO_LARGE,
            "The file is larger than the most the folder's declaration lets the pack read as content.",
            "Raise `content_max_bytes` in the folder's file, or read the entry's digest instead.",
        ));
    };
    Ok(Value::Text(folder::text(bytes)?))
}
