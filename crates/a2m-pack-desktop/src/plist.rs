//! A reader of property lists, in the XML form and the binary form, for an
//! application's own `Info.plist` (decision 0317). Read from bytes, on
//! every system, with no system call: nothing in a file is run, nothing it
//! names is fetched, and no entity it defines is expanded.
//!
//! Both forms give the same value, which the tests hold against one
//! fixture written in both. The reader is bounded in bytes, depth, objects,
//! and the work of walking them: a binary file whose objects lead back to
//! themselves is refused by name, and so is one that names an object beyond
//! its table. A form it does not know, such as a version of the binary
//! format other than `00`, is a named stop, never a guess (rule 7).

use std::collections::BTreeMap;

use crate::xml::{self, Bounds, Element};

/// Why a file is not read at all.
pub(crate) type NotRead = &'static str;

/// The deepest a list may nest.
const DEEPEST: usize = 32;
/// The most objects one binary list may hold.
const MOST_OBJECTS: usize = 100_000;
/// The most steps the walk of a binary list may make, so that objects that
/// share children cannot be walked as a tree of unbounded size.
const MOST_STEPS: usize = 200_000;
/// The longest string the reader takes, in characters.
const LONGEST_STRING: usize = 65_536;

/// One value of a property list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Plist {
    Dict(BTreeMap<String, Plist>),
    Array(Vec<Plist>),
    String(String),
    Int(i64),
    Real(f64),
    Bool(bool),
    /// Bytes, kept only as their count.
    Data(usize),
    /// A date, as the text the file gives (XML) or the seconds the file
    /// gives (binary), kept apart from the form.
    Date,
    /// An object the reader gives no meaning to (a null, a set, a UID).
    Other,
}

impl Plist {
    pub(crate) fn get(&self, key: &str) -> Option<&Plist> {
        match self {
            Self::Dict(map) => map.get(key),
            _ => None,
        }
    }

    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn flag(&self) -> Option<bool> {
        match self {
            Self::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    pub(crate) fn list(&self) -> &[Plist] {
        match self {
            Self::Array(items) => items,
            _ => &[],
        }
    }
}

/// Reads a property list, in either form.
///
/// # Errors
///
/// A named reason when the bytes are not a property list of a form the
/// reader knows.
pub(crate) fn read(bytes: &[u8]) -> Result<Plist, NotRead> {
    if bytes.starts_with(b"bplist") {
        binary(bytes)
    } else {
        let text = std::str::from_utf8(bytes).map_err(|_| "it is not UTF-8 text")?;
        text_form(text)
    }
}

// ---- the XML form ----

fn text_form(text: &str) -> Result<Plist, NotRead> {
    let root = xml::parse(
        text,
        Bounds {
            depth: DEEPEST + 4,
            elements: MOST_OBJECTS,
        },
        &|said| said.contains("-//Apple//DTD PLIST 1.0//EN"),
    )?;
    if root.local != "plist" {
        return Err("its root is not a `plist`");
    }
    let [only] = root.children.as_slice() else {
        return Err("a `plist` holds exactly one value");
    };
    node(only, 0)
}

fn node(of: &Element, depth: usize) -> Result<Plist, NotRead> {
    if depth > DEEPEST {
        return Err("its values nest deeper than the reader takes");
    }
    let text = |of: &Element| -> Result<String, NotRead> {
        if of.text.chars().count() > LONGEST_STRING {
            return Err("a string is longer than the reader takes");
        }
        Ok(of.text.clone())
    };
    Ok(match of.local.as_str() {
        "dict" => {
            let mut map = BTreeMap::new();
            let mut key: Option<String> = None;
            for child in &of.children {
                match (child.local.as_str(), key.take()) {
                    ("key", None) => key = Some(text(child)?),
                    ("key", Some(_)) => return Err("a key is followed by a key"),
                    (_, Some(name)) => {
                        let value = node(child, depth + 1)?;
                        // A key written twice: the last, as the system's own
                        // reader takes it.
                        map.insert(name, value);
                    }
                    (_, None) => return Err("a value has no key"),
                }
            }
            if key.is_some() {
                return Err("a key has no value");
            }
            Plist::Dict(map)
        }
        "array" => Plist::Array(
            of.children
                .iter()
                .map(|child| node(child, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        "string" => Plist::String(text(of)?),
        "integer" => Plist::Int(
            of.text
                .trim()
                .parse()
                .map_err(|_| "an integer is not one the reader takes")?,
        ),
        "real" => Plist::Real(
            of.text
                .trim()
                .parse()
                .map_err(|_| "a real is not one the reader takes")?,
        ),
        "true" => Plist::Bool(true),
        "false" => Plist::Bool(false),
        "data" => Plist::Data(
            of.text
                .chars()
                .filter(|c| !c.is_whitespace())
                .count()
                .div_ceil(4)
                * 3,
        ),
        "date" => Plist::Date,
        _ => return Err("a value is of a kind the reader does not know"),
    })
}

// ---- the binary form ----

/// A cursor over the bytes of a binary list, every read bounds-checked.
struct Bytes<'a> {
    all: &'a [u8],
}

impl Bytes<'_> {
    fn slice(&self, at: usize, len: usize) -> Result<&[u8], NotRead> {
        let end = at.checked_add(len).ok_or("an offset is beyond the file")?;
        self.all
            .get(at..end)
            .ok_or("an object is beyond the end of the file")
    }

    /// A big-endian whole number of `size` bytes.
    fn number(&self, at: usize, size: usize) -> Result<u64, NotRead> {
        if size == 0 || size > 8 {
            return Err("a number is of a size the reader does not take");
        }
        Ok(self
            .slice(at, size)?
            .iter()
            .fold(0_u64, |sum, byte| (sum << 8) | u64::from(*byte)))
    }
}

struct Table<'a> {
    bytes: Bytes<'a>,
    offsets: Vec<usize>,
    reference_size: usize,
    steps: usize,
}

fn binary(all: &[u8]) -> Result<Plist, NotRead> {
    if !all.starts_with(b"bplist00") {
        return Err("it is a binary property list of a version the reader does not know");
    }
    let bytes = Bytes { all };
    let trailer_at = all
        .len()
        .checked_sub(32)
        .filter(|at| *at >= 8)
        .ok_or("it is too short to be a binary property list")?;
    let offset_size = usize::from(*all.get(trailer_at + 6).ok_or("a trailer is cut short")?);
    let reference_size = usize::from(*all.get(trailer_at + 7).ok_or("a trailer is cut short")?);
    let count = to_usize(bytes.number(trailer_at + 8, 8)?)?;
    let top = to_usize(bytes.number(trailer_at + 16, 8)?)?;
    let table_at = to_usize(bytes.number(trailer_at + 24, 8)?)?;
    if !(1..=8).contains(&offset_size) || !(1..=8).contains(&reference_size) {
        return Err("a size in its trailer is not one the reader takes");
    }
    if count == 0 || count > MOST_OBJECTS {
        return Err("it holds more objects than the reader takes, or none");
    }
    if top >= count {
        return Err("its top object is not in its table");
    }
    let table_len = count
        .checked_mul(offset_size)
        .ok_or("its object table is beyond the file")?;
    if table_at < 8
        || table_at
            .checked_add(table_len)
            .is_none_or(|end| end > trailer_at)
    {
        return Err("its object table is not where its trailer says");
    }
    let mut offsets = Vec::with_capacity(count);
    for index in 0..count {
        let offset = to_usize(bytes.number(table_at + index * offset_size, offset_size)?)?;
        if offset < 8 || offset >= table_at {
            return Err("an object is not in the part of the file that holds objects");
        }
        offsets.push(offset);
    }
    let mut table = Table {
        bytes,
        offsets,
        reference_size,
        steps: 0,
    };
    let mut visiting = vec![false; count];
    table.object(top, 0, &mut visiting)
}

fn to_usize(number: u64) -> Result<usize, NotRead> {
    usize::try_from(number).map_err(|_| "a number is beyond what the reader takes")
}

impl Table<'_> {
    /// The length an object states: in its marker, or, when the marker
    /// says so, in an integer object after it. Gives the length and where
    /// the object's content starts.
    fn length(&self, at: usize, info: u8) -> Result<(usize, usize), NotRead> {
        if info != 0x0F {
            return Ok((usize::from(info), at + 1));
        }
        let marker = *self.bytes.slice(at + 1, 1)?.first().ok_or("cut short")?;
        if marker >> 4 != 0x1 {
            return Err("a length is not stated as an integer");
        }
        let size = 1_usize << (marker & 0x0F);
        if size > 8 {
            return Err("a length is of a size the reader does not take");
        }
        let length = to_usize(self.bytes.number(at + 2, size)?)?;
        Ok((length, at + 2 + size))
    }

    fn object(
        &mut self,
        index: usize,
        depth: usize,
        visiting: &mut Vec<bool>,
    ) -> Result<Plist, NotRead> {
        if depth > DEEPEST {
            return Err("its values nest deeper than the reader takes");
        }
        self.steps += 1;
        if self.steps > MOST_STEPS {
            return Err("walking its values takes more steps than the reader allows");
        }
        let at = *self
            .offsets
            .get(index)
            .ok_or("an object names another that is not in the table")?;
        if visiting.get(index).copied().unwrap_or(true) {
            return Err("an object leads back to itself");
        }
        let marker = *self.bytes.slice(at, 1)?.first().ok_or("cut short")?;
        let (kind, info) = (marker >> 4, marker & 0x0F);
        match kind {
            0x0 => Ok(match info {
                0x08 => Plist::Bool(false),
                0x09 => Plist::Bool(true),
                0x00 | 0x0F => Plist::Other,
                _ => return Err("an object is of a kind the reader does not know"),
            }),
            0x1 => {
                let size = 1_usize << info;
                if size > 8 {
                    return Err("an integer is of a size the reader does not take");
                }
                let value = self.bytes.number(at + 1, size)?;
                // One-, two-, and four-byte integers are unsigned; eight
                // bytes are signed.
                Ok(Plist::Int(if size == 8 {
                    i64::from_be_bytes(value.to_be_bytes())
                } else {
                    i64::try_from(value)
                        .map_err(|_| "an integer is beyond what the reader takes")?
                }))
            }
            0x2 => {
                let size = 1_usize << info;
                let raw = self.bytes.number(at + 1, size)?;
                match size {
                    4 => Ok(Plist::Real(f64::from(f32::from_bits(
                        u32::try_from(raw).map_err(|_| "a real is cut short")?,
                    )))),
                    8 => Ok(Plist::Real(f64::from_bits(raw))),
                    _ => Err("a real is of a size the reader does not take"),
                }
            }
            0x3 => Ok(Plist::Date),
            0x4 => {
                let (length, _) = self.length(at, info)?;
                Ok(Plist::Data(length))
            }
            0x5 => {
                let (length, start) = self.length(at, info)?;
                if length > LONGEST_STRING {
                    return Err("a string is longer than the reader takes");
                }
                let raw = self.bytes.slice(start, length)?;
                if !raw.is_ascii() {
                    return Err("a string marked ASCII holds other bytes");
                }
                Ok(Plist::String(String::from_utf8_lossy(raw).into_owned()))
            }
            0x6 => {
                let (units, start) = self.length(at, info)?;
                if units > LONGEST_STRING {
                    return Err("a string is longer than the reader takes");
                }
                let raw = self.bytes.slice(start, units * 2)?;
                let (pairs, _) = raw.as_chunks::<2>();
                let words: Vec<u16> = pairs.iter().map(|pair| u16::from_be_bytes(*pair)).collect();
                let text = String::from_utf16(&words)
                    .map_err(|_| "a string holds text that is not well formed")?;
                Ok(Plist::String(text))
            }
            0x8 | 0xC => Ok(Plist::Other),
            0xA => {
                let (count, start) = self.length(at, info)?;
                let references = self.references(start, count)?;
                visiting[index] = true;
                let mut items = Vec::new();
                for reference in references {
                    items.push(self.object(reference, depth + 1, visiting)?);
                }
                visiting[index] = false;
                Ok(Plist::Array(items))
            }
            0xD => {
                let (count, start) = self.length(at, info)?;
                let keys = self.references(start, count)?;
                let values = self.references(start + count * self.reference_size, count)?;
                visiting[index] = true;
                let mut map = BTreeMap::new();
                for (key, value) in keys.into_iter().zip(values) {
                    let Plist::String(name) = self.object(key, depth + 1, visiting)? else {
                        return Err("a key is not a string");
                    };
                    let value = self.object(value, depth + 1, visiting)?;
                    map.insert(name, value);
                }
                visiting[index] = false;
                Ok(Plist::Dict(map))
            }
            _ => Err("an object is of a kind the reader does not know"),
        }
    }

    fn references(&self, start: usize, count: usize) -> Result<Vec<usize>, NotRead> {
        if count > MOST_OBJECTS {
            return Err("it holds more objects than the reader takes, or none");
        }
        (0..count)
            .map(|position| {
                self.bytes
                    .number(start + position * self.reference_size, self.reference_size)
                    .and_then(to_usize)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Example</string>
  <key>Count</key><integer>3</integer>
  <key>On</key><true/>
  <key>Items</key><array><string>a</string><string>b &amp; c</string></array>
</dict></plist>"#;

    #[test]
    fn an_xml_list_is_read() {
        let read = read(XML.as_bytes()).unwrap();
        assert_eq!(
            read.get("CFBundleName").and_then(Plist::text),
            Some("Example")
        );
        assert_eq!(read.get("Count"), Some(&Plist::Int(3)));
        assert_eq!(read.get("On").and_then(Plist::flag), Some(true));
        let items: Vec<_> = read
            .get("Items")
            .unwrap()
            .list()
            .iter()
            .filter_map(Plist::text)
            .collect();
        assert_eq!(items, ["a", "b & c"]);
    }

    #[test]
    fn a_hostile_xml_list_is_refused_by_name() {
        let entity = "<!DOCTYPE plist [<!ENTITY a \"b\">]><plist><dict/></plist>";
        assert!(
            read(entity.as_bytes())
                .unwrap_err()
                .contains("document type")
        );
        assert!(read(b"<plist><dict><key>a</key></dict></plist>").is_err());
        assert!(read(b"<plist><dict><string>a</string></dict></plist>").is_err());
        assert!(read(b"<plist><dict/><dict/></plist>").is_err());
        assert!(read(b"<plist><surprise/></plist>").is_err());
        assert!(read(b"<other/>").is_err());
        assert!(read(&[0xff, 0xfe, 0x00]).is_err());
        let deep = format!(
            "<plist>{}{}</plist>",
            "<array>".repeat(60),
            "</array>".repeat(60)
        );
        assert!(read(deep.as_bytes()).is_err());
    }

    /// A binary list of one array that holds itself.
    fn cyclic() -> Vec<u8> {
        let mut file = b"bplist00".to_vec();
        // Object 0 at offset 8: an array of one reference, to object 0.
        file.extend_from_slice(&[0xA1, 0x00]);
        let table_at = file.len();
        file.push(8);
        file.extend_from_slice(&[0; 6]);
        file.extend_from_slice(&[0, 1, 1]);
        file.extend_from_slice(&1_u64.to_be_bytes());
        file.extend_from_slice(&0_u64.to_be_bytes());
        file.extend_from_slice(&(table_at as u64).to_be_bytes());
        file
    }

    #[test]
    fn a_binary_list_that_leads_back_to_itself_is_refused_by_name() {
        assert_eq!(
            read(&cyclic()).unwrap_err(),
            "an object leads back to itself"
        );
    }

    /// The fixture's binary list: a real file of the form.
    const BINARY: &[u8] =
        include_bytes!("../tests/fixtures/bundles/binary/Example.app/Contents/Info.plist");

    #[test]
    fn a_binary_list_changed_at_any_byte_or_cut_anywhere_never_stops_the_reader() {
        // The reader answers every one of them, with a value or a name; it
        // never panics, reads beyond the file, or loops.
        let whole = read(BINARY).unwrap();
        assert!(whole.get("CFBundleURLTypes").is_some());
        for at in 0..BINARY.len() {
            for byte in [0x00_u8, 0x01, 0x0F, 0x7F, 0x80, 0xFF] {
                let mut changed = BINARY.to_vec();
                changed[at] = byte;
                let _ = read(&changed);
            }
            let _ = read(&BINARY[..at]);
        }
    }

    #[test]
    fn a_binary_list_cut_short_or_of_another_version_is_refused_by_name() {
        let whole = cyclic();
        for cut in [0, 7, 8, 20, whole.len() - 1] {
            assert!(read(&whole[..cut]).is_err(), "{cut}");
        }
        let mut other = whole.clone();
        other[6] = b'1';
        other[7] = b'5';
        assert!(read(&other).is_err());
        // A reference to an object that is not in the table.
        let mut beyond = whole;
        beyond[9] = 7;
        assert!(read(&beyond).is_err());
    }
}
