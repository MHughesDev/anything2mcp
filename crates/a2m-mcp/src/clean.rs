//! The redactor on the way out of the endpoint.
//!
//! A result is redacted by the dispatcher before this crate sees it. What
//! the dispatcher never sees is what the protocol layer writes itself: an
//! error that quotes a tool's name or a revision, and the request id and
//! progress token it echoes. So every text of every message passes the
//! redactor as the message leaves.

use a2m_core::Redact;
use serde_json::Value as Json;

/// Redacts every text in `value`, names of fields included. Returns
/// whether anything changed.
pub(crate) fn walk(redact: &dyn Redact, value: &mut Json) -> bool {
    match value {
        Json::String(text) => {
            let clean = redact.redact(text);
            let changed = clean != *text;
            *text = clean;
            changed
        }
        Json::Array(items) => items
            .iter_mut()
            .fold(false, |changed, item| walk(redact, item) | changed),
        Json::Object(map) => {
            let mut changed = false;
            let mut renamed = Vec::new();
            for (name, inner) in map.iter_mut() {
                changed |= walk(redact, inner);
                let clean = redact.redact(name);
                if clean != *name {
                    renamed.push((name.clone(), clean));
                }
            }
            for (name, clean) in renamed {
                if let Some(inner) = map.remove(&name) {
                    map.insert(clean, inner);
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}
