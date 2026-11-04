#[allow(dead_code)]
#[path = "support/playback/mod.rs"]
mod playback_support;
mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{
            PlaySelection, PlayerAction, PlayerView, ProfilePlayRecordPeriod as Period,
            ProfilePlayRecordRemovalAction as RankAction,
        },
    },
    membership::Quality,
    playback::controller::PlaybackStatus,
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json, to_value};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

const RECORD: &str = "/api/v1/play/record";
const REMOVE: &str = "/api/play/record/empty";
const GUEST: &str = "9007199254740993";
const DETAIL: &str = "/api/w/v1/user/detail/9007199254740993";

fn fixture(
    replies: impl IntoIterator<Item = Reply>,
) -> (
    tempfile::TempDir,
    Arc<ScriptApi>,
    Arc<AccountSession>,
    Arc<MusicClient>,
    String,
) {
    let dir = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(ScriptApi::new(replies));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = Arc::new(
        MusicClient::with_session(session.clone(), registry).with_audio_factory(|| {
            Ok(playback_support::audio_engine(Arc::new(
                playback_support::Device::default(),
            )))
        }),
    );
    (dir, api, session, client, epoch)
}

fn detail(visibility: Option<bool>) -> Value {
    let mut response =
        json!({"code":200,"profile":{"userId":GUEST,"nickname":"合成访客"},"listenSongs":27});
    if let Some(visibility) = visibility {
        response["peopleCanSeeMyPlayRecord"] = json!(visibility);
    }
    response
}

fn rows() -> Value {
    json!([{"song":{"id":71,"name":"合成曲目","artists":[{"name":"合成歌手"}],"url":"https://private.invalid/secret","voice_id":22},"playCount":54,"score":99}])
}

#[tokio::test]
async fn exact_week_and_all_requests_preserve_service_order_and_keep_guest_counts_native() {
    let (_dir, api, _, client, epoch) = fixture([
        json!({"code":200,"weekData":rows()}).into(),
        detail(Some(true)).into(),
        json!({"code":"200","weekData":[],"allData":rows()}).into(),
    ]);
    let own = to_value(
        client
            .profile_play_record(&epoch, None, Period::Week)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(own["epoch"], epoch);
    assert_eq!(own["value"]["isSelf"], true);
    assert_eq!(own["value"]["items"][0]["playCount"], 54);
    let guest = to_value(
        client
            .profile_play_record(&epoch, Some(GUEST), Period::All)
            .await
            .unwrap(),
    )
    .unwrap();
    let row = &guest["value"]["items"][0];
    assert_eq!(guest["value"]["isSelf"], false);
    assert_eq!(row["track"]["id"], "71");
    assert_eq!(row["track"]["artists"][0], "合成歌手");
    assert!(row.get("playCount").is_none());
    assert!(guest["value"].get("revision").is_none());
    for forbidden in ["private.invalid", "voiceId", "\"score\""] {
        assert!(!guest.to_string().contains(forbidden));
    }
    let calls = api.calls();
    assert_eq!(calls[0].path, RECORD);
    assert_eq!(
        calls[0].payload,
        json!({"offset":0,"total":true,"limit":120,"uid":"9","type":1})
    );
    assert_eq!(calls[1].path, DETAIL);
    assert_eq!(calls[1].payload, json!({"all":true,"userId":GUEST}));
    assert_eq!(calls[2].path, RECORD);
    assert_eq!(
        calls[2].payload,
        json!({"offset":0,"total":true,"limit":120,"uid":GUEST,"type":0})
    );
    assert!(calls.iter().all(|call| !call.cache));
}

#[tokio::test]
async fn profile_entry_and_fresh_permission_are_distinct_from_an_empty_or_failed_rank() {
    let mut blocked = detail(Some(true));
    blocked["profile"]["inBlacklist"] = json!(true);
    let (_dir, api, _, client, epoch) = fixture([
        detail(Some(true)).into(),
        json!({"code":200,"playlist":[],"more":false}).into(),
        detail(Some(false)).into(),
        detail(None).into(),
        blocked.into(),
        detail(Some(true)).into(),
        json!({"code":"403"}).into(),
        json!({"code":200,"weekData":[]}).into(),
        json!({"code":200,"weekData":null}).into(),
    ]);
    let profile = to_value(
        client
            .playlist_creator_profile(&epoch, GUEST)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        profile["value"]["listeningRank"],
        json!({"isSelf":false,"listenSongs":27})
    );
    for _ in 0..4 {
        let result = client
            .profile_play_record(&epoch, Some(GUEST), Period::Week)
            .await
            .unwrap();
        assert_eq!(
            to_value(result).unwrap()["value"],
            json!({"state":"denied"})
        );
    }
    let empty = client
        .profile_play_record(&epoch, None, Period::Week)
        .await
        .unwrap();
    assert_eq!(
        to_value(empty).unwrap()["value"],
        json!({"state":"ready","isSelf":true,"items":[]})
    );
    assert!(matches!(
        client.profile_play_record(&epoch, None, Period::Week).await,
        Err(Error::Response(_))
    ));
    assert_eq!(
        api.calls()
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        [
            DETAIL,
            "/api/user/playlist",
            DETAIL,
            DETAIL,
            DETAIL,
            DETAIL,
            RECORD,
            RECORD,
            RECORD
        ]
    );
}

#[tokio::test]
async fn previous_account_result_cannot_escape_ipc_after_switch() {
    let (reply, send) = gate();
    let (_dir, api, session, client, epoch) = fixture([reply]);
    let pending =
        tokio::spawn(async move { client.profile_play_record(&epoch, None, Period::Week).await });
    api.wait_for_calls(1).await;
    session
        .adopt(api, json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    send.send(Ok(json!({"code":200,"weekData":rows()})))
        .unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
}

async fn own_rank(client: &MusicClient, epoch: &str, period: Period) -> Value {
    to_value(
        client
            .profile_play_record(epoch, None, period)
            .await
            .unwrap(),
    )
    .unwrap()["value"]
        .clone()
}

fn revision(view: &Value) -> &str {
    view["revision"].as_str().unwrap()
}

#[tokio::test]
async fn removal_requires_the_exact_native_owner_period_and_song() {
    let (_dir, api, _, client, epoch) = fixture([
        json!({"code":200,"weekData":rows()}).into(),
        detail(Some(true)).into(),
        json!({"code":200,"weekData":rows()}).into(),
        json!({"code":200,"allData":rows()}).into(),
        json!({"code":200}).into(),
        json!({"code":200}).into(),
    ]);
    let week = own_rank(&client, &epoch, Period::Week).await;
    let guest = to_value(
        client
            .profile_play_record(&epoch, Some(GUEST), Period::Week)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(guest["value"]["revision"].is_null());
    let all = own_rank(&client, &epoch, Period::All).await;
    for (period, id, key) in [
        (Period::Week, "71", "00000000000000000000000000000000"),
        (Period::All, "71", revision(&week)),
        (Period::Week, "72", revision(&week)),
    ] {
        assert!(matches!(
            client
                .remove_profile_play_record(&epoch, period, id, key, RankAction::Remove)
                .await,
            Err(Error::StaleResponse)
        ));
    }
    assert_eq!(api.calls().len(), 4);
    let removed = client
        .remove_profile_play_record(
            &epoch,
            Period::Week,
            "71",
            revision(&week),
            RankAction::Remove,
        )
        .await
        .unwrap();
    assert_eq!(to_value(removed).unwrap()["value"], "removed");
    assert!(matches!(
        client
            .remove_profile_play_record(
                &epoch,
                Period::Week,
                "71",
                revision(&week),
                RankAction::Remove
            )
            .await,
        Err(Error::StaleResponse)
    ));
    client
        .remove_profile_play_record(
            &epoch,
            Period::All,
            "71",
            revision(&all),
            RankAction::Remove,
        )
        .await
        .unwrap();
    let calls = api.calls();
    assert_eq!(calls.len(), 6);
    assert_eq!(calls[4].path, REMOVE);
    assert_eq!(calls[4].payload, json!({"type":"1","songId":"71"}));
    assert_eq!(calls[5].path, REMOVE);
    assert_eq!(calls[5].payload, json!({"type":"0","songId":"71"}));
    assert!(calls.iter().all(|call| !call.cache));
}

#[tokio::test]
async fn ambiguous_write_survives_reopen_and_only_readback_can_report_absence() {
    let (dir, api, _, client, epoch) = fixture([
        json!({"code":200,"weekData":rows()}).into(),
        Reply::Ready(Err(Error::Transport("lost rank reply".into()))),
        json!({"code":200,"weekData":rows()}).into(),
        json!({"code":200,"weekData":rows()}).into(),
        json!({"code":200,"weekData":[]}).into(),
    ]);
    let week = own_rank(&client, &epoch, Period::Week).await;
    let remove = client
        .remove_profile_play_record(
            &epoch,
            Period::Week,
            "71",
            revision(&week),
            RankAction::Remove,
        )
        .await
        .unwrap();
    assert_eq!(to_value(remove).unwrap()["value"], "uncertain");
    drop(client);
    let registry = ProfileRegistry::open(dir.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = MusicClient::with_session(session, registry);
    let week = own_rank(&client, &epoch, Period::Week).await;
    assert_eq!(week["items"][0]["removalUncertain"], true);
    for (action, expected) in [
        (RankAction::Remove, "uncertain"),
        (RankAction::Check, "uncertain"),
        (RankAction::Check, "not_listed"),
    ] {
        let result = client
            .remove_profile_play_record(&epoch, Period::Week, "71", revision(&week), action)
            .await
            .unwrap();
        assert_eq!(to_value(result).unwrap()["value"], expected);
    }
    assert_eq!(
        api.calls()
            .iter()
            .map(|c| c.path.as_str())
            .collect::<Vec<_>>(),
        [RECORD, REMOVE, RECORD, RECORD, RECORD]
    );
}

#[tokio::test]
async fn account_switch_leaves_the_original_pending_record_and_own_read_lock_blocks_a_race() {
    let (reply, send) = gate();
    let (_dir, api, session, client, epoch) = fixture([
        json!({"code":200,"weekData":rows()}).into(),
        reply,
        json!({"code":200,"weekData":rows()}).into(),
        json!({"code":200,"weekData":rows()}).into(),
    ]);
    let week = own_rank(&client, &epoch, Period::Week).await;
    let other = client.clone();
    let original = epoch.clone();
    let pending = tokio::spawn(async move {
        other
            .remove_profile_play_record(
                &original,
                Period::Week,
                "71",
                revision(&week),
                RankAction::Remove,
            )
            .await
    });
    api.wait_for_calls(2).await;
    assert!(matches!(
        client.profile_play_record(&epoch, None, Period::Week).await,
        Err(Error::MutationBusy)
    ));
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    let current = session.token().unwrap().generation.to_string();
    let same_song = own_rank(&client, &current, Period::Week).await;
    assert!(same_song["items"][0]["removalUncertain"].is_null());
    send.send(Ok(json!({"code":200}))).unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    let current = session.token().unwrap().generation.to_string();
    let original = own_rank(&client, &current, Period::Week).await;
    assert_eq!(original["items"][0]["removalUncertain"], true);
    assert_eq!(
        api.calls()
            .iter()
            .filter(|call| call.path == REMOVE)
            .count(),
        1
    );
}

#[tokio::test]
async fn canonical_business_rejection_requires_another_explicit_selection() {
    let (_dir, api, _, client, epoch) = fixture([
        json!({"code":200,"weekData":rows()}).into(),
        json!({"code":"403"}).into(),
        json!({"code":200}).into(),
    ]);
    let week = own_rank(&client, &epoch, Period::Week).await;
    let rejected = client
        .remove_profile_play_record(
            &epoch,
            Period::Week,
            "71",
            revision(&week),
            RankAction::Remove,
        )
        .await
        .err()
        .unwrap();
    assert!(rejected.is_service_code(403));
    assert_eq!(api.calls().len(), 2);
    let outcome = client
        .remove_profile_play_record(
            &epoch,
            Period::Week,
            "71",
            revision(&week),
            RankAction::Remove,
        )
        .await
        .unwrap();
    assert_eq!(to_value(outcome).unwrap()["value"], "removed");
    assert_eq!(api.calls().len(), 3);
}

const SOURCE: &str = "/api/song/enhance/player/url/v1";

fn denied_head() -> Value {
    json!({"song":{"id":72,"name":"无权首曲","dt":1000,"privilege":{"pl":0,"flag":0}}})
}
fn playable_rank() -> Value {
    json!({"code":200,"weekData":[denied_head(), rows()[0].clone()]})
}
fn sound() -> Value {
    json!({"code":200,"data":[{"id":71,"code":200,"url":"https://m.test.music.126.net/rank?auth=private",
        "md5":"fixture","level":"standard","expi":120}]})
}
async fn play_rank(
    client: &MusicClient,
    epoch: &str,
    uid: Option<&str>,
) -> cumulus_core::Result<PlayerView> {
    client
        .play_selection(
            epoch,
            PlaySelection::ProfileRankPlaylist {
                uid: uid.map(str::to_owned),
                quality: Quality::Standard,
            },
        )
        .await
}

#[tokio::test]
async fn profile_card_defaults_to_week_and_only_the_exact_source_toggles() {
    let (dir, api, _, client, epoch) = fixture([
        playable_rank().into(),
        sound().into(),
        detail(Some(false)).into(),
        detail(Some(true)).into(),
        playable_rank().into(),
    ]);
    let own = play_rank(&client, &epoch, None).await.unwrap();
    assert_eq!(own.profile_rank_playlist_uid.as_deref(), Some("9"));
    assert_eq!(own.state, PlaybackStatus::Playing);
    assert_eq!(own.track.as_ref().unwrap().id, "71");
    let paused = play_rank(&client, &epoch, Some("9")).await.unwrap();
    assert_eq!(paused.state, PlaybackStatus::Paused);
    assert_eq!(paused.play_id, own.play_id);
    assert_eq!(paused.queue_revision, own.queue_revision);
    let resumed = play_rank(&client, &epoch, None).await.unwrap();
    assert_eq!(resumed.state, PlaybackStatus::Playing);
    assert_eq!(resumed.play_id, own.play_id);
    assert_eq!(api.calls().len(), 2);
    assert!(matches!(
        play_rank(&client, &epoch, Some(GUEST)).await,
        Err(Error::State("listening rank playback is private"))
    ));
    let unchanged = client.player_snapshot(&epoch).await.unwrap();
    assert_eq!(unchanged.play_id, resumed.play_id);
    assert_eq!(unchanged.queue_revision, resumed.queue_revision);
    let guest = play_rank(&client, &epoch, Some(GUEST)).await.unwrap();
    assert_eq!(guest.profile_rank_playlist_uid.as_deref(), Some(GUEST));
    assert_eq!(
        guest.play_id, resumed.play_id,
        "same authorized song does not restart"
    );
    assert_ne!(
        guest.queue_revision, resumed.queue_revision,
        "guest rank replaces the source"
    );
    let queue = ProfileRegistry::open(dir.path(), ApiOrigin::Pc)
        .unwrap()
        .store()
        .get("9", "playback", "queue")
        .unwrap()
        .unwrap();
    assert_eq!(
        queue["items"][0]["source"],
        json!({
            "kind":"profile_rank_playlist","period":"week","scene":"unknown","href":"",
            "resourceType":"track","originalResourceType":"hostPlayRankPlaylist",
            "computeSourceResourceType":"hostPlayRankPlaylist","pldScene":"playlist",
            "sourceData":{"id":"hostPlayRankPlaylist","userId":GUEST,"name":"TA的听歌排行",
                "toDetailUrlPath":format!("/userDetail/user/{GUEST}/recentWeekRecord")}
        })
    );
    let calls = api.calls();
    assert_eq!(
        calls.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        [RECORD, SOURCE, DETAIL, DETAIL, RECORD]
    );
    assert_eq!(
        calls[0].payload,
        json!({"offset":0,"total":true,"limit":120,"uid":"9","type":1})
    );
    assert_eq!(
        calls[4].payload,
        json!({"offset":0,"total":true,"limit":120,"uid":GUEST,"type":1})
    );
}

#[tokio::test]
async fn a_delayed_profile_rank_cannot_override_a_newer_stop() {
    let (pending, reply) = gate();
    let (_dir, api, _, client, epoch) = fixture([
        playable_rank().into(),
        sound().into(),
        detail(Some(true)).into(),
        pending,
    ]);
    let own = play_rank(&client, &epoch, None).await.unwrap();
    let task = {
        let client = client.clone();
        let epoch = epoch.clone();
        tokio::spawn(async move { play_rank(&client, &epoch, Some(GUEST)).await })
    };
    api.wait_for_calls(4).await;
    let stopped = client
        .player_action(&epoch, PlayerAction::Stop)
        .await
        .unwrap();
    reply.send(Ok(playable_rank())).unwrap();
    assert!(matches!(task.await.unwrap(), Err(Error::StaleResponse)));
    let current = client.player_snapshot(&epoch).await.unwrap();
    assert_eq!(current.state, PlaybackStatus::Idle);
    assert_eq!(current.queue_revision, stopped.queue_revision);
    assert_eq!(
        current.profile_rank_playlist_uid,
        own.profile_rank_playlist_uid
    );
    assert_eq!(api.calls().len(), 4);
}
