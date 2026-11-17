//! The stored form of values: what the engine hands to `Control::evidence`
//! and what the control plane reads back into a run snapshot.

use a2m_core::{CanonicalJson, Failure, FailureClass, FailureCode, Values};

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
    serde_json::from_slice(bytes).map_err(|_| {
        Failure::new(
            FailureClass::Input,
            FailureCode::PAYLOAD_UNREADABLE,
            "Stored values cannot be read back as values.",
            "Read the record again; the stored evidence cannot be used.",
        )
    })
}
