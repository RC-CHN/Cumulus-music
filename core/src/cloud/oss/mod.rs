//! OSS channel 3, recovered U:0x232cb0/0x242c80/0x2f2610/0x3164a0.
//! Object credentials and requests never use the account Cookie transport.
mod allocation;
mod backend;
mod crc;
mod manager;
mod recovery;
mod request;
mod response;
mod sign;
mod state;
mod transfer;
mod xml;
use crate::{Error, Result};
pub use allocation::OssAllocation;
pub use backend::{OssBackend, ReqwestOssBackend};
pub use crc::{check_crc64, crc64};
pub use manager::OssUploads;
pub use request::{OssOperation, OssRequest, OssRequestKind, PartTag};
pub use response::{Completion, ObjectDigest, OssReply, PartInfo, PartsPage};
pub use sign::sign_v1;
use state::{Job, Part, Pending};
pub use state::{OssControl, OssPending, OssRunOptions, OssStatus, OssView};
