//! The first-party packs. This file is the only place in the workspace
//! where a pack is named, and nothing is done with a pack here but join
//! its crates and hand it to the registry builder.
//!
//! Every pack is compiled in. Whether one is enabled is the setting
//! `providers.disabled`, which the registry builder reads: a pack that is
//! switched off is listed as not enabled and registers nothing.
//!
//! Not here yet: the desktop pack, which is not built. Plug-ins found on
//! the machine are not looked for.

use a2m_pack_web_browser::SharedBrowsers;
use a2m_provider::{Pack, RegistryBuilder};

/// One first-party pack: its manifest, whose `[[settings]]` are the schema
/// of its settings group, and the one call that hands it to the registry.
pub(crate) struct FirstParty {
    pub(crate) manifest: &'static str,
    pub(crate) add: Box<dyn Fn(&mut RegistryBuilder)>,
}

fn one(manifest: &'static str, add: impl Fn(&mut RegistryBuilder) + 'static) -> FirstParty {
    FirstParty {
        manifest,
        add: Box::new(add),
    }
}

/// Every first-party pack of this build, in the order they are registered.
pub(crate) fn first_party() -> Vec<FirstParty> {
    // The web pack has two crates that drive one set of browsers. The
    // handle on them is made once here and given to both: the interface
    // driver, and the probe that only observes.
    let browsers = SharedBrowsers::default();
    let (for_ui, for_probe) = (browsers.clone(), browsers.clone());
    vec![
        one(a2m_pack_web_ui::pack(browsers.clone()).manifest(), move |builder| {
            a2m_pack_web_ui::add_to(builder, for_ui.clone());
        }),
        one(a2m_pack_web_probe::pack(browsers).manifest(), move |builder| {
            a2m_pack_web_probe::add_to(builder, for_probe.clone());
        }),
        one(a2m_pack_http::HttpPack.manifest(), a2m_pack_http::add_to),
        one(a2m_pack_mcp::MANIFEST, |builder| {
            builder.add_pack(&a2m_pack_mcp::pack());
        }),
        one(a2m_pack_command::MANIFEST, a2m_pack_command::register),
        one(a2m_pack_data::pack().manifest(), |builder| {
            builder.add_pack(&a2m_pack_data::pack());
        }),
        one(a2m_pack_file::MANIFEST, |builder| {
            builder.add_pack(&a2m_pack_file::pack());
        }),
        one(a2m_pack_stream::MANIFEST, a2m_pack_stream::register),
        one(a2m_pack_device::MANIFEST, a2m_pack_device::add_to),
    ]
}

/// Hands every first-party pack to the registry builder.
pub(crate) fn fill(builder: &mut RegistryBuilder) {
    for pack in first_party() {
        (pack.add)(builder);
    }
}
