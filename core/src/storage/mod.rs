//! Persistence primitives shared by account-scoped business domains.
mod operation_lock;
mod state;
pub use operation_lock::OperationLock;
pub use state::{StateRow, StateStore, StateTransaction};
