//! The first-party packs. This file is the only place in the workspace
//! where a pack is named, and nothing is done with a pack here but hand it
//! to the registry builder.
//!
//! Every pack is compiled in. Whether one is enabled is the setting
//! `providers.disabled`, which the registry builder reads: a pack that is
//! switched off is listed as not enabled and registers nothing.
//!
//! Not here yet: the web pack's crates, which are being built, and the
//! desktop pack. Plug-ins found on the machine are not looked for.

use a2m_provider::{Pack, RegistryBuilder};

/// One first-party pack: its manifest, whose `[[settings]]` are the schema
/// of its settings group, and the one call that hands it to the registry.
pub(crate) struct FirstParty {
    pub(crate) manifest: &'static str,
    pub(crate) add: fn(&mut RegistryBuilder),
}

/// Every first-party pack of this build, in the order they are registered.
pub(crate) fn first_party() -> Vec<FirstParty> {
    vec![
        FirstParty {
            manifest: a2m_pack_http::HttpPack.manifest(),
            add: a2m_pack_http::add_to,
        },
        FirstParty {
            manifest: a2m_pack_mcp::MANIFEST,
            add: |builder| builder.add_pack(&a2m_pack_mcp::pack()),
        },
        FirstParty {
            manifest: a2m_pack_command::MANIFEST,
            add: a2m_pack_command::register,
        },
        FirstParty {
            manifest: a2m_pack_data::pack().manifest(),
            add: |builder| builder.add_pack(&a2m_pack_data::pack()),
        },
        FirstParty {
            manifest: a2m_pack_file::MANIFEST,
            add: |builder| builder.add_pack(&a2m_pack_file::pack()),
        },
        FirstParty {
            manifest: a2m_pack_stream::MANIFEST,
            add: a2m_pack_stream::register,
        },
        FirstParty {
            manifest: a2m_pack_device::MANIFEST,
            add: a2m_pack_device::add_to,
        },
    ]
}

/// Hands every first-party pack to the registry builder.
pub(crate) fn fill(builder: &mut RegistryBuilder) {
    for pack in first_party() {
        (pack.add)(builder);
    }
}
