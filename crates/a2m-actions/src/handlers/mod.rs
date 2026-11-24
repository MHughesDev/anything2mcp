//! The handlers of the actions that are built. An action with no handler
//! here answers `input`/`action_not_built`.

mod access;
mod admin;
pub(crate) mod build;
pub(crate) mod captures;
mod client;
mod drafts;
mod machine;
mod meta;
mod promote;
mod provider;
mod reach;
mod scout;
pub(crate) mod scout_report;
pub(crate) mod sessions;
pub(crate) mod tools;
pub(crate) mod watches;

pub(crate) use scout::{SCOUT_SCRATCH, discovery_report_json};

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
        "net.interfaces" => machine::net_interfaces,
        "net.routes" => machine::net_routes,
        "net.resolvers" => machine::net_resolvers,
        "net.neighbours" => machine::net_neighbours,
        "net.listeners" => machine::net_listeners,
        "net.proxy" => machine::net_proxy,
        "net.standing" => machine::net_standing,
        "net.current" => reach::net_current,
        "reach.name" => reach::reach_name,
        "reach.list" => reach::reach_list,
        "reach.withdraw" => reach::reach_withdraw,
        "provider.list" => provider::provider_list,
        "provider.describe" => provider::provider_describe,
        "tool.search" => tools::tool_search,
        "tool.describe" => tools::tool_describe,
        "tool.call" => tools::tool_call,
        "run.get" => tools::run_get,
        "run.list" => tools::run_list,
        "run.resume" => tools::run_resume,
        "run.cancel" => tools::run_cancel,
        "trace.show" => tools::trace_show,
        "target.show" => tools::target_show,
        "target.add" => build::target_add,
        "target.list" => drafts::target_list,
        "target.status" => drafts::target_status,
        "draft.read" => drafts::draft_read,
        "draft.try" => tools::draft_try,
        "discover.run" => tools::discover_run,
        "target.probe" => scout::target_probe,
        "auth.detect" => scout::auth_detect,
        "scout.run" => scout::scout_run,
        "spec.import" => scout::spec_import,
        "scout.report" => scout::scout_report,
        "finding.list" => tools::finding_list,
        "finding.describe" => tools::finding_describe,
        "unknown.list" => tools::unknown_list,
        "draft.write" => drafts::draft_write,
        "draft.delete" => drafts::draft_delete,
        "binding.promote" => promote::binding_promote,
        "shadow.report" => promote::shadow_report,
        "client.connect" => client::client_connect,
        "test.run" => drafts::test_run,
        "fixture.record" => crate::fixture::fixture_record,
        "draft.lint" => build::draft_lint,
        "version.publish" => build::version_publish,
        "publish.request" => build::publish_request,
        "policy.explain" => admin::policy_explain,
        "toolset.create" => access::toolset_create,
        "toolset.update" => access::toolset_update,
        "toolset.remove" => access::toolset_remove,
        "toolset.list" => access::toolset_list,
        "grant.add" => access::grant_add,
        "grant.remove" => access::grant_remove,
        "grant.list" => access::grant_list,
        "policy.test" => access::policy_test,
        "policy.publish" => access::policy_publish,
        "policy.versions" => access::policy_versions,
        "policy.tools_allowed" => access::policy_tools_allowed,
        "policy.who_can" => access::policy_who_can,
        "audit.query" => admin::audit_query,
        "secret.set" => admin::secret_set,
        "killswitch.set" => admin::killswitch_set,
        "session.status" => admin::session_status,
        "session.login" => sessions::session_login,
        "session.revoke" => sessions::session_revoke,
        "session.list" => sessions::session_list,
        "session.end" => sessions::session_end,
        "watch.create" => watches::watch_create,
        "watch.confirm" => watches::watch_confirm,
        "watch.get" => watches::watch_get,
        "watch.list" => watches::watch_list,
        "watch.changes" => watches::watch_changes,
        "watch.pause" => watches::watch_pause,
        "watch.resume" => watches::watch_resume,
        "watch.end" => watches::watch_end,
        "capture.start" => captures::capture_start,
        "capture.stop" => captures::capture_stop,
        _ => return None,
    })
}
