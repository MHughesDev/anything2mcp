//! The back end of a system that has none yet: every method stops with
//! `system_not_built` before anything is asked of anyone.

use a2m_provider::Deadline;

use super::{App, AxError, Limits, Window};
use crate::tree::{El, Node, Tree};

/// The look-only handle, which on this system reaches nothing.
pub(crate) struct Look;

/// A watch that never begins.
pub(crate) struct Watch;

impl Watch {
    pub(crate) async fn next(&mut self, _deadline: Deadline) -> Result<bool, AxError> {
        Err(AxError::NotBuilt)
    }
}

impl Look {
    pub(crate) async fn connect(_session: Option<&str>) -> Result<Self, AxError> {
        Err(AxError::NotBuilt)
    }

    pub(crate) async fn application(
        &self,
        _program: &str,
        _name: &str,
        _digest: Option<&str>,
    ) -> Result<App, AxError> {
        Err(AxError::NotBuilt)
    }

    pub(crate) async fn windows(&self, _app: &App) -> Result<Vec<Window>, AxError> {
        Err(AxError::NotBuilt)
    }

    pub(crate) async fn tree(
        &self,
        _root: &El,
        _limits: &Limits,
        _deadline: Deadline,
    ) -> Result<Tree, AxError> {
        Err(AxError::NotBuilt)
    }

    pub(crate) async fn text(&self, _node: &Node, _limits: &Limits) -> Result<String, AxError> {
        Err(AxError::NotBuilt)
    }

    pub(crate) async fn watch(&self, _app: &App) -> Result<Watch, AxError> {
        Err(AxError::NotBuilt)
    }

    pub(crate) async fn close(self) {}
}
