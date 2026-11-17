//! Test stand: a simulated device with readable and writable points and its
//! own update mechanism.
//!
//! The device speaks the Modbus application protocol over TCP: a small, stated
//! subset, written here with no protocol library. It holds bits and words of
//! several layouts, a small process (a pump whose measured speed follows its
//! setpoint, a valve with a position sensor), and a two-slot firmware update
//! reached through its own registers. The README lists the subset, the point
//! map, the update sequence, and every case and fault.
//!
//! A test starts a device with [`TestDevice::start`], talks to it over the
//! port it reports, and asks its [`Control`] what the device received and what
//! it holds. A test never proves a write through the product's own read path.
//!
//! It is test equipment, not part of the product, so it sits outside the crate
//! layering. A product crate may use it in tests only. It names no real vendor
//! or product.

pub mod control;
mod device;
pub mod faults;
pub mod firmware;
pub mod harness;
pub mod map;
pub mod protocol;

pub use control::{Answer, Control, FirmwareState, FrameRecord, Outputs, Snapshot};
pub use device::{
    DeviceConfig, LATCH_IMAGE_FAILED, LATCH_OUT_OF_RANGE, MODEL, SERIAL_NUMBER, TEMPERATURE_SENSOR,
    VENDOR,
};
pub use faults::{Corruption, Fault, FaultKind, Scope};
pub use firmware::{UpdateCommand, UpdateError, UpdateState, Version};
pub use harness::{StartError, TestDevice};
pub use map::{Access, Data, Point, PointId, Table, Value};
pub use protocol::{Exception, FrameKind, Request};
