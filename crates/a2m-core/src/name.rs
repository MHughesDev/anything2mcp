//! Checked names, and the two ways a name can be wrong.

use std::fmt;

/// The longest name shown back in an error. A name comes from a file or a
/// caller and may be of any length.
const SHOWN: usize = 64;

/// A name that is not one of a closed vocabulary's values.
///
/// It maps to the failure class `input`, code `schema_invalid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownName {
    kind: &'static str,
    name: String,
    known: &'static [&'static str],
}

impl UnknownName {
    pub(crate) fn new(kind: &'static str, name: &str, known: &'static [&'static str]) -> Self {
        Self {
            kind,
            name: shorten(name),
            known,
        }
    }

    /// What kind of word was expected, such as "run state".
    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// The names that would have been accepted.
    #[must_use]
    pub fn known(&self) -> &'static [&'static str] {
        self.known
    }
}

impl fmt::Display for UnknownName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a {}; expected one of: {}",
            self.name,
            self.kind,
            self.known.join(", ")
        )
    }
}

impl std::error::Error for UnknownName {}

/// A name that does not have the shape every registered name must have.
///
/// It maps to the failure class `input`, code `schema_invalid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidName {
    kind: &'static str,
    name: String,
    longest: usize,
    dotted: bool,
}

impl InvalidName {
    /// What kind of name was expected, such as "mode".
    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.kind
    }
}

impl fmt::Display for InvalidName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.dotted {
            return write!(
                f,
                "`{}` is not a valid {}; it is two names joined by one dot, each starting with a \
                 lowercase letter and continuing with lowercase letters, digits, and underscores, \
                 and it has at most {} characters",
                self.name, self.kind, self.longest
            );
        }
        write!(
            f,
            "`{}` is not a valid {} name; a name starts with a lowercase letter, continues with \
             lowercase letters, digits, and underscores, and has at most {} characters",
            self.name, self.kind, self.longest
        )
    }
}

impl std::error::Error for InvalidName {}

/// Whether `text` has the shape of a registered name: `[a-z][a-z0-9_]*`, at
/// most `longest` characters.
#[must_use]
pub const fn is_name(text: &str, longest: usize) -> bool {
    is_name_bytes(text.as_bytes(), longest)
}

const fn is_name_bytes(bytes: &[u8], longest: usize) -> bool {
    if bytes.is_empty() || bytes.len() > longest || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

/// Whether `text` is two names joined by one dot, such as
/// `kernel.no_fixed_wait`: each part a name, the whole at most `longest`
/// characters.
#[must_use]
pub const fn is_dotted_name(text: &str, longest: usize) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() > longest {
        return false;
    }
    let mut dot = bytes.len();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.' {
            dot = i;
            break;
        }
        i += 1;
    }
    if dot == bytes.len() {
        return false;
    }
    let (owner, rest) = bytes.split_at(dot);
    let (_, rule) = rest.split_at(1);
    is_name_bytes(owner, longest) && is_name_bytes(rule, longest)
}

/// Checks a name's shape, and says what is wrong when it is not a name.
///
/// # Errors
///
/// [`InvalidName`] when `text` is not a name of at most `longest` characters.
pub fn check_name(
    kind: &'static str,
    text: &str,
    longest: usize,
) -> Result<(), InvalidName> {
    if is_name(text, longest) {
        Ok(())
    } else {
        Err(InvalidName {
            kind,
            name: shorten(text),
            longest,
            dotted: false,
        })
    }
}

/// Checks the shape of two names joined by one dot, such as
/// `kernel.no_fixed_wait`: each part a name, the whole at most `longest`
/// characters.
///
/// # Errors
///
/// [`InvalidName`] when `text` is not two names joined by one dot.
pub fn check_dotted_name(
    kind: &'static str,
    text: &str,
    longest: usize,
) -> Result<(), InvalidName> {
    if is_dotted_name(text, longest) {
        Ok(())
    } else {
        Err(InvalidName {
            kind,
            name: shorten(text),
            longest,
            dotted: true,
        })
    }
}

/// Defines a checked name: a word whose values arrive with packs, crates, or
/// target files, and whose shape alone is checked (decisions 0060 and 0062).
///
/// ```
/// a2m_core::checked_name! {
///     /// The name of a queue.
///     pub struct QueueName: "queue", 32;
/// }
/// const DEFAULT: QueueName = QueueName::fixed("default");
/// assert_eq!(QueueName::new("default").unwrap(), DEFAULT);
/// assert!(QueueName::new("Not a name").is_err());
/// ```
///
/// The type gets `LONGEST`, `new`, `fixed`, `as_str`, `Display`, `FromStr`,
/// and serde support that refuses a badly formed name. A name is lowercase
/// letters, digits, and underscores, starting with a letter; with `dotted`
/// after the length, it is two such names joined by one dot. `fixed` is for
/// a name written in the source: used in a `const`, a badly formed name is an
/// error when the crate is built. Whether anyone registered the name is
/// never this type's question.
///
/// A crate that uses the macro needs no dependency beyond this one.
#[macro_export]
macro_rules! checked_name {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident: $kind:literal, $longest:literal;
    ) => {
        $crate::checked_name! {
            @define
            $(#[$meta])*
            $vis struct $name: $kind, $longest,
            $crate::__name::check_name, $crate::__name::is_name;
        }
    };
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident: $kind:literal, $longest:literal, dotted;
    ) => {
        $crate::checked_name! {
            @define
            $(#[$meta])*
            $vis struct $name: $kind, $longest,
            $crate::__name::check_dotted_name, $crate::__name::is_dotted_name;
        }
    };
    (
        @define
        $(#[$meta:meta])*
        $vis:vis struct $name:ident: $kind:literal, $longest:literal, $check:path, $is:path;
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis struct $name(::std::borrow::Cow<'static, str>);

        impl $name {
            /// The longest the name may be.
            pub const LONGEST: usize = $longest;

            /// Checks `name` and keeps it.
            ///
            /// # Errors
            ///
            /// `InvalidName` when the name does not have the shape described
            /// on the type, or is longer than `LONGEST`.
            pub fn new(name: &str) -> ::std::result::Result<Self, $crate::InvalidName> {
                $check($kind, name, Self::LONGEST)?;
                Ok(Self(::std::borrow::Cow::Owned(name.to_owned())))
            }

            /// Makes the name from text written in the source.
            ///
            /// # Panics
            ///
            /// When the text does not have the shape described on the type.
            /// Used in a `const`, as it is meant to be, that is an error when
            /// the crate is built and never at run time.
            #[must_use]
            pub const fn fixed(name: &'static str) -> Self {
                assert!(
                    $is(name, Self::LONGEST),
                    "a name written in the source does not have the shape its type requires"
                );
                Self(::std::borrow::Cow::Borrowed(name))
            }

            /// The name, as it is written in files and messages.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::InvalidName;

            fn from_str(text: &str) -> ::std::result::Result<Self, Self::Err> {
                Self::new(text)
            }
        }

        impl $crate::__name::serde::Serialize for $name {
            fn serialize<S: $crate::__name::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> $crate::__name::serde::Deserialize<'de> for $name {
            fn deserialize<D: $crate::__name::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::std::result::Result<Self, D::Error> {
                let text = <::std::string::String as $crate::__name::serde::Deserialize>::deserialize(
                    deserializer,
                )?;
                Self::new(&text).map_err($crate::__name::serde::de::Error::custom)
            }
        }
    };
}

/// At most `SHOWN` characters of `text`, with a mark when it was cut.
fn shorten(text: &str) -> String {
    if text.chars().count() <= SHOWN {
        text.to_owned()
    } else {
        let mut cut: String = text.chars().take(SHOWN).collect();
        cut.push('…');
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::{check_name, is_name, shorten};

    #[test]
    fn a_name_is_lowercase_letters_digits_and_underscores() {
        for good in ["ui", "page_http", "a", "v2", "needs_login"] {
            assert!(is_name(good, 32), "{good}");
        }
        for bad in [
            "",
            "UI",
            "2fa",
            "_x",
            "page-http",
            "page http",
            "naïve",
            "a.b",
        ] {
            assert!(!is_name(bad, 32), "{bad}");
        }
    }

    #[test]
    fn a_name_has_a_longest_length() {
        assert!(is_name(&"a".repeat(32), 32));
        assert!(!is_name(&"a".repeat(33), 32));
    }

    #[test]
    fn a_wrong_name_is_reported_with_the_rule() {
        let error = check_name("mode", "Page-HTTP", 32).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("`Page-HTTP`"), "{text}");
        assert!(text.contains("mode name"), "{text}");
        assert!(text.contains("at most 32"), "{text}");
    }

    #[test]
    fn a_very_long_name_is_cut_before_it_is_shown() {
        let long = "x".repeat(10_000);
        let shown = shorten(&long);
        assert_eq!(shown.chars().count(), 65);
        assert!(shown.ends_with('…'));
        assert!(check_name("mode", &long, 32).unwrap_err().to_string().len() < 400);
    }
}
