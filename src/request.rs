use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

#[derive(Clone, Debug)]
pub(crate) struct OperationCancellation {
    cancelled: Option<Arc<AtomicBool>>,
    deadline: Option<Instant>,
    #[cfg(test)]
    test_hook: Option<TestHook>,
}

thread_local! {
    static REQUEST_CANCELLATION: RefCell<Option<OperationCancellation>> =
        const { RefCell::new(None) };
}

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
