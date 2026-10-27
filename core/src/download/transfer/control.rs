use crate::Result;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirective {
    Continue,
    Pause,
}
/// Keep this callback quick. It may check account generation, durable commands
/// and directory migration state. It is never invoked under a DB mutex.
pub trait TransferControl: Send + Sync {
    fn checkpoint(&self) -> Result<TransferDirective>;
}
impl<F: Fn() -> Result<TransferDirective> + Send + Sync> TransferControl for F {
    fn checkpoint(&self) -> Result<TransferDirective> {
        self()
    }
}
#[derive(Clone)]
pub struct RunOptions {
    pub byte_budget: Option<u64>,
    pub control: Arc<dyn TransferControl>,
}
impl Default for RunOptions {
    fn default() -> Self {
        Self {
            byte_budget: None,
            control: Arc::new(|| Ok(TransferDirective::Continue)),
        }
    }
}
