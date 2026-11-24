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
    shape: Shape,
    /// The part of the name that Windows keeps for a device, when that is
    /// the only thing wrong with it (decision 0431).
    device: Option<String>,
}

/// Which shape a name was held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// One name.
    Name,
    /// Two names joined by one dot.
    Dotted,
    /// A reference to a stored credential.
    Reference,
}

impl InvalidName {
    /// What kind of name was expected, such as "mode".
    #[must_use]
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// The part of the name that Windows keeps for a device, such as `con`,
    /// when that is the only thing wrong with it (decision 0431).
    #[must_use]
    pub fn kept_for_a_device(&self) -> Option<&str> {
        self.device.as_deref()
    }

    /// What to do about it.
    #[must_use]
    pub fn next_step(&self) -> String {
        if let Some(device) = &self.device {
            let rename = |part: &str| {
                if part == device {
                    format!("{part}_main")
                } else {
                    part.to_owned()
                }
            };
            let instead = match self.name.split_once('.') {
                Some((owner, rule)) => format!("{}.{}", rename(owner), rename(rule)),
                None => rename(&self.name),
            };
            return format!(
                "Give the {} a name Windows does not keep for a device, such as `{instead}`.",
                self.kind
            );
        }
        match self.shape {
            Shape::Name => format!(
                "Write the {} name in lowercase, such as `page_http`.",
                self.kind
            ),
            Shape::Dotted => format!(
                "Write the {} as an owner and a rule, such as `kernel.no_fixed_wait`.",
                self.kind
            ),
            Shape::Reference => format!(
                "Store the value in the vault and write the {} here, such as `vault://example/service/password`.",
                self.kind
            ),
        }
    }
}

impl fmt::Display for InvalidName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.shape == Shape::Reference {
            // What was written in a reference's place may be the credential
            // itself, so it is never shown back.
            return write!(
                f,
                "this is not a valid {}; it is `vault://` and one to four parts of lowercase \
                 letters, digits, `_`, `-`, and `.`, joined by `/`, each part at most {} characters",
                self.kind, self.longest
            );
        }
        if let Some(device) = &self.device {
            return write!(
                f,
                "`{}` is not a valid {}{}; Windows keeps `{device}` for a device, so a file or \
                 folder of that name cannot be made there, and the name is refused on every system",
                self.name,
                self.kind,
                if self.shape == Shape::Name {
                    " name"
                } else {
                    ""
                }
            );
        }
        if self.shape == Shape::Dotted {
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
    has_name_shape(bytes, longest) && !is_device_bytes(bytes)
}

/// Whether `bytes` is `[a-z][a-z0-9_]*`, at most `longest` characters,
/// whatever Windows keeps.
const fn has_name_shape(bytes: &[u8], longest: usize) -> bool {
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

/// Whether `bytes` is a name Windows keeps for a device: `con`, `prn`,
/// `aux`, `nul`, or `com` or `lpt` and one digit (decision 0431). A path
/// whose last part is one of them names the device on Windows 10, so such a
/// name is refused as a registered name on every system. Only lowercase is
/// tested, because a name with any other letter is refused by its shape.
const fn is_device_bytes(bytes: &[u8]) -> bool {
    match bytes {
        b"con" | b"prn" | b"aux" | b"nul" => true,
        [b'c', b'o', b'm', digit] | [b'l', b'p', b't', digit] => digit.is_ascii_digit(),
        _ => false,
    }
}

/// The part of a name that Windows keeps for a device, when that is the
/// only thing wrong with it: the whole of a name, or one side of a dotted
/// name whose other side and length are right.
fn device_part(text: &str, shape: Shape, longest: usize) -> Option<String> {
    let parts: Vec<&str> = match shape {
        Shape::Name => vec![text],
        Shape::Dotted if text.len() <= longest => match text.split_once('.') {
            Some((owner, rule)) => vec![owner, rule],
            None => return None,
        },
        Shape::Dotted | Shape::Reference => return None,
    };
    let shaped = parts
        .iter()
        .all(|part| has_name_shape(part.as_bytes(), longest));
    shaped
        .then(|| {
            parts
                .into_iter()
                .find(|part| is_device_bytes(part.as_bytes()))
        })
        .flatten()
        .map(str::to_owned)
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
pub fn check_name(kind: &'static str, text: &str, longest: usize) -> Result<(), InvalidName> {
    if is_name(text, longest) {
        Ok(())
    } else {
        Err(InvalidName {
            kind,
            name: shorten(text),
            longest,
            shape: Shape::Name,
            device: device_part(text, Shape::Name, longest),
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
            shape: Shape::Dotted,
            device: device_part(text, Shape::Dotted, longest),
        })
    }
}

/// The longest one part of a credential reference may be.
const REFERENCE_PART: usize = 64;

/// Checks the shape of a reference to a stored credential: `scheme`, then
/// one to four parts joined by `/`, each of lowercase letters, digits, `_`,
/// `-`, and `.`, and none of them `.` or `..`.
pub(crate) fn check_reference(
    kind: &'static str,
    text: &str,
    scheme: &str,
) -> Result<(), InvalidName> {
    let part_ok = |part: &str| {
        !part.is_empty()
            && part.len() <= REFERENCE_PART
            && part != "."
            && part != ".."
            && part.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'-' | b'.')
            })
    };
    let well_formed = text
        .strip_prefix(scheme)
        .is_some_and(|path| path.split('/').count() <= 4 && path.split('/').all(part_ok));
    if well_formed {
        Ok(())
    } else {
        Err(InvalidName {
            kind,
            name: String::new(),
            longest: REFERENCE_PART,
            shape: Shape::Reference,
            device: None,
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
    use super::{check_dotted_name, check_name, is_dotted_name, is_name, shorten};

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

    /// Decision 0431: the names Windows keeps for a device are refused on
    /// every system, alone or as a part of a dotted name.
    #[test]
    fn a_name_windows_keeps_for_a_device_is_refused_with_the_reason() {
        let mut kept = vec![
            "con".to_owned(),
            "prn".to_owned(),
            "aux".to_owned(),
            "nul".to_owned(),
        ];
        for digit in 0..=9 {
            kept.push(format!("com{digit}"));
            kept.push(format!("lpt{digit}"));
        }
        for name in &kept {
            assert!(!is_name(name, 32), "{name}");
            let error = check_name("target", name, 32).unwrap_err();
            let text = error.to_string();
            assert!(text.contains(&format!("`{name}`")), "{text}");
            assert!(text.contains("Windows keeps"), "{text}");
            assert!(text.contains("every system"), "{text}");
            assert_eq!(error.kept_for_a_device(), Some(name.as_str()));
            let fix = error.next_step();
            assert!(fix.contains(&format!("`{name}_")), "{fix}");
            for dotted in [format!("{name}.rule"), format!("owner.{name}")] {
                assert!(!is_dotted_name(&dotted, 64), "{dotted}");
                let text = check_dotted_name("lint code", &dotted, 64)
                    .unwrap_err()
                    .to_string();
                assert!(text.contains(&format!("`{name}`")), "{text}");
                assert!(text.contains("Windows keeps"), "{text}");
            }
            let fix = check_dotted_name("lint code", &format!("{name}x.{name}"), 64)
                .unwrap_err()
                .next_step();
            assert!(fix.contains(&format!("`{name}x.{name}_main`")), "{fix}");
        }
        for name in [
            "console",
            "nulls",
            "com10",
            "lpt10",
            "auxiliary",
            "con_main",
            "prn2",
        ] {
            assert!(is_name(name, 32), "{name}");
        }
        assert!(is_dotted_name("console.rule", 64));
        // A name that is wrong in its shape says so, and not the device rule.
        for wrong in ["Con", "con.rule", "con-1"] {
            let text = check_name("target", wrong, 32).unwrap_err().to_string();
            assert!(!text.contains("Windows"), "{text}");
            assert!(
                check_name("target", wrong, 32)
                    .unwrap_err()
                    .kept_for_a_device()
                    .is_none()
            );
        }
        for wrong in ["con.Rule", "con.rule.x", "con"] {
            let text = check_dotted_name("lint code", wrong, 64)
                .unwrap_err()
                .to_string();
            assert!(!text.contains("Windows"), "{text}");
        }
    }
}
