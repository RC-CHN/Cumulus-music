//! Recovered M/21 resource threads, M/438 pages and M/232 comment likes.
mod api;
mod cache;
mod floor;
mod likes;
mod model;
mod service;
mod writes;
pub use api::CommentApi;
pub use floor::{CommentFloorPage, CommentFloorQuery};
pub use likes::CommentLike;
pub use model::{
    Comment, CommentAuthor, CommentPage, CommentQuery, CommentReply, CommentResource, CommentSort,
    ResourceKind,
};
pub use service::Comments;
pub use writes::{CommentWrite, CommentWriteCommand, CommentWritePhase, validate_comment_text};

pub(crate) fn vinyl_query(offset: usize) -> CommentQuery {
    CommentQuery {
        sort: CommentSort::Latest,
        limit: crate::protocol::api::VinylCommentRead::LIMIT,
        offset,
        before_time: 0,
        comment_id: None,
        compare_user_location: false,
        compose_concert: false,
        mark_replied: false,
        force_flat_comment: false,
        show_inner: false,
    }
}
