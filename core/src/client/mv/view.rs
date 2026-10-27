use super::*;
use crate::{
    account::resources::MutationRecord,
    mv::{MvDetail, playback::VideoSnapshot},
};
use serde_json::json;
pub(super) fn card(d: &MvDetail) -> MvCard {
    MvCard {
        id: d.id.clone(),
        title: d.title.clone(),
        artists: if d.artists.is_empty() {
            vec![d.artist_name.clone()]
        } else {
            d.artists.iter().map(|a| a.name.clone()).collect()
        },
        cover_url: super::super::mapping::artwork(&json!(d.cover_url)),
        duration_ms: d.duration_ms,
        play_count: d.play_count,
        subscribed: d.subscribed,
    }
}
pub(super) fn detail(
    d: MvDetail,
    cached: bool,
    like_operation: Option<crate::mv::MvLike>,
) -> Result<MvView> {
    let downloads = d
        .download_options()
        .into_iter()
        .map(|b| {
            let reason = if b.point.unwrap_or(0) > 0 {
                Some("此清晰度需要云贝消费")
            } else if d.privilege.as_ref().is_some_and(|p| {
                p.unauthorized || p.fee.unwrap_or(0) > 0 && p.paid.unwrap_or(0) == 0
            }) {
                Some("当前账号尚未取得下载权限")
            } else {
                None
            };
            MvDownloadChoice {
                resolution: b.br,
                estimated_bytes: b.size,
                points: b.point.unwrap_or(0),
                available: reason.is_none(),
                reason,
            }
        })
        .collect();
    let mut play_resolutions = d
        .bitrates
        .iter()
        .map(|b| b.br)
        .filter(|br| {
            d.privilege
                .as_ref()
                .and_then(|p| p.max_play_br)
                .is_none_or(|max| *br <= max)
        })
        .collect::<Vec<_>>();
    play_resolutions.sort_unstable();
    Ok(MvView {
        card: card(&d),
        description: d.description,
        published_date: d.published_date,
        play_resolutions,
        downloads,
        cached,
        liked: d.liked_known.then_some(d.liked),
        like_count: d.like_count,
        comment_count: d.comment_count,
        like_operation: like_operation.filter(|r| r.phase.pending()).map(like),
        related_songs: d
            .related_songs
            .iter()
            .map(super::super::mapping::track)
            .collect::<Result<Vec<_>>>()?,
        related_privilege_failed: d.related_privilege_failed,
    })
}
pub(super) fn like(r: crate::mv::MvLike) -> MvLikeView {
    MvLikeView {
        id: r.id,
        desired: r.desired,
        observed: r.observed.as_ref().map(|o| o.liked),
        count: r.observed.map(|o| o.count),
        phase: r.phase,
    }
}
pub(super) fn operation(r: MutationRecord) -> MvMutationView {
    MvMutationView {
        id: r.key,
        desired: r.desired == true,
        observed: r.observed.and_then(|v| v.as_bool()),
        phase: r.phase,
    }
}
pub(super) fn video(v: crate::mv::playback::VideoSnapshot) -> VideoView {
    let VideoSnapshot {
        account,
        revision,
        play_id,
        id,
        status,
        requested_resolution,
        actual_resolution,
        local,
        position,
        duration,
        paused,
        buffering,
        volume,
        speed,
        error,
        ..
    } = v;
    VideoView {
        epoch: account.generation.to_string(),
        revision: revision.to_string(),
        play_id,
        id,
        state: status,
        requested_resolution,
        actual_resolution,
        local,
        position,
        duration,
        paused,
        buffering,
        volume,
        speed,
        error: error.map(|_| "视频播放失败，请重试或切换清晰度"),
    }
}
