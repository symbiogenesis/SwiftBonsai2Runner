//! Retained caller identity for bounded native startup operations.
use std::{
    cell::RefCell,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(windows)]
use bonsai_windows_process::ParentProcess;

static CONTEXT: OnceLock<Context> = OnceLock::new();

thread_local! {
    static PEER_CANCELLED: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

struct PeerCancellation(Option<Arc<AtomicBool>>);

impl Drop for PeerCancellation {
    fn drop(&mut self) {
        PEER_CANCELLED.with(|current| *current.borrow_mut() = self.0.take());
    }
}

// Scoped to this worker thread, including unwinding. Existing hash and owned
// subprocess checkpoints inherit peer failure without changing direct callers.
pub(crate) fn with_peer_cancellation<T>(
    cancelled: &Arc<AtomicBool>,
    operation: impl FnOnce() -> T,
) -> T {
    let _restore = PeerCancellation(
        PEER_CANCELLED.with(|current| current.replace(Some(Arc::clone(cancelled)))),
    );
    operation()
}

struct Context {
    #[cfg(windows)]
    parent: Option<ParentProcess>,
}

impl Context {
    fn new(parent: Option<(u32, u64)>) -> crate::Result<Self> {
        #[cfg(windows)]
        {
            let parent = parent
                .map(|(id, creation)| ParentProcess::open(id, creation))
                .transpose()
                .map_err(|error| {
                    format!("Startup caller identity could not be verified: {error}")
                })?;
            Ok(Self { parent })
        }
        #[cfg(not(windows))]
        {
            if parent.is_some() {
                return Err("Startup caller identity requires Windows.".into());
            }
            Ok(Self {})
        }
    }

    fn checkpoint(&self) -> crate::Result<()> {
        #[cfg(windows)]
        if let Some(parent) = &self.parent {
            parent
                .ensure_alive()
                .map_err(|error| format!("Startup cancelled: {error}"))?;
        }
        Ok(())
    }
}

/// Retains the optional caller identity for this startup command.
///
/// The creation identity is a Windows UTC creation FILETIME, in 100-nanosecond
/// ticks since 1601. A retained noninheritable handle prevents PID reuse from
/// making an exited caller appear alive. Call this once before dispatching a
/// startup command; direct CLI callers may omit the parent identity.
///
/// # Errors
/// Rejects an inaccessible, exited or mismatched caller, unsupported caller
/// tracking on another platform, or repeated initialization.
pub fn initialize(parent: Option<(u32, u64)>) -> crate::Result<()> {
    CONTEXT
        .set(Context::new(parent)?)
        .map_err(|_| "Startup cancellation context was already initialized.".into())
}

/// Checks whether the retained startup caller remains alive.
///
/// # Errors
/// Rejects an exited caller or failed native wait after initialization.
pub fn checkpoint() -> crate::Result<()> {
    if PEER_CANCELLED.with(|current| {
        current
            .borrow()
            .as_ref()
            .is_some_and(|cancelled| cancelled.load(Ordering::Acquire))
    }) {
        return Err("Startup operation cancelled after a peer failed.".into());
    }
    CONTEXT.get().map_or(Ok(()), Context::checkpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_cancellation_is_scoped_and_restored_after_unwind() -> crate::Result<()> {
        let cancelled = Arc::new(AtomicBool::new(true));
        let live = Arc::new(AtomicBool::new(false));
        with_peer_cancellation(&cancelled, || {
            assert!(checkpoint().is_err());
            with_peer_cancellation(&live, || assert!(checkpoint().is_ok()));
            assert!(checkpoint().is_err());
        });
        checkpoint()?;
        let panic = std::panic::catch_unwind(|| {
            with_peer_cancellation(&cancelled, || {
                assert!(checkpoint().is_err());
                std::panic::resume_unwind(Box::new("Synthetic scoped cancellation unwind."));
            });
        });
        assert!(panic.is_err());
        checkpoint()
    }

    #[test]
    fn optional_caller_does_not_require_global_state() -> crate::Result<()> {
        let context = Context::new(None)?;
        context.checkpoint()?;
        context.checkpoint()
    }

    #[test]
    fn invalid_caller_identity_rejects() {
        assert!(Context::new(Some((0, 0))).is_err());
        assert!(Context::new(Some((std::process::id(), 1))).is_err());
    }
}
