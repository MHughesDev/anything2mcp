//! A log file that is rotated by size.
//!
//! Lines are appended to one file. When the next line would take the file past
//! its limit, the file is renamed with a number, older files move up by one,
//! and the oldest beyond the count kept is deleted. A line is never split
//! across two files.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// How large a log file may grow and how many old ones are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rotation {
    /// The size, in bytes, a file is not allowed to pass.
    pub(crate) max_bytes: u64,
    /// How many rotated files are kept beside the current one.
    pub(crate) keep: u32,
}

/// The file being written, and the rule for rotating it.
#[derive(Debug)]
pub(crate) struct RotatingFile {
    path: PathBuf,
    rotation: Rotation,
    file: Option<File>,
    written: u64,
}

impl RotatingFile {
    /// Opens `path` for appending, creating the folder and the file as needed.
    pub(crate) fn open(path: &Path, rotation: Rotation) -> io::Result<Self> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let (file, written) = open_for_append(path)?;
        Ok(Self {
            path: path.to_owned(),
            rotation,
            file: Some(file),
            written,
        })
    }

    /// The path of the file being written.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one line, which must already end with a line feed, rotating
    /// first when the file would otherwise pass its limit.
    pub(crate) fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        let length = line.len() as u64;
        if self.written > 0 && self.written.saturating_add(length) > self.rotation.max_bytes {
            self.rotate()?;
        }
        let file = match &mut self.file {
            Some(file) => file,
            None => {
                let (file, written) = open_for_append(&self.path)?;
                self.written = written;
                self.file.insert(file)
            }
        };
        file.write_all(line)?;
        self.written = self.written.saturating_add(length);
        Ok(())
    }

    /// Closes the current file, moves it and the older ones up by one, and
    /// leaves the next write to open a new file.
    fn rotate(&mut self) -> io::Result<()> {
        // The file is closed before it is renamed: Windows refuses to rename
        // an open file.
        self.file = None;
        if self.rotation.keep == 0 {
            remove_if_present(&self.path)?;
        } else {
            remove_if_present(&numbered(&self.path, self.rotation.keep))?;
            for number in (1..self.rotation.keep).rev() {
                let from = numbered(&self.path, number);
                if from.exists() {
                    fs::rename(&from, numbered(&self.path, number + 1))?;
                }
            }
            fs::rename(&self.path, numbered(&self.path, 1))?;
        }
        self.written = 0;
        Ok(())
    }
}

/// The path of the `number`th rotated file: the log's own name with the
/// number after it, so `engine.log.1` is the newest of the old files.
pub(crate) fn numbered(path: &Path, number: u32) -> PathBuf {
    let mut name = path.file_name().map(ToOwned::to_owned).unwrap_or_default();
    name.push(format!(".{number}"));
    path.with_file_name(name)
}

fn open_for_append(path: &Path) -> io::Result<(File, u64)> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let written = file.metadata()?.len();
    Ok((file, written))
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{RotatingFile, Rotation, numbered};
    use crate::testing::TempFolder;

    #[test]
    fn a_full_file_is_rotated_and_the_oldest_is_deleted() {
        let folder = TempFolder::new("rotate");
        let path = folder.path().join("logs").join("engine.log");
        let rotation = Rotation {
            max_bytes: 20,
            keep: 2,
        };
        let mut log = RotatingFile::open(&path, rotation).unwrap();
        // Each line is 10 bytes, so a file holds two.
        for number in 0..7 {
            log.write_line(format!("line {number:03}\n").as_bytes())
                .unwrap();
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), "line 006\n");
        assert_eq!(
            fs::read_to_string(numbered(&path, 1)).unwrap(),
            "line 004\nline 005\n"
        );
        assert_eq!(
            fs::read_to_string(numbered(&path, 2)).unwrap(),
            "line 002\nline 003\n"
        );
        assert!(!numbered(&path, 3).exists());
    }

    #[test]
    fn a_line_longer_than_the_limit_is_written_whole_in_its_own_file() {
        let folder = TempFolder::new("rotate-long");
        let path = folder.path().join("engine.log");
        let rotation = Rotation {
            max_bytes: 8,
            keep: 1,
        };
        let mut log = RotatingFile::open(&path, rotation).unwrap();
        log.write_line(b"short\n").unwrap();
        log.write_line(b"a line longer than the limit\n").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "a line longer than the limit\n"
        );
        assert_eq!(fs::read_to_string(numbered(&path, 1)).unwrap(), "short\n");
    }

    #[test]
    fn an_existing_file_is_appended_to_and_counted() {
        let folder = TempFolder::new("rotate-append");
        let path = folder.path().join("engine.log");
        fs::write(&path, "0123456789\n").unwrap();
        let rotation = Rotation {
            max_bytes: 16,
            keep: 1,
        };
        let mut log = RotatingFile::open(&path, rotation).unwrap();
        log.write_line(b"next\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "0123456789\nnext\n");
        log.write_line(b"again\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "again\n");
        assert_eq!(
            fs::read_to_string(numbered(&path, 1)).unwrap(),
            "0123456789\nnext\n"
        );
    }

    #[test]
    fn keeping_no_old_files_starts_the_file_again() {
        let folder = TempFolder::new("rotate-none");
        let path = folder.path().join("engine.log");
        let rotation = Rotation {
            max_bytes: 8,
            keep: 0,
        };
        let mut log = RotatingFile::open(&path, rotation).unwrap();
        log.write_line(b"first\n").unwrap();
        log.write_line(b"second\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second\n");
        assert!(!numbered(&path, 1).exists());
        assert_eq!(log.path(), path);
    }
}
