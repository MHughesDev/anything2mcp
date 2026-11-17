//! The data pack: the `sql` mode over relational stores.
//!
//! It brings one surface kind, `database`; one file kind, `statements/`,
//! which declares every statement a binding may run; the lowering of the
//! verbs `read`, `list`, `search`, `create`, `update`, and `delete` into the
//! steps `begin`, `execute`, `fetch`, `expect_rows`, and `commit`; the
//! driver of those steps; the discoverer that reads a store's own catalog;
//! and the drafting rule from a table to a statements file.
//!
//! A statement's text is a constant of the plan. A value from a tool input
//! reaches the store only as a bound parameter. A read runs only on a
//! connection the store itself holds read-only, so a statement that changes
//! data is refused by the store. What a read-only connection does not stop,
//! such as attaching another file, is refused three times: by the compiler
//! and again by the driver from the statement's words, and by the
//! connection's own guard whatever the words say.
//!
//! The first store is the embedded SQL engine, behind the pack's own small
//! interface in `store`. Server stores, hand-written writes, procedures,
//! `upsert`, `stamped` and `find` identity, the `before_values` guard, and
//! writes beneath an application are not built.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 5.

mod discover;
mod draft;
mod driver;
mod kind;
mod lint;
mod lower;
mod model;
mod names;
#[cfg(feature = "postgres")]
mod postgres;
mod shape;
mod sqlite;
mod store;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use a2m_core::{Failure, Value};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar};

pub use names::codes;

use crate::driver::Openers;

/// The key of a `database` surface that holds its address: the path of the
/// store's file below its root, with forward slashes.
const ADDRESS_KEY: &str = "file";

/// The key of a `database_server` surface that holds its address: the IP
/// address the server listens on.
#[cfg(feature = "postgres")]
const SERVER_ADDRESS_KEY: &str = "host";

/// The manifest of a build with the server store: the embedded store's, with
/// the second surface kind, the network the surfaces name, and the codes only
/// the server store returns. Made from the one file, so that the two cannot
/// drift apart; a test holds each change to exactly the place it is made.
#[cfg(feature = "postgres")]
pub(crate) static SERVER_MANIFEST: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let base = include_str!("../anything2mcp-provider.toml");
    let mut manifest = base
        .replace(
            "surface_kinds = [\"database\"]",
            "surface_kinds = [\"database\", \"database_server\"]",
        )
        .replace("network = \"none\"", "network = \"surfaces\"")
        .replace(
            "sessions = [\"database\"]",
            "sessions = [\"database\", \"database_server\"]",
        );
    manifest.push_str(include_str!("../anything2mcp-provider-server.toml"));
    manifest
});

/// The data pack. The product crate hands it to the registry builder:
/// `builder.add_pack(&a2m_pack_data::pack())`.
pub struct DataPack {
    openers: Openers,
}

impl std::fmt::Debug for DataPack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataPack").finish_non_exhaustive()
    }
}

/// The data pack, speaking to the stores this build knows.
#[must_use]
pub fn pack() -> DataPack {
    DataPack {
        openers: Arc::new(store::opener),
    }
}

impl DataPack {
    /// The pack over other stores, for a test that puts a fault between the
    /// driver and a store.
    #[cfg(test)]
    pub(crate) fn with_openers(openers: Openers) -> Self {
        Self { openers }
    }
}

fn setting(cx: &PackContext, key: &str, default: i64) -> i64 {
    cx.settings()
        .get(key)
        .and_then(Value::as_int)
        .unwrap_or(default)
}

impl Pack for DataPack {
    fn manifest(&self) -> &'static str {
        #[cfg(feature = "postgres")]
        {
            SERVER_MANIFEST.as_str()
        }
        #[cfg(not(feature = "postgres"))]
        {
            include_str!("../anything2mcp-provider.toml")
        }
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let names = names::Names::new()?;
        let max_rows = u32::try_from(setting(cx, "max_rows", 1000)).unwrap_or(1000);
        let result_bytes =
            u64::try_from(setting(cx, "result_bytes", 4_000_000)).unwrap_or(4_000_000);
        to.driver(
            names.mode.clone(),
            Arc::new(driver::SqlDriver::new(
                Arc::clone(cx.redact()),
                result_bytes,
                Arc::clone(&self.openers),
                cx.roots().clone(),
            )),
        )?;
        let registrations = CompilerRegistrations::new().surface_kind(SurfaceKindSpec::new(
            names::DATABASE,
            ADDRESS_KEY,
            JsonSchema::object()
                .required(ADDRESS_KEY, JsonSchema::text())
                .required("store", JsonSchema::words(store::STORES))
                .optional("root", JsonSchema::name()),
        ));
        // A server: its address, its port, its database, and the one schema
        // statements are read in. The role and its password come from the
        // session keeper's lease, never from a target file; there is no
        // switch for the link, which is held to the loopback address.
        #[cfg(feature = "postgres")]
        let registrations = registrations.surface_kind(SurfaceKindSpec::new(
            names::DATABASE_SERVER,
            SERVER_ADDRESS_KEY,
            JsonSchema::object()
                .required(SERVER_ADDRESS_KEY, JsonSchema::text())
                .required("store", JsonSchema::words(store::SERVER_STORES))
                .required("port", JsonSchema::int_between(1, 65535))
                .required("database", JsonSchema::name())
                .optional("schema", JsonSchema::name()),
        ));
        to.compiler(
            registrations
                .file_kind(Arc::new(kind::StatementKind::new(max_rows)))
                .lowering(Arc::new(lower::SqlLowering::new(names.mode.clone())))
                .lint_rule(Arc::new(lint::HazardNotDeclared))
                .lint_rule(Arc::new(lint::StatementUnused))
                .drafting_rule(Arc::new(draft::TableToStatements {
                    kind: names.table.clone(),
                    max_rows,
                }))
                .drafting_rule(Arc::new(draft::TableToReads {
                    kind: names.table.clone(),
                })),
        )?;
        to.discoverer(
            Arc::new(discover::CatalogDiscoverer::new(
                names.table.clone(),
                cx.telemetry().clock(),
            )?),
            Arc::new(discover::CatalogDescriber { kind: names.table }),
        )?;
        Ok(())
    }
}
