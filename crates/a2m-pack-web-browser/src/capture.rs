//! The observing tab as a traffic capture of the provider contract
//! (decision 0391): what [`ObserveTab::exchanges`] records, in the
//! contract's form. A pack that holds observing tabs offers the capability
//! by declaring it and registering a source that opens one.

use a2m_provider::{BoxFuture, CapturedRequest, CapturedTraffic, Deadline, TrafficCapture};

use crate::tab::{ObserveTab, PageExchange, PageExchanges};

impl TrafficCapture for ObserveTab {
    fn start(&self) {
        self.record_exchanges();
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
    }
}
