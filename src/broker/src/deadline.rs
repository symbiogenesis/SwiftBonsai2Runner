//! Optional caller deadlines, separate from bounded transport operations.

use std::{future::pending, pin::Pin};
use tokio::time::{Instant, Sleep, error::Elapsed, sleep_until, timeout_at};

/// An optional absolute request deadline supplied by a trusted caller.
///
/// Ordinary chat has no elapsed-time cap. Explicit diagnostic and embedding
/// callers can still bound the complete request by passing an [`Instant`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequestDeadline(Option<Instant>);

impl RequestDeadline {
    /// No total request deadline; cancellation and individual I/O bounds remain.
    pub const NONE: Self = Self(None);

    pub(crate) fn expired(self) -> bool {
        self.0.is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub(crate) fn limit(self, operation_deadline: Instant) -> Instant {
        self.0.map_or(operation_deadline, |deadline| {
            deadline.min(operation_deadline)
        })
    }

    pub(crate) fn earlier(self, operation_deadline: Instant) -> Self {
        Self(Some(self.limit(operation_deadline)))
    }

    pub(crate) async fn timeout<F: Future>(self, operation: F) -> Result<F::Output, Elapsed> {
        match self.0 {
            Some(deadline) => timeout_at(deadline, operation).await,
            None => Ok(operation.await),
        }
    }

    pub(crate) fn timer(self) -> Option<Pin<Box<Sleep>>> {
        self.0.map(|deadline| Box::pin(sleep_until(deadline)))
    }

    pub(crate) async fn wait(self) {
        match self.0 {
            Some(deadline) => sleep_until(deadline).await,
            None => pending().await,
        }
    }
}

impl From<Instant> for RequestDeadline {
    fn from(deadline: Instant) -> Self {
        Self(Some(deadline))
    }
}
