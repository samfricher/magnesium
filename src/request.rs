//! One cooperative request lifetime shared by blocking source and learner work.
use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

/// Cooperative cancellation/budget token for synchronous request work.
///
/// Checked at real traversal/extraction boundaries (directory walk, per-entry
/// budget loop, on-demand extraction) and in SQLite progress callbacks where
/// supported. A dropped future alone does not stop blocking work; callers that
/// need early termination must signal this token and the worker must poll it.
/// Never carries prompt, response, credential, or source-content data.
#[derive(Clone, Debug, Default)]
pub(crate) struct OperationCancellation {
    cancelled: Option<Arc<AtomicBool>>,
    deadline: Option<Instant>,
    #[cfg(test)]
    test_hook: Option<TestHook>,
}

thread_local! {
    /// Request-scoped cancellation installed by an MCP blocking worker. Any
    /// [`OperationCancellation`] check on that thread observes it, including
    /// `none()` tokens created by helpers that do not thread the request token
    /// explicitly. The scope guard restores the previous value on drop so a
    /// pooled blocking thread never inherits stale cancellation.
    static REQUEST_CANCELLATION: RefCell<Option<OperationCancellation>> =
        const { RefCell::new(None) };
}

/// RAII scope installing the request cancellation observed by every
/// [`OperationCancellation`] check on this thread. Dropping the guard restores
/// the previous scope.
pub(crate) struct RequestCancellationScope {
    previous: Option<OperationCancellation>,
}

impl Drop for RequestCancellationScope {
    fn drop(&mut self) {
        REQUEST_CANCELLATION.with(|slot| {
            *slot.borrow_mut() = self.previous.take();
        });
    }
}

/// Install the request cancellation for work running on the current blocking
/// worker. The MCP server bridges its request `CancellationToken` into the
/// supplied token; dropping or cancelling the request sets its flag, and this
/// scope makes every nested request check observe it.
pub(crate) fn enter_request_cancellation(
    cancellation: &OperationCancellation,
) -> RequestCancellationScope {
    let previous = REQUEST_CANCELLATION.with(|slot| slot.replace(Some(cancellation.clone())));
    RequestCancellationScope { previous }
}

impl OperationCancellation {
    pub(crate) fn none() -> Self {
        Self {
            cancelled: None,
            deadline: None,
            #[cfg(test)]
            test_hook: None,
        }
    }

    pub(crate) fn with_flag(flag: Arc<AtomicBool>) -> Self {
        Self {
            cancelled: Some(flag),
            deadline: None,
            #[cfg(test)]
            test_hook: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_deadline(deadline: Instant) -> Self {
        Self {
            cancelled: None,
            deadline: Some(deadline),
            test_hook: None,
        }
    }

    /// Preserve caller cancellation while imposing the preparation deadline.
    /// A stricter caller deadline continues to win.
    pub(crate) fn with_earlier_deadline(&self, deadline: Instant) -> Self {
        Self {
            cancelled: self.cancelled.clone(),
            #[cfg(test)]
            test_hook: self.test_hook.clone(),
            deadline: Some(
                self.deadline
                    .map_or(deadline, |existing| existing.min(deadline)),
            ),
        }
    }

    /// This token's own state, excluding any request-scoped ambient
    /// cancellation.
    fn directly_cancelled(&self) -> bool {
        if self
            .cancelled
            .as_deref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
        {
            return true;
        }
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        if self.directly_cancelled() {
            return true;
        }
        REQUEST_CANCELLATION.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(OperationCancellation::directly_cancelled)
        })
    }
}

#[cfg(test)]
#[derive(Clone)]
struct TestHook(Arc<dyn Fn(&'static str) + Send + Sync>);
#[cfg(test)]
impl std::fmt::Debug for TestHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("request test hook")
    }
}
#[cfg(test)]
impl OperationCancellation {
    pub(crate) fn with_test_hook(mut self, hook: Arc<dyn Fn(&'static str) + Send + Sync>) -> Self {
        self.test_hook = Some(TestHook(hook));
        self
    }
}
#[cfg(test)]
pub(crate) fn test_checkpoint(name: &'static str) {
    let hook =
        REQUEST_CANCELLATION.with(|slot| slot.borrow().as_ref().and_then(|c| c.test_hook.clone()));
    if let Some(hook) = hook {
        (hook.0)(name);
    }
}
