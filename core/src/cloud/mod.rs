//! Native cloud upload contracts (cloudmusic_util / M/78 / M/3127).
//! Allocation, audio metadata and account cloud resources have separate modules.
mod allocation;
mod api;
pub mod library;
mod metadata;
pub mod nos;
pub mod oss;
pub mod sort;
mod source;
pub mod upload;
pub use allocation::AttachmentKind;
pub use api::CloudApi;
pub use metadata::{AudioMetadata, UploadedAttachment};
