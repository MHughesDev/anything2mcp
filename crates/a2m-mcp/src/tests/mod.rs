//! The endpoint and the proxy, proven with raw messages through the real
//! kernel against the built-in test application.
//!
//! The test pack, its driver, and the hand-built plan of the imaginary
//! `example` target are the engine's own test helpers. The tests are part
//! of the library's own test build so that they can read how many requests
//! an endpoint handed the dispatcher.

#[path = "../../../a2m-engine/tests/common/mod.rs"]
mod engine;

mod checklist;
mod door;
mod wire;
mod world;
