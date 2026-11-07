//! Optional ownership for small workers, independent from passive snapshots.
use crate::online::Context;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
pub struct WorkerHandle {
    pub id: u64,
    pub(crate) context: Context,
}
impl WorkerHandle {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let mut context = Context::standalone();
        context.id = id;
        Self { id, context }
    }
    pub fn cancel(&self) {
        self.context.cancel();
    }
    pub fn is_cancelled(&self) -> bool {
        self.context.check().is_err()
    }
}

pub struct WorkerReply<T> {
    pub id: Option<u64>,
    pub value: T,
    pub finished: bool,
    handle: Option<WorkerHandle>,
}
impl<T> WorkerReply<T> {
    pub fn background(value: T) -> Self {
        Self {
            id: None,
            value,
            finished: true,
            handle: None,
        }
    }
    pub fn owned(handle: &WorkerHandle, value: T, finished: bool) -> Self {
        Self {
            id: Some(handle.id),
            value,
            finished,
            handle: Some(handle.clone()),
        }
    }
    pub(crate) fn is_cancelled(&self) -> bool {
        self.handle.as_ref().is_some_and(WorkerHandle::is_cancelled)
    }
}

pub(crate) struct WorkerRequest<T> {
    pub value: T,
    pub handle: Option<WorkerHandle>,
}
