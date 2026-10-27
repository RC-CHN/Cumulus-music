//! Cookie-free Watchman configuration and report transport. Hosts are a closed
//! set from the recovered loader; no account cookie or arbitrary URL is accepted.
mod config;
mod http;
mod request;
mod time;
pub use config::{ConfigService, ConfigSnapshot};
pub use http::{NativeBackend, Transport};
pub use request::{ApiHost, HttpMethod, Request, RequestKind};
pub use time::parse_server_time;
