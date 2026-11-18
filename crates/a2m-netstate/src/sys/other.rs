//! A system with no reader: every fact is a named stop (rule 7).

use crate::error::{Fact, NetError};
use crate::model::{Interfaces, Listeners, Neighbours, Proxy, Resolvers, Routes};

fn not_built<T>(fact: Fact) -> Result<T, NetError> {
    Err(NetError::NotBuilt {
        fact,
        system: std::env::consts::OS,
    })
}

pub(crate) fn interfaces() -> Result<Interfaces, NetError> {
    not_built(Fact::Interfaces)
}

pub(crate) fn routes() -> Result<Routes, NetError> {
    not_built(Fact::Routes)
}

pub(crate) fn resolvers() -> Result<Resolvers, NetError> {
    not_built(Fact::Resolvers)
}

pub(crate) fn neighbours() -> Result<Neighbours, NetError> {
    not_built(Fact::Neighbours)
}

pub(crate) fn listeners() -> Result<Listeners, NetError> {
    not_built(Fact::Listeners)
}

pub(crate) fn proxy() -> Result<Proxy, NetError> {
    not_built(Fact::Proxy)
}
