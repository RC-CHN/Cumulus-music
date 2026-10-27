//! Explicit public-comment intents. A persisted Sending intent is never resent.
mod api;
mod manager;
mod model;
mod reconcile;
pub use model::{CommentWrite, CommentWriteCommand, CommentWritePhase, validate_comment_text};
pub(super) const JOURNAL: &str = "comment-writes-v1";
