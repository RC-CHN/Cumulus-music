//! NCM framing follows recovered cloudmusic.dll CE73B0/CE7580/CE62E0.
//! The keybox/audio transform is adapted from the supplied MIT ncmdump source;
//! see docs/third-party/ncmdump-LICENSE.txt and core-ncm-contracts.txt.
mod cipher;
mod export;
mod reader;
mod writer;

pub use export::{ExtractReport, extract};
pub use reader::{AudioFormat, HeaderPolicy, NcmInfo, NcmReader};
pub use writer::{NcmWriteOptions, NcmWriteReport, write_file, write_plan_stream, write_stream};

const MAGIC: &[u8; 8] = b"CTENFDAM";
const CORE_KEY: &[u8; 16] = b"hzHRAmso5kInbaxW";
const META_KEY: &[u8; 16] = super::tags::COMMENT_KEY;
const KEY_PREFIX: &[u8] = b"neteasecloudmusic";
const META_PREFIX: &[u8] = super::tags::COMMENT_PREFIX.as_bytes();
const MAX_FIELD: u64 = 16 * 1024 * 1024;
const MAX_COVER: u64 = 64 * 1024 * 1024;
