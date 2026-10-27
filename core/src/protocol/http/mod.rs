//! Fixed-origin HTTP transport. Wire preparation, credentials and network I/O
//! are separate so scope and mutation behavior can be tested independently.
mod backend;
mod client;
mod origin;
mod profile;
mod request;
pub use backend::{BodyStatus, HttpBackend, HttpReply, ReqwestBackend};
pub use client::{MusicHttpClient, ResponseInfo};
pub use origin::{
    ApiOrigin, RoutingPolicy, WireRoute, select_route, select_route_with_policy, validate_path,
};
pub use profile::{APP_VERSION, ClientProfile, OFFICIAL_WINDOWS_USER_AGENT};
pub use request::{EapiStyle, RequestInput, RequestOptions, RequestPlan, build_request};
