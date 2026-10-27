//! Official M/3023/M/3100/M/3127 cloud list and deletion with durable recovery.
mod api;
mod delete;
mod service;
mod types;
use super::CloudApi;
use crate::{
    Error, Result,
    account::{AccountSession, SessionToken},
    storage::{OperationLock, StateStore},
};
use serde_json::{Value, json};
pub use service::CloudLibrary;
use std::{collections::BTreeSet, sync::Arc};
use types::{BatchPhase, DeleteBatch};
pub use types::{CloudEntry, CloudSnapshot, DeleteOperation, DeletePhase};
