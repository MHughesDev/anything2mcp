//! The one deterministic encoding: canonical JSON.
//!
//! The same data gives the same bytes on every machine and every run: no
//! spaces, the keys of every object sorted by their bytes, whole numbers
//! only, and one way to escape text. A compiled target, a pack's payload, and
//! the bytes a value's digest is made from all use it (decision 0069).

use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::digest::Digest;
use crate::failure::{Failure, FailureClass, FailureCode};

/// Why data could not be written in the canonical encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotEncodable {
    /// It holds a float. No number in a compiled target is a float.
    Float,
    /// It holds something JSON cannot carry: a map whose keys are not text,
    /// or a number beyond 64 bits.
    NotData,
}

impl fmt::Display for NotEncodable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float => f.write_str(
                "the data holds a float; the compiled target's encoding carries whole numbers, and a decimal travels as text",
            ),
            Self::NotData => f.write_str(
                "the data holds something the compiled target's encoding cannot carry: a map whose keys are not text, or a number beyond 64 bits",
            ),
        }
    }
}

/// Writes `text` as a JSON string, the one way: the short escapes for the
/// quote, the backslash, and the five named control characters; `\u00xx`
/// for the other control characters; everything else as it is.
pub(crate) fn write_string(out: &mut Vec<u8>, text: &str) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(b'"');
    for byte in text.bytes() {
        match byte {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x00..=0x1f => {
                out.extend_from_slice(b"\\u00");
                out.push(HEX[usize::from(byte >> 4)]);
                out.push(HEX[usize::from(byte & 0x0f)]);
            }
            _ => out.push(byte),
        }
    }
    out.push(b'"');
}

/// The entries of an object, sorted by the bytes of their keys. Sorting is
/// done here and not left to the map, whose order depends on how the outside
/// crate was built.
fn sorted(map: &serde_json::Map<String, serde_json::Value>) -> Vec<(&String, &serde_json::Value)> {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    entries
}

/// Writes a whole number, or refuses a float.
fn write_number(out: &mut Vec<u8>, number: &serde_json::Number) -> Result<(), NotEncodable> {
    if let Some(whole) = number.as_i64() {
        out.extend_from_slice(whole.to_string().as_bytes());
    } else if let Some(whole) = number.as_u64() {
        out.extend_from_slice(whole.to_string().as_bytes());
    } else {
        return Err(NotEncodable::Float);
    }
    Ok(())
}

/// Writes `value` with no spaces and every object's keys sorted.
fn write_compact(out: &mut Vec<u8>, value: &serde_json::Value) -> Result<(), NotEncodable> {
    match value {
        serde_json::Value::Null => out.extend_from_slice(b"null"),
        serde_json::Value::Bool(true) => out.extend_from_slice(b"true"),
        serde_json::Value::Bool(false) => out.extend_from_slice(b"false"),
        serde_json::Value::Number(number) => write_number(out, number)?,
        serde_json::Value::String(text) => write_string(out, text),
        serde_json::Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_compact(out, item)?;
            }
            out.push(b']');
        }
        serde_json::Value::Object(map) => {
            out.push(b'{');
            for (index, (key, item)) in sorted(map).into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                write_string(out, key);
                out.push(b':');
                write_compact(out, item)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

/// Writes `value` for a person to read: the same content and the same order
/// as the compact form, one entry per line, indented by two spaces.
fn write_indented(
    out: &mut Vec<u8>,
    value: &serde_json::Value,
    depth: usize,
) -> Result<(), NotEncodable> {
    let indent = |out: &mut Vec<u8>, depth: usize| {
        out.push(b'\n');
        out.resize(out.len() + depth * 2, b' ');
    };
    match value {
        serde_json::Value::Array(items) if !items.is_empty() => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                indent(out, depth + 1);
                write_indented(out, item, depth + 1)?;
            }
            indent(out, depth);
            out.push(b']');
        }
        serde_json::Value::Object(map) if !map.is_empty() => {
            out.push(b'{');
            for (index, (key, item)) in sorted(map).into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                indent(out, depth + 1);
                write_string(out, key);
                out.extend_from_slice(b": ");
                write_indented(out, item, depth + 1)?;
            }
            indent(out, depth);
            out.push(b'}');
        }
        other => write_compact(out, other)?,
    }
    Ok(())
}

/// The canonical bytes of anything that can be serialized.
pub(crate) fn to_bytes<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, NotEncodable> {
    let tree = serde_json::to_value(value).map_err(|_| NotEncodable::NotData)?;
    let mut out = Vec::new();
    write_compact(&mut out, &tree)?;
    Ok(out)
}

/// The same content as [`to_bytes`], laid out for a person to read.
pub(crate) fn to_readable<T: Serialize + ?Sized>(value: &T) -> Result<String, NotEncodable> {
    let tree = serde_json::to_value(value).map_err(|_| NotEncodable::NotData)?;
    let mut out = Vec::new();
    write_indented(&mut out, &tree, 0)?;
    String::from_utf8(out).map_err(|_| NotEncodable::NotData)
}

/// Data in the canonical encoding, held as it was written.
///
/// It is stored and hashed by the kernel and decoded only by whoever wrote
/// it. The same data always encodes to the same bytes, so two payloads are
/// equal exactly when their data is.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct CanonicalJson(String);

/// A pack's own compiled knowledge: a screen model, a request template, the
/// details of one step. The pack encodes it when it compiles or lowers; the
/// pack's driver decodes it; the kernel never does.
pub type PackPayload = CanonicalJson;

impl CanonicalJson {
    /// Encodes `value`.
    ///
    /// # Errors
    ///
    /// A failure of class `input`, code `payload_not_encodable`, when the
    /// value holds a float, a map whose keys are not text, or a number
    /// beyond 64 bits.
    pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Self, Failure> {
        let bytes = to_bytes(value).map_err(not_encodable)?;
        String::from_utf8(bytes)
            .map(Self)
            .map_err(|_| not_encodable(NotEncodable::NotData))
    }

    /// Decodes the payload as the type its reader expects.
    ///
    /// # Errors
    ///
    /// A failure of class `input`, code `payload_unreadable`, when the
    /// payload is not that type. The message gives the place and never the
    /// content.
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, Failure> {
        serde_json::from_str(&self.0).map_err(|error| {
            Failure::new(
                FailureClass::Input,
                FailureCode::PAYLOAD_UNREADABLE,
                format!(
                    "A payload in the compiled target does not decode as the type its reader expects, at character {} of {}.",
                    error.column(),
                    self.0.len()
                ),
                "Compile the target again with the versions of the packs that are enabled.",
            )
        })
    }

    /// A payload that holds nothing: the encoding of no value.
    #[must_use]
    pub fn none() -> Self {
        Self("null".to_owned())
    }

    /// Whether the payload holds nothing.
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.0 == "null"
    }

    /// The encoded bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// The digest of the encoded bytes.
    #[must_use]
    pub fn digest(&self) -> Digest {
        Digest::of(self.0.as_bytes())
    }
}

impl Default for CanonicalJson {
    fn default() -> Self {
        Self::none()
    }
}

impl fmt::Debug for CanonicalJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CanonicalJson({} bytes)", self.0.len())
    }
}

impl Serialize for CanonicalJson {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for CanonicalJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <String as serde::Deserialize>::deserialize(deserializer).map(Self)
    }
}

/// The failure for data that cannot be encoded.
pub(crate) fn not_encodable(error: NotEncodable) -> Failure {
    Failure::new(
        FailureClass::Input,
        FailureCode::PAYLOAD_NOT_ENCODABLE,
        error.to_string(),
        "Hold numbers as whole numbers or decimals, and key every map by text.",
    )
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::{CanonicalJson, PackPayload, to_bytes, to_readable, write_string};
    use crate::{FailureClass, FailureCode};

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Template {
        path: String,
        method: String,
        retries: u32,
        headers: BTreeMap<String, String>,
        offset: i64,
        note: Option<String>,
    }

    fn template() -> Template {
        Template {
            path: "/orders/{order_id}".to_owned(),
            method: "GET".to_owned(),
            retries: 2,
            headers: BTreeMap::from([("accept".to_owned(), "application/json".to_owned())]),
            offset: -5,
            note: None,
        }
    }

    #[test]
    fn the_same_data_gives_the_same_bytes_with_sorted_keys_and_no_spaces() {
        let bytes = to_bytes(&template()).unwrap();
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            r#"{"headers":{"accept":"application/json"},"method":"GET","note":null,"offset":-5,"path":"/orders/{order_id}","retries":2}"#
        );
        assert_eq!(to_bytes(&template()).unwrap(), bytes);
    }

    #[test]
    fn a_map_with_no_order_of_its_own_is_written_in_one_order() {
        // A hash map iterates in an order that differs between runs. The
        // encoding must not let that through.
        let names = [
            "zulu", "alpha", "mike", "bravo", "yankee", "charlie", "x", "delta",
        ];
        let expected =
            to_bytes(&names.iter().map(|n| (*n, 1)).collect::<BTreeMap<_, _>>()).unwrap();
        for _ in 0..20 {
            let map: HashMap<&str, u32> = names.iter().map(|n| (*n, 1)).collect();
            assert_eq!(to_bytes(&map).unwrap(), expected);
        }
    }

    #[test]
    fn a_float_is_refused() {
        #[derive(serde::Serialize)]
        struct Price {
            amount: f64,
        }
        let failure = PackPayload::encode(&Price { amount: 12.5 }).unwrap_err();
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &FailureCode::PAYLOAD_NOT_ENCODABLE);
        assert!(PackPayload::encode(&vec![1.0f32]).is_err());
        let keyed_by_number: BTreeMap<(u8, u8), u8> = BTreeMap::from([((1, 2), 3)]);
        assert!(PackPayload::encode(&keyed_by_number).is_err());
    }

    #[test]
    fn text_is_escaped_one_way_and_reads_back() {
        let text = "quote \" slash \\ tab \t line \n bell \u{7} nul \u{0} é 日本 \u{1f600}";
        let mut out = Vec::new();
        write_string(&mut out, text);
        let written = String::from_utf8(out).unwrap();
        assert!(
            written.contains("\\u0007") && written.contains("\\u0000"),
            "{written}"
        );
        assert!(written.contains("é 日本"), "{written}");
        assert_eq!(serde_json::from_str::<String>(&written).unwrap(), text);
        // The outside crate escapes the same way, so its output is canonical too.
        assert_eq!(serde_json::to_string(text).unwrap(), written);
    }

    #[test]
    fn a_payload_reads_back_as_what_was_encoded_and_only_as_that() {
        let payload = PackPayload::encode(&template()).unwrap();
        assert_eq!(payload.decode::<Template>().unwrap(), template());
        assert_eq!(payload, PackPayload::encode(&template()).unwrap());
        assert_eq!(
            payload.digest(),
            PackPayload::encode(&template()).unwrap().digest()
        );

        let failure = payload.decode::<Vec<String>>().unwrap_err();
        assert_eq!(failure.class(), FailureClass::Input);
        assert_eq!(failure.code(), &FailureCode::PAYLOAD_UNREADABLE);
        assert!(!failure.message().contains("orders"), "{failure}");

        assert!(CanonicalJson::none().is_none());
        assert_eq!(
            CanonicalJson::default().decode::<Option<u8>>().unwrap(),
            None
        );
        assert!(!format!("{payload:?}").contains("orders"));
    }

    #[test]
    fn a_payload_travels_inside_other_data_as_text() {
        let payload = PackPayload::encode(&template()).unwrap();
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.starts_with('"'));
        assert_eq!(serde_json::from_str::<PackPayload>(&json).unwrap(), payload);
    }

    #[test]
    fn the_readable_form_has_the_same_content_in_the_same_order() {
        let readable = to_readable(&template()).unwrap();
        assert!(
            readable.starts_with(
                "{\n  \"headers\": {\n    \"accept\": \"application/json\"\n  },\n  \"method\""
            ),
            "{readable}"
        );
        assert!(!readable.contains('\r'));
        let back: serde_json::Value = serde_json::from_str(&readable).unwrap();
        assert_eq!(to_bytes(&back).unwrap(), to_bytes(&template()).unwrap());
    }
}
