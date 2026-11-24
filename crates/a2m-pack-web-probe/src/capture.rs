//! The pack's traffic capture source (decision 0399): an observing tab of
//! the base, opened on the surface's own address and recording the
//! requests its page makes by itself. The manifest declares it under
//! `points.traffic_capture` for `web` surfaces; what it yields reaches a
//! caller only through the contract's check.

use a2m_core::{AccountName, Failure, Identity};
use a2m_pack_web_browser::{
    Address, Look, Observer, Origins, PageEnvironment, ProfileKey, SafeRequests,
    looks_like_an_action,
};
use a2m_provider::{BoxFuture, CaptureSource, Deadline, SurfaceRef, TrafficCapture};

use crate::look::{DISCOVERY_ACCOUNT, after_origin, failure, looks_like_an_action_refused};

/// Opens an observing tab on a surface and records what its page asks
/// for by itself.
pub(crate) struct ObservingCapture(pub(crate) Observer);

impl CaptureSource for ObservingCapture {
    fn open<'a>(
        &'a self,
        surface: &'a SurfaceRef,
        deadline: Deadline,
    ) -> BoxFuture<'a, Result<Box<dyn TrafficCapture>, Failure>> {
        Box::pin(async move {
            let address = Address::new(&surface.surface.address)?;
            // The check every address of a discovery passes (decision 0144).
            if looks_like_an_action(after_origin(address.as_str())) {
                return Err(looks_like_an_action_refused("is"));
            }
            let profile = ProfileKey {
                target: surface.target.clone(),
                environment: surface.environment.clone(),
                identity: Identity::Account(AccountName::new(DISCOVERY_ACCOUNT)?),
            };
            let tab = self
                .0
                .observing_tab(
                    &profile,
                    &PageEnvironment::default(),
                    &SafeRequests::default(),
                    &Origins::of([&address]),
                )
                .await
                .map_err(|error| failure(&error))?;
            TrafficCapture::start(&tab);
            tab.goto(&address, deadline)
                .await
                .map_err(|error| failure(&error))?;
            Ok(Box::new(tab) as Box<dyn TrafficCapture>)
        })
    }
}
