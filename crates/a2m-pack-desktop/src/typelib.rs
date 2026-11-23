//! A reader of COM type libraries, from the bytes of a file (decision 0318).
//!
//! A type library is read as bytes by a library taken whole,
//! `msft-typelib`, and never loaded by the system: no code of the system
//! or of the library described runs on a file that may be hostile, and the
//! reader runs on every system. The library is bounded here (interfaces,
//! members, parameters, text), and a test changes a type library at every
//! byte and cuts it at every length and holds that the reader answers each
//! with a value or a name (the fuzzing decision 0279 asks for before the
//! library is used).
//!
//! Only the MSFT form is read. The older form (`SLTG`), a type library
//! inside a program file (a resource), and anything else are refused by
//! name, never guessed at (rule 7).
//!
//! What it gives for each member is how the library declares it to be
//! invoked: an ordinary function, a property get, a property put, or a
//! property put by reference. A property get is the library's statement
//! that a member only reads.

use msft_typelib::{TypeDescription, TypeKind, TypeLib};

/// The most interfaces one library may give.
const MOST_INTERFACES: usize = 1_000;
/// The most members of one interface.
const MOST_MEMBERS: usize = 2_000;
/// The most parameters of one member.
const MOST_PARAMETERS: usize = 64;
/// The longest of a library's own words kept, in characters.
const LONGEST_WORDS: usize = 200;

/// Why a file is not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refused {
    /// A form no reader here knows: the reason.
    UnknownFormat(&'static str),
    /// The MSFT form, and it does not hold together: what the reader says.
    Malformed(String),
}

/// How a member is invoked, as the library declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Invoke {
    Function,
    PropertyGet,
    PropertyPut,
    PropertyPutRef,
    /// A value of the field the reader does not know.
    Other,
}

impl Invoke {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::PropertyGet => "property_get",
            Self::PropertyPut => "property_put",
            Self::PropertyPutRef => "property_put_ref",
            Self::Other => "other",
        }
    }
}

/// A type, as the library writes it: its variant type number, and a word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Type {
    /// The `VARTYPE` of a base type; 0 for the others.
    pub vt: u16,
    /// A word for it: the base type's name, or `pointer`, `safe_array`,
    /// `array`, `user_defined`.
    pub word: String,
}

/// One parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parameter {
    pub name: String,
    pub ty: Type,
    pub input: bool,
    pub output: bool,
    pub optional: bool,
    pub retval: bool,
}

/// One member of an interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub name: String,
    pub memid: i32,
    pub invoke: Invoke,
    /// What it gives: the function's return type, or, for a member that
    /// returns a status and gives its value through a last parameter marked
    /// `retval`, the type that parameter points to.
    pub returns: Type,
    pub parameters: Vec<Parameter>,
}

/// One interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Interface {
    pub name: String,
    pub guid: Option<String>,
    /// `dispatch` or `interface`.
    pub kind: &'static str,
    pub members: Vec<Member>,
}

/// A type library, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Library {
    pub name: String,
    pub guid: Option<String>,
    pub version: (u16, u16),
    pub interfaces: Vec<Interface>,
    /// The names of its coclasses.
    pub coclasses: Vec<String>,
    /// How many types of other kinds were passed over.
    pub passed_over: u64,
    /// Whether a bound cut the reading short.
    pub cut: bool,
}

fn words(text: Option<&str>) -> String {
    text.unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(LONGEST_WORDS)
        .collect()
}

/// The `VT_HRESULT` variant type: a status.
const VT_HRESULT: u16 = 25;
/// The `VT_PTR` variant type.
const VT_PTR: u16 = 26;

fn type_of(lib: &TypeLib<'_>, datatype: i32) -> Type {
    match lib.type_description(datatype) {
        Some(TypeDescription::Base(vt)) => Type {
            vt,
            word: msft_typelib::vt_name(vt).to_owned(),
        },
        Some(TypeDescription::Pointer(_)) => Type {
            vt: VT_PTR,
            word: "pointer".to_owned(),
        },
        Some(TypeDescription::SafeArray(_)) => Type {
            vt: 0,
            word: "safe_array".to_owned(),
        },
        Some(TypeDescription::CArray(_)) => Type {
            vt: 0,
            word: "array".to_owned(),
        },
        Some(TypeDescription::UserDefined(_)) => Type {
            vt: 0,
            word: "user_defined".to_owned(),
        },
        None => Type {
            vt: 0,
            word: "unknown".to_owned(),
        },
    }
}

/// The type a pointer type points to, one level.
fn pointee(lib: &TypeLib<'_>, datatype: i32) -> Option<Type> {
    match lib.type_description(datatype)? {
        TypeDescription::Pointer(inner) => Some(type_of(lib, inner)),
        _ => None,
    }
}

fn member(lib: &TypeLib<'_>, ti: &msft_typelib::TypeInfoEntry<'_>, index: usize) -> Option<Member> {
    let func = lib.funcs(ti).nth(index)?;
    let invoke = match func.invoke_kind() {
        1 => Invoke::Function,
        2 => Invoke::PropertyGet,
        4 => Invoke::PropertyPut,
        8 => Invoke::PropertyPutRef,
        _ => Invoke::Other,
    };
    let parameters: Vec<Parameter> = lib
        .params(&func)
        .take(MOST_PARAMETERS)
        .map(|parameter| Parameter {
            name: words(lib.name(parameter.name_offset())),
            ty: type_of(lib, parameter.datatype()),
            input: parameter.is_in(),
            output: parameter.is_out(),
            optional: parameter.is_optional(),
            retval: parameter.is_retval(),
        })
        .collect();
    let mut returns = type_of(lib, func.datatype());
    if returns.vt == VT_HRESULT {
        // A status, and the value through the last parameter marked
        // `retval`: the type it points to.
        let last = lib
            .params(&func)
            .filter(|parameter| parameter.is_retval())
            .last();
        if let Some(inner) = last.and_then(|parameter| pointee(lib, parameter.datatype())) {
            returns = inner;
        }
    }
    Some(Member {
        name: words(lib.func_name(ti, index)),
        memid: lib.func_memid(ti, index).unwrap_or(0),
        invoke,
        returns,
        parameters,
    })
}

/// Reads a type library.
///
/// # Errors
///
/// [`Refused`], by name, when the bytes are not an MSFT type library.
pub(crate) fn read(bytes: &[u8]) -> Result<Library, Refused> {
    if bytes.starts_with(b"SLTG") {
        return Err(Refused::UnknownFormat(
            "it is the older SLTG form of a type library, for which no reader is known",
        ));
    }
    if bytes.starts_with(b"MZ") {
        return Err(Refused::UnknownFormat(
            "it is a program file, which may hold a type library as a resource; reading one from inside a program file is not built",
        ));
    }
    if !bytes.starts_with(b"MSFT") {
        return Err(Refused::UnknownFormat(
            "it is not in the MSFT form, the only form of type library the reader knows",
        ));
    }
    let lib = TypeLib::parse(bytes).map_err(|error| Refused::Malformed(error.to_string()))?;
    let mut out = Library {
        name: words(lib.lib_name()),
        guid: lib.lib_guid().map(|guid| guid.to_string()),
        version: (lib.version_major(), lib.version_minor()),
        interfaces: Vec::new(),
        coclasses: Vec::new(),
        passed_over: 0,
        cut: false,
    };
    for (position, entry) in lib.typeinfos().enumerate() {
        if position >= MOST_INTERFACES {
            out.cut = true;
            break;
        }
        let ti = entry.map_err(|error| Refused::Malformed(error.to_string()))?;
        let kind = match ti.typekind() {
            TypeKind::Dispatch => "dispatch",
            TypeKind::Interface => "interface",
            TypeKind::CoClass => {
                out.coclasses.push(words(lib.name(ti.name_offset())));
                continue;
            }
            _ => {
                out.passed_over += 1;
                continue;
            }
        };
        let count = usize::from(ti.func_count());
        if count > MOST_MEMBERS {
            out.cut = true;
        }
        let members: Vec<Member> = (0..count.min(MOST_MEMBERS))
            .filter_map(|index| member(&lib, &ti, index))
            .collect();
        out.interfaces.push(Interface {
            name: words(lib.name(ti.name_offset())),
            guid: lib.guid(ti.guid_offset()).map(|guid| guid.to_string()),
            kind,
            members,
        });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "../tests/common/typelib.rs"]
mod builder;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_library_is_read_and_its_members_are_told_apart() {
        let library = read(&super::builder::example_typelib()).unwrap();
        assert_eq!(library.name, "ExampleDeskLib");
        assert_eq!(library.version, (1, 0));
        let [orders] = library.interfaces.as_slice() else {
            panic!("{library:?}");
        };
        assert_eq!(orders.kind, "dispatch");
        let members: Vec<(&str, Invoke, u16)> = orders
            .members
            .iter()
            .map(|member| (member.name.as_str(), member.invoke, member.returns.vt))
            .collect();
        assert_eq!(
            members,
            [
                ("Total", Invoke::PropertyGet, 3),
                ("Label", Invoke::PropertyGet, 8),
                ("Label", Invoke::PropertyPut, 24),
                ("Remove", Invoke::Function, 11),
            ]
        );
        assert_eq!(orders.members[3].parameters.len(), 1);
        assert_eq!(orders.members[3].parameters[0].name, "order");
        assert!(orders.members[3].parameters[0].input);
    }

    /// The fuzzing decision 0279 asks for before a library is taken: the
    /// reader answers every file that is the built library changed at one
    /// byte, or cut at any length, with a value or a name. It never
    /// panics, reads beyond the file, or runs long.
    #[test]
    fn a_library_changed_at_any_byte_or_cut_anywhere_never_stops_the_reader() {
        let whole = super::builder::example_typelib();
        for at in 0..whole.len() {
            for byte in [0x00_u8, 0x01, 0x04, 0x7F, 0x80, 0xFF] {
                let mut changed = whole.clone();
                changed[at] = byte;
                let _ = read(&changed);
            }
            let _ = read(&whole[..at]);
        }
        // Four bytes at a time, to the largest values a field holds.
        for at in (0..whole.len().saturating_sub(4)).step_by(2) {
            for word in [0xFFFF_FFFF_u32, 0x7FFF_FFFF, 0x8000_0000, 0x0001_0000] {
                let mut changed = whole.clone();
                changed[at..at + 4].copy_from_slice(&word.to_le_bytes());
                let _ = read(&changed);
            }
        }
    }

    #[test]
    fn a_form_no_reader_here_knows_is_refused_by_name() {
        for (bytes, what) in [
            (&b"SLTG\0\0\0\0"[..], "SLTG"),
            (&b"MZ\x90\0\x03"[..], "program file"),
            (&b"\x7fELF"[..], "MSFT form"),
            (&b""[..], "MSFT form"),
        ] {
            match read(bytes) {
                Err(Refused::UnknownFormat(why)) => assert!(why.contains(what), "{why}"),
                other => panic!("{other:?}"),
            }
        }
        // The MSFT magic and nothing after it is malformed, not unknown.
        assert!(matches!(read(b"MSFT"), Err(Refused::Malformed(_))));
    }
}
