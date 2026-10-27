//! Official M/24, M/75, M/137, M/600 and async:listenTogether* contracts.
//! HTTP control and message reduction are separate from the IM/RTC adapter.
//! No polling endpoint is represented as a replacement for that adapter.
mod api;
mod message;
mod model;
pub mod native_im;
mod privilege;
mod queue;
mod service;
mod sync;
pub mod transport;

pub use api::{CreatedRoom, ImCredentials, RtcCredentials, TogetherApi};
pub use message::{MessageKind, RoomMessage};
pub use model::{Member, Room, RoomRole, SharedPlaylist, TogetherSnapshot};
pub use privilege::{CommonPrivilege, LimitedMember, Preview};
pub use queue::{ListCommand, ListCommandKind, SharedMode, Version, shared_window, versions_equal};
pub use service::{
    EndNotice, IncomingPhase, IncomingPlayback, SharedReception, TogetherOperation, TogetherPhase,
    TogetherService, TogetherState, WriteIntent,
};
pub use sync::{CommandKind, Direction, PlayCommand, PlayState, PlaybackSync, TimedCommand};
