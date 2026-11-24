//! The observing tab and the watching tab as traffic captures of the
//! provider contract (decisions 0391 and 0392): what
//! [`ObserveTab::exchanges`] and [`WatchTab::exchanges`] record, in the
//! contract's form. A pack that holds such tabs offers the capability by
//! declaring it and registering a source that opens one.

use a2m_provider::{BoxFuture, CapturedRequest, CapturedTraffic, Deadline, TrafficCapture};

use crate::tab::{ObserveTab, PageExchange, PageExchanges, WatchTab};

impl TrafficCapture for ObserveTab {
    fn start(&self) {
        self.record_exchanges();
    }

    fn take_recorded<'a>(&'a self, until: Deadline) -> BoxFuture<'a, CapturedTraffic> {
        Box::pin(async move { captured(self.exchanges(until).await) })
    }
}

/// A watching tab records from the moment it opens: it is opened for a
/// capture alone, and the first page the engine sends it to is part of it.
impl TrafficCapture for WatchTab {
    fn start(&self) {
        self.record();
    }

    fn take_recorded<'a>(&'a self, until: Deadline) -> BoxFuture<'a, CapturedTraffic> {
        Box::pin(async move { captured(self.exchanges(until).await) })
    }
}

/// The tab's exchanges in the contract's form. Every field the tab keeps
/// is already structure; the contract checks it again on the way out.
#[must_use]
pub fn captured(taken: PageExchanges) -> CapturedTraffic {
    CapturedTraffic {
        requests: taken.exchanges.into_iter().map(request).collect(),
        dropped: taken.dropped,
        awaited: taken.awaited,
        withheld: 0,
    }
}

fn request(one: PageExchange) -> CapturedRequest {
    CapturedRequest {
        method: one.method,
        origin: one.origin,
        path: one.path,
        query: one.query,
        query_left_out: one.query_left_out,
        credential_headers: one.credential_headers,
        set_headers: one.set_headers,
        headers_left_out: false,
        status: one.status,
        media_type: one.content_type,
        fields: one.fields,
        names_withheld: one.names_withheld,
        body_media_type: one.body_media_type,
        body_fields: one.body_fields,
        body_names_withheld: one.body_names_withheld,
    }
}
