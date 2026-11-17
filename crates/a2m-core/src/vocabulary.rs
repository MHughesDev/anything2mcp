//! One macro that defines a closed vocabulary: an enum whose every value has
//! one stable name, used alike in files, the ledger, messages, and JSON.

/// Defines an enum with a stable name for each value.
///
/// The enum gets `ALL`, `NAMES`, `as_str`, `Display`, `FromStr`, and serde
/// support that reads and writes the same names. Values are listed in the
/// order the documents list them; where the enum derives `Ord`, that order is
/// the ordering.
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident: $kind:literal {
            $( $(#[$value_meta:meta])* $value:ident => $text:literal ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis enum $name {
            $( $(#[$value_meta])* $value ),+
        }

        impl $name {
            /// Every value, in the order the documents list them.
            pub const ALL: &'static [Self] = &[ $( Self::$value ),+ ];

            /// The name of every value, in the same order as `ALL`.
            pub const NAMES: &'static [&'static str] = &[ $( $text ),+ ];

            /// The stable name used in files, the ledger, and messages.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( Self::$value => $text ),+
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::UnknownName;

            fn from_str(text: &str) -> ::std::result::Result<Self, Self::Err> {
                match text {
                    $( $text => Ok(Self::$value), )+
                    _ => Err($crate::UnknownName::new($kind, text, Self::NAMES)),
                }
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::std::result::Result<Self, D::Error> {
                let text = <::std::string::String as ::serde::Deserialize>::deserialize(deserializer)?;
                text.parse().map_err(::serde::de::Error::custom)
            }
        }
    };
}

pub(crate) use vocabulary;
