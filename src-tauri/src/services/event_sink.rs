use crate::{
    core::ssh::executor::EventSink,
    domain::events::{EventSequence, ExecutionEvent, ExecutionEventPayload},
    error::AppResult,
};

pub struct MonotonicEventSink<'a, E: EventSink> {
    inner: &'a mut E,
    sequence: EventSequence,
}

impl<'a, E: EventSink> MonotonicEventSink<'a, E> {
    pub fn new(inner: &'a mut E) -> Self {
        Self {
            inner,
            sequence: EventSequence::default(),
        }
    }

    pub fn emit(&mut self, payload: ExecutionEventPayload) -> AppResult<()> {
        self.inner.send(self.sequence.next(payload))
    }
}

impl<E: EventSink> EventSink for MonotonicEventSink<'_, E> {
    fn send(&mut self, event: ExecutionEvent) -> AppResult<()> {
        self.emit(event.payload)
    }
}
