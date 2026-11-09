//! Copyright-first startup candidate: native read/normalization and no unique create.
use super::{DailyNotificationKind, DailyNotificationSchedule, MusicClient, READ_LIMIT};
use crate::{
    Error, Result,
    catalog::merge_song_privileges,
    client::{dto::TrackView, mapping},
    protocol::api::{CopyrightNotificationRead, identifier, validate_response},
};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};

pub(super) struct ReturningSong {
    pub(super) track: TrackView,
    pub(super) playable: bool,
}

pub(super) struct CopyrightOffer {
    pub(super) playlist: String,
    pub(super) rows: Vec<ReturningSong>,
    pub(super) description: String,
}

pub(super) enum Candidate {
    Empty,
    /// A legitimate official candidate wins arbitration even though this host
    /// cannot publish its unique playlist or display that primary action.
    NeedsUniquePlaylist,
    Existing(CopyrightOffer),
}

fn normalized(reply: &Value, requested: &HashSet<String>) -> Result<Vec<ReturningSong>> {
    let songs = reply["songs"]
        .as_array()
        .filter(|rows| rows.len() <= CopyrightNotificationRead::MAX_SONGS)
        .ok_or(Error::Response("invalid copyright song detail rows"))?;
    let privileges = match reply.get("privileges") {
        None => &[][..],
        Some(rows) => rows
            .as_array()
            .filter(|rows| rows.len() <= CopyrightNotificationRead::MAX_SONGS)
            .ok_or(Error::Response("invalid copyright song privileges"))?
            .as_slice(),
    };
    let detail_ids: HashSet<_> = songs
        .iter()
        .map(|song| identifier(&song["id"]))
        .collect::<Result<_>>()?;
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    for song in merge_song_privileges(songs, privileges)? {
        let id = identifier(&song["id"])?;
        // M/21 can synthesize privilege-only placeholders; they are neither a
        // usable preview nor a legitimate source for an arbitrary media read.
        if !requested.contains(&id) || !detail_ids.contains(&id) || !seen.insert(id) {
            continue;
        }
        let mut track = mapping::track(&song)?;
        track.cover_url = track.cover_url.filter(|url| url.len() <= 2048);
        if !track.name.is_empty() {
            rows.push(ReturningSong {
                track,
                playable: mapping::online_play_all_eligible(&song),
            });
        }
    }
    Ok(rows)
}

fn classify(data: &Value, rows: Vec<ReturningSong>) -> Candidate {
    if rows.is_empty() {
        return Candidate::Empty;
    }
    let Ok(playlist) = identifier(&data["returnSongPlaylistId"]) else {
        return Candidate::NeedsUniquePlaylist;
    };
    if playlist.len() > 32 {
        return Candidate::NeedsUniquePlaylist;
    }
    let description = data["songNum"]
        .as_u64()
        .filter(|n| (1..=1_000_000).contains(n))
        .map(|count| format!("有{count}首你喜欢的歌曲重新上架了！"))
        .unwrap_or_default();
    Candidate::Existing(CopyrightOffer {
        playlist,
        rows,
        description,
    })
}

impl MusicClient {
    pub(super) async fn copyright_candidate(
        &self,
        schedule: &DailyNotificationSchedule,
        foreground: &(impl Fn() -> bool + Sync),
    ) -> Result<Candidate> {
        let permission = || {
            if self.can_offer_notification(
                schedule,
                DailyNotificationKind::Copyright,
                foreground,
            )? {
                Ok(())
            } else {
                Err(Error::StaleResponse)
            }
        };
        permission()?;
        let api = &schedule.api;
        let revision = &schedule.credentials;
        let reply = tokio::time::timeout(
            READ_LIMIT,
            api.request_copyright_notification(
                CopyrightNotificationRead::popup(),
                revision,
                &permission,
            ),
        )
        .await
        .map_err(|_| Error::State("copyright popup read timed out"))??;
        permission()?;
        let reply = validate_response(CopyrightNotificationRead::POPUP, reply, &[200])?;
        let data = &reply["data"];
        let Some(ids) = data["songIdList"].as_array().filter(|ids| !ids.is_empty()) else {
            return Ok(Candidate::Empty);
        };
        let read = CopyrightNotificationRead::details(ids)?;
        let requested = ids.iter().map(identifier).collect::<Result<_>>()?;
        let details = tokio::time::timeout(
            READ_LIMIT,
            api.request_copyright_notification(read, revision, &permission),
        )
        .await
        .map_err(|_| Error::State("copyright song details timed out"))??;
        permission()?;
        let details = validate_response(CopyrightNotificationRead::DETAILS, details, &[200])?;
        Ok(classify(data, normalized(&details, &requested)?))
    }

    pub(super) fn copyright_play_authority(
        &self,
        schedule: &DailyNotificationSchedule,
    ) -> impl Fn() -> bool + Send + Sync + 'static {
        let (owner, api, revision) = (
            schedule.owner.clone(),
            Arc::clone(&schedule.api),
            schedule.credentials.clone(),
        );
        let session = Arc::clone(&self.session);
        let experiments = Arc::clone(&self.pc_experiments);
        move || {
            session.require_api(&owner, &api).is_ok()
                && api
                    .verified_pc_experiment_revision()
                    .is_ok_and(|value| value.as_deref() == Some(&revision))
                && experiments
                    .observed_group(&owner, &api, &revision, "PH-PC-push2")
                    .is_ok_and(|value| value.as_deref() == Some("t1"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_valid_existing_playlist_materializes_a_candidate() {
        let payload = json!({"songs":[{"id":2,"name":"回归", "al":{"picUrl":"https://p1.music.126.net/a.jpg?SECRET=x"}},
            {"id":999,"name":"foreign"}],"privileges":[{"id":2,"pl":320000},{"id":3,"pl":320000}]});
        let requested = HashSet::from(["2".into(), "3".into()]);
        let rows = || normalized(&payload, &requested).unwrap();
        assert_eq!(rows().len(), 1);
        let Candidate::Existing(offer) =
            classify(&json!({"returnSongPlaylistId":81,"songNum":7}), rows())
        else {
            panic!("existing playlist should be available");
        };
        assert_eq!(offer.playlist, "81");
        assert_eq!(offer.description, "有7首你喜欢的歌曲重新上架了！");
        assert!(offer.rows[0].playable);
        assert_eq!(
            offer.rows[0].track.cover_url.as_deref(),
            Some("https://p1.music.126.net/a.jpg?param=320y320")
        );
        for data in [
            json!({}),
            json!({"returnSongPlaylistId":0}),
            json!({"returnSongPlaylistId":"foreign"}),
        ] {
            assert!(matches!(
                classify(&data, rows()),
                Candidate::NeedsUniquePlaylist
            ));
        }
        assert!(matches!(classify(&Value::Null, vec![]), Candidate::Empty));
    }

    #[test]
    fn signed_detail_keeps_original_packed_ids_and_normal_trial() {
        let (path, body) = CopyrightNotificationRead::details(&[json!(2), json!("001"), json!(2)])
            .unwrap()
            .into_request();
        assert_eq!(path, "/api/v3/song/detail");
        assert_eq!(
            body,
            json!({"c":r#"[{"id":"2","v":0},{"id":"001","v":0},{"id":"2","v":0}]"#,"trialMode":-1})
        );
        assert!(CopyrightNotificationRead::details(&[json!(0)]).is_err());
    }
}
