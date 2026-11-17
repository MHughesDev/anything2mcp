//! A value that must never be shown.

use std::fmt;

use zeroize::Zeroize;

/// What stands in for a secret wherever one would otherwise be printed.
const REDACTED: &str = "[redacted]";

/// A cookie, a token, a password, or any other value a caller must never see.
///
/// A `Secret` cannot be printed, logged, or serialized: its `Debug` and
/// `Display` show `[redacted]`, and it implements neither `Serialize` nor
/// `Clone`. The only way to the value is [`Secret::expose`], which is named so
/// that every use can be found and reviewed.
///
/// This type stops accidents. It does not replace the redactor, which every
/// secret is also registered with so that a value that reached a system and
/// came back in a response is caught as well.
///
/// The value's bytes are overwritten with zeros when the secret is dropped
/// (spec section 216, decision 0122). That covers the bytes this wrapper
/// holds. A copy made from [`Secret::expose`] is the caller's to wipe.
pub struct Secret(String);

impl Secret {
    /// Wraps a value. From here on it cannot be printed.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself, for handing to the system it belongs to.
    ///
    /// Never pass the result to a formatter, a logger, a trace, an error
    /// message, or a tool result.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Secret {
    /// Overwrites the value's bytes with zeros and leaves the secret empty.
    fn wipe(&mut self) {
        self.0.zeroize();
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::Secret;

    const VALUE: &str = "correct horse battery staple";

    /// A structure of the kind that ends up in a log line.
    #[derive(Debug)]
    #[allow(
        dead_code,
        reason = "read only through Debug, which is the point of the test"
    )]
    struct Session {
        target: &'static str,
        cookie: Secret,
        tokens: Vec<Secret>,
        refresh: Option<Secret>,
    }

    #[test]
    fn a_secret_is_never_shown_by_any_formatter() {
        let secret = Secret::new(VALUE);
        let shown = [
            format!("{secret}"),
            format!("{secret:?}"),
            format!("{secret:#?}"),
            format!("{secret:>40}"),
            secret.to_string(),
        ];
        for text in shown {
            assert!(!text.contains(VALUE), "{text}");
            assert!(!text.contains("horse"), "{text}");
            assert!(text.contains("[redacted]"), "{text}");
        }
    }

    #[test]
    fn a_secret_inside_another_structure_is_never_shown() {
        let session = Session {
            target: "example",
            cookie: Secret::new(VALUE),
            tokens: vec![Secret::new(VALUE), Secret::new(VALUE)],
            refresh: Some(Secret::new(VALUE)),
        };
        for text in [format!("{session:?}"), format!("{session:#?}")] {
            assert!(!text.contains("horse"), "{text}");
            assert_eq!(text.matches("[redacted]").count(), 4, "{text}");
            assert!(text.contains("example"), "{text}");
        }
    }

    #[test]
    fn the_value_is_reached_only_by_asking_for_it() {
        assert_eq!(Secret::new(VALUE).expose(), VALUE);
    }

    /// `Drop` calls `wipe` and nothing else. What `wipe` leaves is checked
    /// here; memory that has been given back cannot be read by a test.
    #[test]
    fn a_dropped_secret_is_wiped() {
        let mut secret = Secret::new(VALUE);
        let held = secret.expose().len();
        assert_eq!(held, VALUE.len());
        secret.wipe();
        assert!(secret.expose().is_empty());
        // The buffer is still the secret's own, and every byte of it is zero.
        assert!(secret.0.capacity() >= held);
        secret.0.extend(std::iter::repeat_n('\0', held));
        assert!(secret.expose().bytes().all(|byte| byte == 0));
    }

    /// These fail to compile, and so fail the build, if `Secret` ever gains a
    /// way to be serialized or copied. Each trait in question is given a
    /// second route to `Probe`; a type that has the trait makes the call
    /// ambiguous.
    #[test]
    fn a_secret_cannot_be_serialized_or_cloned() {
        trait Probe<Marker> {
            fn check() {}
        }
        impl<T: ?Sized> Probe<()> for T {}

        struct Serializes;
        impl<T: ?Sized + serde::Serialize> Probe<Serializes> for T {}
        struct Clones;
        impl<T: Clone> Probe<Clones> for T {}

        <Secret as Probe<_>>::check();
    }
}
