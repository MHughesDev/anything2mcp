//! The handlers of the actions that are built. An action with no handler
//! here answers `input`/`action_not_built`.

mod machine;
mod meta;
mod provider;

use crate::def::HandlerFn;

/// Defines a handler: a function of the kernel and the call that returns a
/// boxed future, as [`HandlerFn`] needs.
macro_rules! handler {
    ($(#[$meta:meta])* $name:ident, |$kernel:ident, $call:ident| $body:block) => {
        $(#[$meta])*
        pub(crate) fn $name(
            $kernel: &$crate::kernel::Kernel,
            $call: $crate::dispatch::Call,
        ) -> $crate::def::BoxFuture<'_, Result<$crate::dispatch::Output, a2m_core::Failure>> {
            Box::pin(async move $body)
        }
    };
}
pub(crate) use handler;

/// The kernel's function for an action, when it is built.
pub(crate) fn handler_for(action: &str) -> Option<HandlerFn> {
    Some(match action {
        "action.search" => meta::action_search,
        "action.describe" => meta::action_describe,
        "action.run" => meta::action_run,
        "docs.get" => meta::docs_get,
        "engine.status" => machine::engine_status,
        "config.show" => machine::config_show,
        "doctor.run" => machine::doctor_run,
        "provider.list" => provider::provider_list,
        "provider.describe" => provider::provider_describe,
        _ => return None,
    })
}
