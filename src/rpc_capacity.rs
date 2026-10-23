use crate::protocol::ToolCall;
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Long execution/observation waits cannot consume the capacity reserved for controls.
#[derive(Clone)]
pub(crate) struct RpcCapacity {
    work: Arc<Semaphore>,
    control: Arc<Semaphore>,
}
impl RpcCapacity {
    pub fn new(limit: usize) -> Self {
        Self {
            work: Arc::new(Semaphore::new(limit)),
            control: Arc::new(Semaphore::new(limit)),
        }
    }
    pub fn acquire(&self, call: &ToolCall) -> Option<OwnedSemaphorePermit> {
        let immediate_status = call.name == "command_status"
            && call
                .arguments
                .get("wait_ms")
                .is_none_or(|value| value.is_null() || value.as_u64() == Some(0));
        let capacity = if immediate_status
            || matches!(
                call.name.as_str(),
                "session_info" | "write_stdin" | "stop_command"
            ) {
            &self.control
        } else {
            &self.work
        };
        capacity.clone().try_acquire_owned().ok()
    }
}
