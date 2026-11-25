//! The reader of the engine's recent log lines, for `engine logs`.
//!
//! It reads `engine.log` and the files rotated out of it, newest first, by
//! the names [`numbered`] gives them, and makes nothing: a folder or a file
//! that is not there is an empty log, not a failure. It reads the lines as
//! the writer left them, already redacted (spec section 217, "Logs").

use std::fs;
use std::io;
use std::path::Path;

use a2m_core::{Failure, FailureClass};

use crate::codes::LOG_UNREADABLE;
use crate::log::LOG_FILE_NAME;
use crate::rotate::numbered;

/// The most rotated files the reader looks at beside the current one, so
/// that a folder of many numbered files is never walked without end.
const FILES_LOOKED_AT: u32 = 1_000;

fn unreadable(kind: io::ErrorKind) -> Failure {
    // The path is not named: the caller knows it, and says it.
    Failure::new(
        FailureClass::Busy,
        LOG_UNREADABLE,
        format!("A file of the engine's log is there and cannot be read ({kind})."),
        "Make the logs folder and its files readable by your user, then ask for the log again.",
    )
}

/// The last `lines` lines of the engine's log in the folder `logs`, oldest
/// first: from `engine.log`, and before it from `engine.log.1`, `.2`, and
/// on, until enough are found or a numbered file is missing. Empty lines
/// are left out. A last line of the current file with no line feed after
/// it is still being written, and is left out too.
///
/// It makes nothing, and it opens no file for writing. A folder that is
/// not there gives an empty list.
///
/// # Errors
///
/// `busy`/`log_unreadable` when a file of the log is there and cannot be
/// read.
pub fn recent_lines(logs: &Path, lines: usize) -> Result<Vec<String>, Failure> {
    let current = logs.join(LOG_FILE_NAME);
    // Newest first, while they are gathered.
    let mut found: Vec<String> = Vec::new();
    for number in 0..=FILES_LOOKED_AT {
        if found.len() >= lines {
            break;
        }
        let path = if number == 0 {
            current.clone()
        } else {
            numbered(&current, number)
        };
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            // Just after a rotation the current file is not there until the
            // next line is written; the numbered files may still be.
            Err(error) if error.kind() == io::ErrorKind::NotFound && number == 0 => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(unreadable(error.kind())),
        };
        let text = String::from_utf8_lossy(&bytes);
        let mut whole: Vec<&str> = text.split('\n').collect();
        // The piece after the last line feed: empty, or a line not yet
        // finished. A rotated file is never written again, so only the
        // current file can hold one.
        whole.pop();
        for line in whole.into_iter().rev() {
            if found.len() >= lines {
                break;
            }
            let line = line.strip_suffix('\r').unwrap_or(line);
            if !line.trim().is_empty() {
                found.push(line.to_owned());
            }
        }
    }
    found.reverse();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::recent_lines;
    use crate::rotate::numbered;
    use crate::testing::TempFolder;

    #[test]
    fn the_last_lines_are_read_across_the_rotated_files_oldest_first() {
        let folder = TempFolder::new("recent");
        let current = folder.path().join("engine.log");
        fs::write(numbered(&current, 2), "one\ntwo\n").unwrap();
        fs::write(numbered(&current, 1), "three\n\nfour\n").unwrap();
        fs::write(&current, "five\nsix\nhalf-written").unwrap();
        assert_eq!(
            recent_lines(folder.path(), 3).unwrap(),
            ["four", "five", "six"]
        );
        assert_eq!(
            recent_lines(folder.path(), 100).unwrap(),
            ["one", "two", "three", "four", "five", "six"]
        );
        assert!(recent_lines(folder.path(), 0).unwrap().is_empty());
        // Just after a rotation, before the next line is written.
        fs::remove_file(&current).unwrap();
        assert_eq!(recent_lines(folder.path(), 2).unwrap(), ["three", "four"]);
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_empty_log_and_is_not_made() {
        let folder = TempFolder::new("recent-absent");
        let logs = folder.path().join("data").join("logs");
        assert!(recent_lines(&logs, 50).unwrap().is_empty());
        assert!(!folder.path().join("data").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_read_is_refused_by_its_code() {
        use std::os::unix::fs::PermissionsExt as _;
        let folder = TempFolder::new("recent-closed");
        let current = folder.path().join("engine.log");
        fs::write(&current, "line\n").unwrap();
        fs::set_permissions(&current, fs::Permissions::from_mode(0o000)).unwrap();
        // An account that reads every file anyway cannot show the refusal.
        if fs::read(&current).is_ok() {
            println!("skipped: this account reads a file closed to it");
            return;
        }
        let refused = recent_lines(folder.path(), 5).unwrap_err();
        assert_eq!(refused.code().as_str(), "log_unreadable");
        assert_eq!(refused.class(), a2m_core::FailureClass::Busy);
        assert!(!refused.message().contains("engine.log"), "{refused:?}");
    }
}
