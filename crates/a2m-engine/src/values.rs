//! The stored form of values: what the engine hands to `Control::evidence`
//! and what the control plane reads back into a run snapshot.

use a2m_core::{CanonicalJson, Failure, Values};

/// The bytes a set of values is stored as: canonical JSON, the same on
/// every machine.
///
/// # Errors
///
/// `input`, `payload_not_encodable`, which no set of values gives.
pub fn encode_values(values: &Values) -> Result<Vec<u8>, Failure> {
    Ok(CanonicalJson::encode(values)?.as_bytes().to_vec())
}

/// Reads values back from the bytes [`encode_values`] wrote.
///
/// # Errors
///
/// `input`, `payload_unreadable`, when the bytes are not stored values.
pub fn decode_values(bytes: &[u8]) -> Result<Values, Failure> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        Failure::new(
            a2m_core::FailureClass::Input,
            a2m_core::FailureCode::PAYLOAD_UNREADABLE,
            "Stored values are not text.",
            "Read the record again; the stored evidence cannot be used.",
        )
    })?;
    let json: CanonicalJson = serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|_| {
            Failure::new(
                a2m_core::FailureClass::Input,
                a2m_core::FailureCode::PAYLOAD_UNREADABLE,
                "Stored values are not canonical JSON.",
                "Read the record again; the stored evidence cannot be used.",
            )
        })?;
    json.decode()
}
