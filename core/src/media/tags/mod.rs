//! Official M/23 metadata plans; container adapters operate on staging copies.
mod comment;
mod helpers;
mod metadata;
mod music;
mod plan;
mod voice;
pub(crate) mod writer;

pub(crate) use comment::COMMENT_KEY;
pub use comment::{COMMENT_PREFIX, decode_comment, encode_comment};
pub use metadata::{MetadataKind, build_metadata};
pub use plan::{StandardTags, TagPlan, official_requires_encryption, track_plan, voice_plan};
pub use writer::{TagContainer, TagWriteReport, write_tagged_copy};
