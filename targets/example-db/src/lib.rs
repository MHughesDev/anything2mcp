//! Test stand: a local relational store with keys and constraints, and a control
//! interface that reports what it stored.
//!
//! The store is one database file of the embedded SQL engine, holding the schema
//! of an imaginary order-entry system named `example`. A test starts one with
//! [`harness::TestDb::start`], hands its path to the code under test, which opens
//! its own connection, and asks the control interface for ground truth: the row as
//! stored, and how many times it was written. Every fault is a setting.
//!
//! Spec section 307 lists the cases; `README.md` lists what is built.
//!
//! The server side, in [`server`], is a real PostgreSQL server with the same
//! system's schema, a relay in front of it that can lose an answer, and control
//! queries run through the server's own client.
//!
//! It is test equipment, not part of the product, so it sits outside the crate
//! layering. A product crate may use it in tests only.

pub mod faults;
pub mod harness;
pub mod rows;
pub mod schema;
pub mod server;
