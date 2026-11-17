//! The form a statement must have, read from its words.
//!
//! This is not how a read is proved a read: the store does that, on a
//! connection it holds read-only. It is a refusal made before the store is
//! asked, because some statements do harm that a read-only connection does
//! not stop: one that attaches another file, one that changes a setting of
//! the connection, one that ends the transaction it runs in. The compiler
//! makes the refusal for a statement of a target file, and the driver makes
//! it again for a statement of a plan, whoever made the plan.
//!
//! The check reads words, never meaning. A read is one statement whose
//! first word is `SELECT` or `WITH`; a write is one statement whose first
//! word is the one its verb names. Text inside quotes and comments is not
//! read at all.

use crate::model::Does;

/// Why a statement's form is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wrong {
    /// A statement declared a read holds a word that changes data.
    Writes,
    /// The statement is not one plain read or one keyed write; why, as a
    /// sentence.
    Form(&'static str),
}

impl Wrong {
    /// The refusal as a sentence.
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::Writes => {
                "A statement declared as a read holds a word that changes data: `INSERT`, `UPDATE`, `DELETE`, `REPLACE`, or `INTO`."
            }
            Self::Form(message) => message,
        }
    }
}

/// The words no statement of a plan may hold outside quotes. None can be
/// part of one read or one keyed write, so each is refused wherever it
/// stands.
const NEVER: &[&str] = &[
    "attach",
    "detach",
    "pragma",
    "vacuum",
    "create",
    "drop",
    "alter",
    "load_extension",
];

/// The words that change data.
const WRITES: &[&str] = &["insert", "update", "delete", "into"];

/// Whose rules of quoting and whose words a statement is read by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    /// The embedded engine: quotes `'`, `"`, `` ` ``, and `[ ]`.
    Embedded,
    /// The server store, PostgreSQL: quotes `'` and `"` only. A text with
    /// escapes (`E'...'`, `U&'...'`) and a text in dollar quotes are refused,
    /// because where they end is read differently by a reader that does not
    /// parse them, and a word could hide there.
    Server,
}

/// The words no statement for the server store may hold outside quotes: none
/// can be part of one read or one keyed write. `SET` is among them except in
/// an update, whose assignments it opens.
const NEVER_SERVER: &[&str] = &[
    "copy",
    "do",
    "call",
    "listen",
    "notify",
    "unlisten",
    "create",
    "alter",
    "drop",
    "grant",
    "revoke",
    "truncate",
    "vacuum",
    "lock",
    "prepare",
    "execute",
    "deallocate",
    "discard",
    "reset",
    "begin",
    "commit",
    "rollback",
    "savepoint",
    "load",
    "set_config",
    "current_setting",
    "dblink",
];

/// The beginnings of names that no statement for the server store may hold:
/// the server's own functions and catalog (`pg_`), its large objects
/// (`lo_`), and the functions that run a statement given as text.
const NEVER_SERVER_PREFIX: &[&str] = &["pg_", "lo_", "query_to_", "cursor_to_", "dblink_"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token<'a> {
    /// A bare word: a keyword, a name, or a function.
    Word(&'a str),
    /// `;`
    End,
    /// `(`
    Open,
    /// Anything else outside quotes and comments.
    Other,
}

fn word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn word_part(byte: u8) -> bool {
    word_start(byte) || byte.is_ascii_digit() || byte == b'$'
}

/// The place after the quoted text that opens at `at`, whose closing mark
/// is `close`. A doubled mark inside is part of the text.
fn after_quoted(bytes: &[u8], at: usize, close: u8) -> Option<usize> {
    let mut at = at + 1;
    loop {
        let found = at + bytes.get(at..)?.iter().position(|byte| *byte == close)?;
        if close != b']' && bytes.get(found + 1) == Some(&close) {
            at = found + 2;
        } else {
            return Some(found + 1);
        }
    }
}

/// The statement's tokens. Quoted text and comments give none.
fn tokens(text: &str, dialect: Dialect) -> Result<Vec<Token<'_>>, Wrong> {
    const OPEN: Wrong =
        Wrong::Form("The statement holds a quote or a comment that is never closed.");
    const ESCAPED: Wrong = Wrong::Form(
        "The statement holds a text with escapes or in dollar quotes. Write a text in plain single quotes, or bind it as a parameter.",
    );
    let server = dialect == Dialect::Server;
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        let next = bytes.get(at + 1).copied();
        match byte {
            _ if byte.is_ascii_whitespace() => at += 1,
            b'-' if next == Some(b'-') => {
                at = bytes
                    .get(at..)
                    .and_then(|rest| rest.iter().position(|byte| *byte == b'\n'))
                    .map_or(bytes.len(), |line| at + line + 1);
            }
            b'/' if next == Some(b'*') => {
                let rest = bytes.get(at + 2..).ok_or(OPEN)?;
                let close = rest.windows(2).position(|pair| pair == b"*/").ok_or(OPEN)?;
                at += 2 + close + 2;
            }
            b'\'' | b'"' => {
                // `E'...'` and `U&'...'` read a backslash as an escape.
                let escaped = at > 0
                    && match found.last() {
                        Some(Token::Word(word)) => {
                            word.eq_ignore_ascii_case("e")
                                && text
                                    .as_bytes()
                                    .get(at - 1)
                                    .is_some_and(u8::is_ascii_alphabetic)
                        }
                        Some(Token::Other) => text.as_bytes().get(at - 1) == Some(&b'&'),
                        _ => false,
                    };
                if server && escaped {
                    return Err(ESCAPED);
                }
                at = after_quoted(bytes, at, byte).ok_or(OPEN)?;
            }
            b'`' if !server => at = after_quoted(bytes, at, byte).ok_or(OPEN)?,
            b'[' if !server => at = after_quoted(bytes, at, b']').ok_or(OPEN)?,
            // A parameter is `$` and a number; anything else after `$` opens
            // a text in dollar quotes.
            b'$' if server && !next.is_some_and(|next| next.is_ascii_digit()) => {
                return Err(ESCAPED);
            }
            b';' => {
                found.push(Token::End);
                at += 1;
            }
            b'(' => {
                found.push(Token::Open);
                at += 1;
            }
            _ if word_start(byte) => {
                let length = bytes.get(at..).map_or(0, |rest| {
                    rest.iter().take_while(|byte| word_part(**byte)).count()
                });
                // A word begins and ends at a letter, a digit, `_`, `$`, or
                // a whole character beyond ASCII, so the cut is a valid one.
                let word = text.get(at..at + length).ok_or(OPEN)?;
                found.push(Token::Word(word));
                at += length;
            }
            _ if byte.is_ascii_digit() => {
                // A number, with whatever letters it carries: `1e5`, `0x1F`.
                found.push(Token::Other);
                at += bytes.get(at..).map_or(1, |rest| {
                    rest.iter()
                        .take_while(|byte| byte.is_ascii_alphanumeric() || **byte == b'.')
                        .count()
                });
            }
            _ => {
                found.push(Token::Other);
                at += 1;
            }
        }
    }
    Ok(found)
}

fn is(word: &str, among: &[&str]) -> bool {
    among.iter().any(|known| word.eq_ignore_ascii_case(known))
}

/// Holds a statement's text to the form its declaration states, by the
/// embedded engine's rules.
#[cfg(test)]
pub(crate) fn check(text: &str, does: Does) -> Result<(), Wrong> {
    check_in(text, does, Dialect::Embedded)
}

/// Holds a statement's text to the form its declaration states, by the
/// rules of the dialect given.
pub(crate) fn check_in(text: &str, does: Does, dialect: Dialect) -> Result<(), Wrong> {
    let tokens = tokens(text, dialect)?;
    // One statement: nothing follows the first `;` but more of them.
    if let Some(end) = tokens.iter().position(|token| *token == Token::End)
        && tokens.iter().skip(end).any(|token| *token != Token::End)
    {
        return Err(Wrong::Form(
            "The text holds more than one statement. A plan runs one statement at a time.",
        ));
    }
    let reads = does == Does::Read;
    for (position, token) in tokens.iter().enumerate() {
        let Token::Word(word) = token else {
            continue;
        };
        let called = tokens.get(position + 1) == Some(&Token::Open);
        if dialect == Dialect::Server
            && (is(word, NEVER_SERVER)
                || NEVER_SERVER_PREFIX.iter().any(|start| {
                    word.get(..start.len())
                        .is_some_and(|part| part.eq_ignore_ascii_case(start))
                })
                || (word.eq_ignore_ascii_case("set") && does != Does::Update))
        {
            return Err(Wrong::Form(
                "The statement copies, calls, or runs another statement, reads or changes a setting, changes the schema or the rights, takes a lock, listens or notifies, or ends its transaction. No statement of a plan may.",
            ));
        }
        if is(word, NEVER)
            || word
                .get(..7)
                .is_some_and(|start| start.eq_ignore_ascii_case("pragma_"))
        {
            return Err(Wrong::Form(
                "The statement attaches or detaches a file, reads or changes a setting, changes the schema, or loads an extension. No statement of a plan may.",
            ));
        }
        // `replace(a, b, c)` is a function of text; `REPLACE` alone writes.
        if reads && (is(word, WRITES) || (word.eq_ignore_ascii_case("replace") && !called)) {
            return Err(Wrong::Writes);
        }
    }
    let first = match tokens.first() {
        Some(Token::Word(word)) => *word,
        _ => "",
    };
    let fits = match does {
        Does::Read => is(first, &["select", "with"]),
        Does::Insert => is(first, &["insert"]),
        Does::Update => is(first, &["update"]),
        Does::Delete => is(first, &["delete"]),
    };
    if fits {
        Ok(())
    } else if reads {
        Err(Wrong::Form(
            "A read is one statement that begins with `SELECT` or `WITH`.",
        ))
    } else {
        Err(Wrong::Form(
            "A write is one `INSERT`, `UPDATE`, or `DELETE`, as its verb states.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Dialect, Wrong, check, check_in};
    use crate::model::Does;

    fn server(text: &str, does: Does) -> Result<(), Wrong> {
        check_in(text, does, Dialect::Server)
    }

    #[test]
    fn the_server_store_takes_one_plain_read_and_the_writes_the_pack_writes() {
        for text in [
            "SELECT a, b FROM t WHERE k = $1",
            "SELECT a FROM t WHERE note = 'it''s; DROP TABLE t' AND k = $2",
            "WITH recent AS (SELECT a FROM t) SELECT a FROM recent -- set role x\n",
            "SELECT \"set\", \"copy\", \"pg_x\" FROM t /* COPY t TO PROGRAM */",
            "SELECT a[1] FROM t",
        ] {
            assert_eq!(server(text, Does::Read), Ok(()), "{text}");
        }
        assert_eq!(
            server("UPDATE \"t\" SET \"a\" = $1 WHERE \"k\" = $2", Does::Update),
            Ok(())
        );
        assert_eq!(
            server(
                "INSERT INTO \"t\" (\"a\") VALUES ($1) RETURNING \"k\"",
                Does::Insert
            ),
            Ok(())
        );
    }

    #[test]
    fn the_server_store_refuses_what_its_read_only_transaction_does_not_stop() {
        for text in [
            "SELECT 1; SET ROLE x",
            "SET ROLE x",
            "SELECT set_config('role', 'x', false)",
            "SELECT current_setting('data_directory')",
            "SELECT pg_read_file('x')",
            "SELECT pg_catalog.pg_sleep(1)",
            "SELECT lo_import('x')",
            "SELECT query_to_xml('SET ROLE x', true, true, '')",
            "SELECT dblink('x', 'y')",
            "COPY t TO STDOUT",
            "DO $$ BEGIN END $$",
            "CALL p()",
            "LISTEN x",
            "NOTIFY x",
            "GRANT SELECT ON t TO PUBLIC",
            "CREATE TABLE x (a int)",
            "ALTER TABLE t ADD COLUMN b int",
            "DROP TABLE t",
            "TRUNCATE t",
            "LOCK TABLE t",
            "PREPARE p AS SELECT 1",
            "EXECUTE p",
            "DISCARD ALL",
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "SELECT 1 FROM t WHERE a = $$ ' $$ OR pg_sleep(1) IS NULL",
            "SELECT $tag$ ' $tag$",
            "SELECT E'\\' ' , pg_read_file('x') --'",
            "SELECT U&'\\0041'",
            "SELECT a FROM t WHERE k = $1 FOR UPDATE",
        ] {
            assert!(server(text, Does::Read).is_err(), "{text}");
        }
        // `SET` opens an update's assignments and nothing else.
        assert!(server("INSERT INTO t (a) VALUES ($1); SET ROLE x", Does::Insert).is_err());
        assert!(
            server(
                "DELETE FROM t WHERE k = $1 AND set_config('a', 'b', true) = 'b'",
                Does::Delete
            )
            .is_err()
        );
    }

    #[test]
    fn a_read_is_one_select_and_a_write_is_what_its_verb_states() {
        for text in [
            "SELECT a, b FROM t WHERE k = ?",
            "  select a from t where k = ? ; ",
            "WITH recent AS (SELECT a FROM t) SELECT a FROM recent -- the newest\n",
            "SELECT replace(a, 'x', 'y') AS a, \"update\", [delete], `into` FROM t /* ; DROP */",
            "SELECT a FROM t WHERE note = 'ATTACH it; PRAGMA x; it''s text'",
        ] {
            assert_eq!(check(text, Does::Read), Ok(()), "{text}");
        }
        assert_eq!(
            check("INSERT INTO \"t\" (\"a\") VALUES (?)", Does::Insert),
            Ok(())
        );
        assert_eq!(
            check("UPDATE \"t\" SET \"a\" = ? WHERE \"k\" = ?", Does::Update),
            Ok(())
        );
        assert_eq!(
            check("DELETE FROM \"t\" WHERE \"k\" = ?", Does::Delete),
            Ok(())
        );
    }

    #[test]
    fn a_read_that_holds_a_word_that_writes_is_named_so() {
        for text in [
            "UPDATE t SET a = a + 1 WHERE k = ? RETURNING a",
            "WITH one AS (SELECT 1) DELETE FROM t",
            "WITH one AS (SELECT 1) REPLACE INTO t VALUES (1)",
            "INSERT INTO t VALUES (?)",
            "REPLACE INTO t VALUES (?)",
        ] {
            assert_eq!(check(text, Does::Read), Err(Wrong::Writes), "{text}");
        }
    }

    #[test]
    fn everything_else_is_refused_by_its_form() {
        for text in [
            "",
            "-- nothing",
            "ATTACH ? AS x",
            "DETACH x",
            "PRAGMA query_only = 0",
            "pragma query_only",
            "VACUUM INTO ?",
            "VACUUM",
            "CREATE TABLE x (a)",
            "DROP TABLE t",
            "ALTER TABLE t ADD COLUMN b",
            "COMMIT",
            "BEGIN",
            "ROLLBACK",
            "SAVEPOINT s",
            "EXPLAIN SELECT 1",
            "ANALYZE",
            "REINDEX",
            "VALUES (1)",
            "SELECT 1; SELECT 2",
            "SELECT 1; COMMIT",
            "SELECT load_extension(?)",
            "SELECT file FROM pragma_database_list",
            "SELECT * FROM PRAGMA_table_info('t')",
            "SELECT 'never closed",
            "SELECT a /* never closed",
            "SELECT \"a FROM t",
            "SELECT [a FROM t",
        ] {
            assert!(
                matches!(check(text, Does::Read), Err(Wrong::Form(_))),
                "{text}"
            );
        }
        // A write is the statement its verb states, and no other.
        for (text, does) in [
            ("UPDATE t SET a = ?", Does::Insert),
            ("DELETE FROM t", Does::Update),
            ("SELECT 1", Does::Delete),
            ("WITH one AS (SELECT 1) UPDATE t SET a = 1", Does::Update),
            ("UPDATE t SET a = ?; DELETE FROM t", Does::Update),
            ("COMMIT", Does::Update),
            ("ATTACH ? AS x", Does::Insert),
        ] {
            assert!(matches!(check(text, does), Err(Wrong::Form(_))), "{text}");
        }
    }
}
