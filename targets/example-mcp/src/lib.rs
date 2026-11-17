//! Test stand: a small MCP server whose tools can change and whose connection can drop after a commit.
//!
//! The MCP pack reaches an MCP server that a system already provides. This
//! crate is such a server, over an imaginary order-entry system named
//! `example`, with a control interface that reports what it received and what
//! it stored. It imitates what makes an upstream server hard: a tool list
//! that changes, a connection lost after a write was committed, answers that
//! are slow, broken, or written to steer an agent, and a credential it may
//! refuse. Every one of those is a setting.
//!
//! It speaks the protocol by hand, with JSON-RPC 2.0 messages and no MCP
//! library, so that it shares no mistake with whatever the product uses.
//!
//! It has two forms. The HTTP form runs inside a test:
//! [`harness::HttpStand::start`]. The stdio form is the program `example-mcp`,
//! which whatever is under test starts: [`harness::StdioStand::new`]. Both
//! hand out the same [`control::Control`].
//!
//! It is test equipment, not part of the product, so it sits outside the
//! crate layering. A product crate may use it in tests only.

pub mod control;
pub mod faults;
pub mod harness;
pub mod http;
pub mod protocol;
pub mod state;
pub mod stdio;
pub mod store;
pub mod tools;
