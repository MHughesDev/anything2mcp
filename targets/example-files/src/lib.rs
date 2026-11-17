//! Test stand: files in a temporary folder, with a control interface that reports the bytes on disk.
//!
//! The file pack is proven against this stand and never against anyone's real
//! files. A test calls [`harness::FileStand::start`] and gets a new folder holding
//! the files of an imaginary order-entry system named `example`, one or more in
//! each format the pack reads. It points what it tests at that folder, and then
//! asks the stand what is on disk: the bytes, their digest, the value at a place
//! as the stand's own reader sees it, and how each file changed between two looks.
//! Every fault the pack's tests need is a setting.
//!
//! It is test equipment, not part of the product, so it sits outside the crate
//! layering and depends on no product crate. A product crate may use it in tests
//! only. The `README.md` beside this crate lists the files, the cases, and the
//! faults.

pub mod control;
pub mod faults;
pub mod harness;
pub mod package;
pub mod reader;
pub mod seed;
