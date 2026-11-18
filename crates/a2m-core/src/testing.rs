//! Help for one test other crates keep. Not part of the interface.
//!
//! Every crate that makes its failures from the rows of a
//! [`failure_codes!`](crate::failure_codes) table keeps a test,
//! `every_failure_takes_its_class_from_a_table`, which reads the crate's
//! sources and finds no place that gives a failure a class of its own
//! (decision 0289). The reading is here, once, so the copies cannot drift.
//!
//! This module reads files, which nothing else in this crate does. It is
//! built for this crate's own tests and under the feature `testing`, which
//! a crate turns on only as a feature of a dev-dependency.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// What gives a failure a class of its own where it is made.
pub const CLASS_OF_ITS_OWN: [&str; 2] = ["Failure::new(", "class: FailureClass::"];

const TEST_ONLY: &[u8] = b"#[cfg(test)]";

/// Each place in the Rust files under `src` that gives a failure a class
/// of its own, outside tests, as `path:line: what`, with the path relative
/// to `src` and separated by `/`. Empty when there is none.
///
/// Tests are what `#[cfg(test)]` marks: a module, inline or in its own
/// file or folder, and any single item, statement, field, or arm. Code
/// after such an item is read. Any other condition is read as code.
///
/// `remakes` names the files that remake a failure in the class it already
/// has, each with the exact number of places it does so. A file with more
/// or fewer, or a named file that is not there, is reported too, so a new
/// remake fails the test until it is counted.
///
/// # Errors
///
/// When a folder or a file under `src` cannot be read.
pub fn classes_of_their_own(src: &Path, remakes: &[(&str, usize)]) -> io::Result<Vec<String>> {
    let mut files = Vec::new();
    rust_files(src, &mut files)?;
    files.sort();
    let mut read = BTreeMap::new();
    let mut test_only = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path)?;
        let (kept, modules) = outside_tests(&text);
        let dir = module_dir(path);
        test_only.extend(modules.into_iter().map(|name| dir.join(name)));
        read.insert(path.clone(), kept);
    }
    let mut found = Vec::new();
    let mut counted: BTreeMap<String, usize> = BTreeMap::new();
    for (path, kept) in &read {
        if test_only
            .iter()
            .any(|module| path.starts_with(module) || *path == module.with_extension("rs"))
        {
            continue;
        }
        let shown = shown_path(src, path);
        let allowed = remakes.iter().any(|(file, _)| *file == shown);
        for (number, line) in kept.lines().enumerate() {
            for what in CLASS_OF_ITS_OWN {
                let times = line.matches(what).count();
                if times == 0 {
                    continue;
                }
                if allowed {
                    *counted.entry(shown.clone()).or_default() += times;
                } else {
                    found.push(format!("{shown}:{}: {what}", number + 1));
                }
            }
        }
    }
    for (file, allowed) in remakes {
        let present = read.keys().any(|path| shown_path(src, path) == *file);
        let made = counted.get(*file).copied().unwrap_or(0);
        if !present {
            found.push(format!(
                "{file}: named as remaking a failure, and not there"
            ));
        } else if made != *allowed {
            found.push(format!(
                "{file}: the test allows {allowed} remakes and finds {made}"
            ));
        }
    }
    Ok(found)
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// The folder in which the modules a file declares have their files.
fn module_dir(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or(Path::new(""));
    match path.file_name().and_then(|name| name.to_str()) {
        Some("lib.rs" | "main.rs" | "mod.rs") | None => parent.to_path_buf(),
        Some(_) => match path.file_stem() {
            Some(stem) => parent.join(stem),
            None => parent.to_path_buf(),
        },
    }
}

fn shown_path(src: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(src).unwrap_or(path);
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// `text` with every item `#[cfg(test)]` marks turned to spaces, lines
/// kept, and the names of the modules among them.
pub fn outside_tests(text: &str) -> (String, Vec<String>) {
    let bytes = text.as_bytes();
    let mut kept = bytes.to_vec();
    let mut modules = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if let Some(past) = skip_literal(bytes, at) {
            at = past;
        } else if bytes[at..].starts_with(TEST_ONLY) {
            let start = past_attributes(bytes, at + TEST_ONLY.len());
            if let Some(name) = module_name(bytes, start) {
                modules.push(name);
            }
            let end = item_end(bytes, start);
            for byte in kept.iter_mut().take(end).skip(at) {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            at = end;
        } else {
            at += 1;
        }
    }
    // Only whole runs of bytes that begin and end at ASCII were blanked.
    (String::from_utf8_lossy(&kept).into_owned(), modules)
}

fn byte(bytes: &[u8], at: usize) -> u8 {
    bytes.get(at).copied().unwrap_or(0)
}

const fn ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Past white space, comments, and outer attributes from `at`.
fn past_attributes(bytes: &[u8], mut at: usize) -> usize {
    loop {
        while byte(bytes, at).is_ascii_whitespace() {
            at += 1;
        }
        if byte(bytes, at) == b'/' && matches!(byte(bytes, at + 1), b'/' | b'*') {
            at = skip_literal(bytes, at).unwrap_or(at + 1);
        } else if byte(bytes, at) == b'#' && byte(bytes, at + 1) == b'[' {
            at = group_end(bytes, at + 1);
        } else {
            return at;
        }
    }
}

/// The name of the module that starts at `at`, if a module does.
fn module_name(bytes: &[u8], mut at: usize) -> Option<String> {
    let word = |at: usize| {
        let end = (at..bytes.len())
            .find(|end| !ident(byte(bytes, *end)))
            .unwrap_or(bytes.len());
        (String::from_utf8_lossy(&bytes[at..end]).into_owned(), end)
    };
    let spaces = |mut at: usize| {
        while byte(bytes, at).is_ascii_whitespace() {
            at += 1;
        }
        at
    };
    let (mut first, mut end) = word(at);
    if first == "pub" {
        at = spaces(end);
        if byte(bytes, at) == b'(' {
            at = group_end(bytes, at);
        }
        (first, end) = word(spaces(at));
    }
    if first != "mod" {
        return None;
    }
    let (name, _) = word(spaces(end));
    (!name.is_empty()).then_some(name)
}

/// Past the bracket that closes the one at `at`.
fn group_end(bytes: &[u8], mut at: usize) -> usize {
    let mut depth = 0usize;
    while at < bytes.len() {
        if let Some(past) = skip_literal(bytes, at) {
            at = past;
            continue;
        }
        match byte(bytes, at) {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return at + 1;
                }
            }
            _ => {}
        }
        at += 1;
    }
    bytes.len()
}

/// Where the item that starts at `at` ends: past the `;` or the `,` that
/// ends it, past the brace that closes its body, or before a bracket that
/// closes what holds it.
fn item_end(bytes: &[u8], mut at: usize) -> usize {
    let mut open = Vec::new();
    // Angle brackets of generics, so that a comma in `<A, B>` ends nothing.
    // Code is formatted: a comparison has spaces around it, an arrow is
    // `->` or `=>`.
    let mut angles = 0usize;
    while at < bytes.len() {
        if let Some(past) = skip_literal(bytes, at) {
            at = past;
            continue;
        }
        let before = if at == 0 { b' ' } else { byte(bytes, at - 1) };
        match byte(bytes, at) {
            b'(' | b'[' | b'{' => open.push(byte(bytes, at)),
            b')' | b']' | b'}' => match open.pop() {
                None => return at,
                Some(b'{') if open.is_empty() => return at + 1,
                Some(_) => {}
            },
            b';' if open.is_empty() => return at + 1,
            b',' if open.is_empty() && angles == 0 => return at + 1,
            b'<' if open.is_empty() && (ident(before) || before == b':') => angles += 1,
            b'>' if open.is_empty()
                && angles > 0
                && !before.is_ascii_whitespace()
                && before != b'-'
                && before != b'=' =>
            {
                angles -= 1;
            }
            _ => {}
        }
        at += 1;
    }
    bytes.len()
}

/// Past the comment, string, or character that starts at `at`, if one
/// does.
fn skip_literal(bytes: &[u8], at: usize) -> Option<usize> {
    let here = byte(bytes, at);
    let next = byte(bytes, at + 1);
    let before = if at == 0 { b' ' } else { byte(bytes, at - 1) };
    match here {
        b'/' if next == b'/' => Some(
            (at..bytes.len())
                .find(|end| byte(bytes, *end) == b'\n')
                .unwrap_or(bytes.len()),
        ),
        b'/' if next == b'*' => {
            let mut depth = 0usize;
            let mut end = at;
            while end < bytes.len() {
                if byte(bytes, end) == b'/' && byte(bytes, end + 1) == b'*' {
                    depth += 1;
                    end += 2;
                } else if byte(bytes, end) == b'*' && byte(bytes, end + 1) == b'/' {
                    depth -= 1;
                    end += 2;
                    if depth == 0 {
                        return Some(end);
                    }
                } else {
                    end += 1;
                }
            }
            Some(bytes.len())
        }
        b'"' => Some(string_end(bytes, at + 1)),
        b'b' | b'c' if next == b'"' && !ident(before) => Some(string_end(bytes, at + 2)),
        b'r' if matches!(next, b'"' | b'#') && !ident(before) => raw_end(bytes, at + 1),
        b'b' | b'c' if next == b'r' && !ident(before) => raw_end(bytes, at + 2),
        b'\'' => {
            if next == b'\\' {
                let end = (at + 3..bytes.len()).find(|end| byte(bytes, *end) == b'\'')?;
                return Some(end + 1);
            }
            // One character, of one to four bytes, then the closing quote;
            // anything else is a lifetime or a label.
            let width = match next {
                0..=0x7f => 1,
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                _ => 4,
            };
            (byte(bytes, at + 1 + width) == b'\'').then_some(at + 2 + width)
        }
        _ => None,
    }
}

/// Past the closing quote of a string whose text starts at `at`.
fn string_end(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() {
        match byte(bytes, at) {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}

/// Past the end of a raw string whose hashes, or quote, start at `at`.
fn raw_end(bytes: &[u8], at: usize) -> Option<usize> {
    let hashes = (at..bytes.len())
        .take_while(|end| byte(bytes, *end) == b'#')
        .count();
    if byte(bytes, at + hashes) != b'"' {
        return None;
    }
    let mut end = at + hashes + 1;
    while end < bytes.len() {
        if byte(bytes, end) == b'"' && (1..=hashes).all(|offset| byte(bytes, end + offset) == b'#')
        {
            return Some(end + 1 + hashes);
        }
        end += 1;
    }
    Some(bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept(text: &str) -> String {
        let (kept, _) = outside_tests(text);
        assert_eq!(kept.lines().count(), text.lines().count());
        kept.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn a_test_module_goes_and_the_code_after_it_stays() {
        let text = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() { let s = \"}\"; let c = '}'; }\n    /* } */ // }\n}\nfn c() {}\n";
        assert_eq!(kept(text), "fn a() {} fn c() {}");
        assert_eq!(outside_tests(text).1, ["tests"]);
    }

    #[test]
    fn a_single_item_goes_and_only_it() {
        let text = "#[cfg(test)]\n#[allow(dead_code)]\npub(crate) fn f<A, B>(a: A) -> Vec<(A, B)> {\n    x\n}\nfn g() {}\n";
        assert_eq!(kept(text), "fn g() {}");
        let fields =
            "struct S {\n    a: u8,\n    #[cfg(test)]\n    seen: Vec<String>,\n    b: u8,\n}\n";
        assert_eq!(kept(fields), "struct S { a: u8, b: u8, }");
        let statement = "fn f() {\n    #[cfg(test)]\n    self.seen.clear();\n    g();\n    #[cfg(test)]\n    {\n        h();\n    }\n    i();\n}\n";
        assert_eq!(kept(statement), "fn f() { g(); i(); }");
        let declared = "#[cfg(test)]\nmod tests;\nfn g() {}\n";
        assert_eq!(kept(declared), "fn g() {}");
        assert_eq!(outside_tests(declared).1, ["tests"]);
    }

    #[test]
    fn other_conditions_and_quoted_markers_are_code() {
        let text = "#[cfg(any(windows, test))]\nfn f() {}\nconst S: &str = \"#[cfg(test)] fn\";\nconst R: &str = r#\"#[cfg(test)]\"#;\nfn g<'a>(x: &'a str) {}\n";
        assert_eq!(
            kept(text),
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }

    #[test]
    fn files_of_test_modules_are_skipped_and_remakes_are_counted() {
        let src = std::env::temp_dir().join(format!("a2m-core-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(src.join("tests")).unwrap();
        std::fs::write(
            src.join("lib.rs"),
            "mod a;\nmod remade;\n#[cfg(test)]\nmod tests {\n    mod inner;\n}\n",
        )
        .unwrap();
        std::fs::write(
            src.join("a.rs"),
            "#[cfg(test)]\nfn helper() {}\nfn f() { Failure::new(c, d, m, n); }\n",
        )
        .unwrap();
        std::fs::write(
            src.join("remade.rs"),
            "fn f() { Failure::new(c, d, m, n); }\n",
        )
        .unwrap();
        std::fs::write(src.join("tests").join("inner.rs"), "Failure::new(\n").unwrap();
        let found = classes_of_their_own(&src, &[("remade.rs", 1)]).unwrap();
        assert_eq!(found, ["a.rs:3: Failure::new("]);
        let found = classes_of_their_own(&src, &[("remade.rs", 0), ("gone.rs", 0)]).unwrap();
        assert_eq!(
            found,
            [
                "a.rs:3: Failure::new(",
                "remade.rs: the test allows 0 remakes and finds 1",
                "gone.rs: named as remaking a failure, and not there",
            ]
        );
        std::fs::remove_dir_all(&src).unwrap();
    }
}
