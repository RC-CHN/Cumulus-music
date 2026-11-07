use super::{
    CARD, COOKIE, Error, Fixture, HttpResponse, PlayerAction, Quality, STARTUP, gate, json, reply,
};
use cumulus_core::{
    client::dto::{
        DownloadCommand, DownloadQuery, VoiceSongDownloadDecision as D, VoiceSongDownloadPlan,
        VoiceSongPurchasedDownloadCommand as C, VoiceSongPurchasedDownloadResult as R,
    },
    download::service::DownloadOptions,
    playlist::PlaylistPhase,
    protocol::pc_experiments::{ASSIGNMENT_PATH, assignment_arguments},
};

const LIKED: &str = "/api/song/like/get";
const DETAIL: &str = "/api/v6/playlist/detail";
const LIST: &str = "/api/user/playlist";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const CREATE: &str = "/api/playlist/create";
const AUTH: &str = "/api/song/enhance/download/url/v1";
const ALL_FLAGS: u32 = (1 << 12) | (1 << 14) | (1 << 16) | (1 << 17) | (1 << 18) | (1 << 21);

fn track(id: u64, mask: u32, maxbr: Option<u64>, flags: u32) -> serde_json::Value {
    let mut privilege = json!({"id":id,"fee":4,"payed":mask,"st":0,"dl":320000,
        "pl":320000,"flag":flags,"maxBrLevel":"vivid","DO_NOT_EXPORT":"raw"});
    if let Some(maxbr) = maxbr {
        privilege["maxbr"] = json!(maxbr);
    }
    json!({"id":id,"name":"合成已购歌曲","album":{},"privilege":privilege})
}

fn card() -> HttpResponse {
    json!({"code":200,"tracks":[
        track(301, 2, Some(998001), ALL_FLAGS),
        track(302, 4, Some(320000), 0),
        track(303, 6, Some(128000), 0),
        track(304, 1, Some(999000), ALL_FLAGS),
        track(305, 32, Some(999000), ALL_FLAGS),
        track(306, 2, None, 1 << 14),
        track(307, 2, None, 1 << 12),
        track(308, 0, Some(999000), ALL_FLAGS),
        override_field(track(309, 2, Some(999000), ALL_FLAGS), "st", json!(-100)),
        override_field(track(310, 2, Some(999000), ALL_FLAGS), "dl", json!(0)),
        override_field(track(311, 2, Some(999000), ALL_FLAGS), "bd", json!(1)),
        override_field(track(312, 2, Some(999000), ALL_FLAGS), "cs", json!(true)),
        override_field(track(313, 2, Some(999000), ALL_FLAGS), "payed", json!(true)),
        override_field(track(314, 2, Some(999000), ALL_FLAGS), "id", json!(315)),
        override_field(track(315, 2, Some(999000), ALL_FLAGS), "subp", json!(-1))
    ]})
    .into()
}

fn override_field(
    mut track: serde_json::Value,
    key: &str,
    value: serde_json::Value,
) -> serde_json::Value {
    track["privilege"][key] = value;
    track
}

fn assignment(a: &str, b: &str) -> HttpResponse {
    json!({"code":200,"data":[
        {"expName":"PH-PC-hyyinzhi","expGroupName":a},
        {"expName":"PH-PC-zhenyingn","expGroupName":b}
    ]})
    .into()
}

fn detail(id: u64, present: bool) -> serde_json::Value {
    json!({"code":200,"playlist":{"id":id,"name":"合成私密歌单","userId":25,"privacy":10,
        "tags":[],"trackIds":if present {vec![json!({"id":301})]} else {vec![]}}})
}

async fn fixture(replies: impl IntoIterator<Item = HttpResponse>) -> (Fixture, String) {
    let f = Fixture::new(&[COOKIE], [card()].into_iter().chain(replies)).await;
    super::picker::config(&f);
    let receipt = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap()
        .receipt
        .unwrap();
    (f, receipt)
}

async fn command(f: &Fixture, receipt: &str, index: usize, command: C) -> cumulus_core::Result<R> {
    f.client
        .player_voice_song_purchased_download(&f.epoch, &f.play_id, receipt, index, command)
        .await
}

async fn plan(f: &Fixture, receipt: &str, index: usize) -> VoiceSongDownloadPlan {
    let R::Plan { plan } = command(f, receipt, index, C::Prepare).await.unwrap() else {
        panic!("expected native purchased plan");
    };
    plan
}

async fn select(f: &Fixture, receipt: &str, quality: Quality, decision: D) -> String {
    let R::Selection {
        quality: selected,
        decision: actual,
        selection,
    } = command(f, receipt, 0, C::Select { quality }).await.unwrap()
    else {
        panic!("expected explicit selection");
    };
    assert_eq!(selected, quality);
    assert_eq!(actual, decision);
    selection.unwrap()
}

fn start(selection: &str, continuation: Option<String>, quality: Quality) -> C {
    C::Start {
        selection: selection.into(),
        continuation,
        options: DownloadOptions {
            quality,
            ..Default::default()
        },
    }
}

fn paths(f: &Fixture, expected: &[&str]) {
    assert_eq!(
        f.http
            .calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        STARTUP
            .into_iter()
            .chain([CARD])
            .chain(expected.iter().copied())
            .collect::<Vec<_>>()
    );
}

fn signed_payload(actual: &serde_json::Value, expected: serde_json::Value) {
    let mut actual = actual.clone();
    let header = actual.as_object_mut().unwrap().remove("header").unwrap();
    let header: serde_json::Value = serde_json::from_str(header.as_str().unwrap()).unwrap();
    assert_eq!(header["os"], "pc");
    assert_eq!(header["appver"], "3.1.42.205594");
    assert_eq!(header["clientSign"], "synthetic-signature");
    assert_eq!(actual, expected);
}

fn no_tasks(f: &Fixture) {
    assert!(
        f.store
            .values("25", "download_tasks_v1")
            .unwrap()
            .is_empty()
    );
}

async fn configure(f: &Fixture) {
    f.client
        .download_command(
            &f.epoch,
            DownloadCommand::Configure {
                root: f.root.path().join("output").to_str().unwrap().into(),
                path_type: 0,
                file_type: 0,
            },
        )
        .await
        .unwrap();
}

async fn assert_exact_task(f: &Fixture, id: &str, quality: Quality) {
    let task = f.store.get("25", "download_tasks_v1", id).unwrap().unwrap();
    assert_eq!(task["song_id"], "301");
    assert_eq!(task["origin"]["kind"], "song");
    assert_eq!(task["options"]["quality"], quality.as_str());
    let default = f
        .client
        .downloads(&f.epoch, DownloadQuery::default())
        .await
        .unwrap();
    assert_eq!(default.options.quality, Quality::Standard);
    signed_payload(
        &f.http.calls().last().unwrap().payload,
        json!({"id":"301","level":quality.as_str(),"immerseType":"c51"}),
    );
}

#[tokio::test]
async fn exact_offers_follow_maxbr_ceiling_and_flag_order_only_signed_vivid_group() {
    use Quality::*;
    let (f, receipt) = fixture([
        assignment("t1", "t1"),
        json!({"code":200,"ids":[301]}).into(),
        assignment("c", "t1"),
    ])
    .await;
    let visible = vec![Sky, JyMaster, JyEffect, HiRes, Lossless, ExHigh, Standard];
    let initial = plan(&f, &receipt, 0).await;
    assert_eq!(initial.decision, D::PurchasedQualityRequired);
    assert_eq!(initial.purchased_qualities, visible);
    assert_eq!(
        plan(&f, &receipt, 1).await.purchased_qualities,
        [ExHigh, Standard]
    );
    assert_eq!(plan(&f, &receipt, 2).await.purchased_qualities, [Standard]);
    assert_eq!(plan(&f, &receipt, 6).await.purchased_qualities, [HiRes]);
    for (index, state) in [
        (3, D::ExtendedRightsUnsupported),
        (4, D::ExtendedRightsUnsupported),
        (5, D::Unavailable),
        (7, D::ExtendedRightsUnsupported),
        (8, D::ExtendedRightsUnsupported),
        (9, D::ExtendedRightsUnsupported),
        (10, D::ExtendedRightsUnsupported),
        (11, D::CloudUnsupported),
        (12, D::ExtendedRightsUnsupported),
        (13, D::Unavailable),
    ] {
        let no = plan(&f, &receipt, index).await;
        assert_eq!(no.decision, state, "index {index}");
        assert!(no.purchased_qualities.is_empty());
    }
    assert!(
        !serde_json::to_string(&initial)
            .unwrap()
            .contains("DO_NOT_EXPORT")
    );
    for quality in [Vivid, Dolby, Higher, Aac64] {
        assert!(matches!(
            command(&f, &receipt, 0, C::Select { quality }).await,
            Err(Error::InvalidInput(_))
        ));
    }
    paths(&f, &[]);
    f.client
        .official_pc_ordinary_skip_assignment(&f.epoch)
        .await
        .unwrap();
    signed_payload(
        &f.http.calls().last().unwrap().payload,
        assignment_arguments(),
    );
    let mut vivid = vec![Vivid];
    vivid.extend(visible.clone());
    assert_eq!(plan(&f, &receipt, 0).await.purchased_qualities, vivid);
    let R::Selection {
        decision: D::MembershipUnknown,
        selection: None,
        ..
    } = command(&f, &receipt, 0, C::Select { quality: Vivid })
        .await
        .unwrap()
    else {
        panic!("membership may only be checked after choosing quality");
    };
    f.client.liked_ids().await.unwrap();
    let chosen = select(&f, &receipt, Vivid, D::Ready).await;
    f.client.refresh_pc_presentation().await.unwrap();
    assert_eq!(plan(&f, &receipt, 0).await.purchased_qualities, visible);
    assert!(matches!(
        command(&f, &receipt, 0, start(&chosen, None, Vivid)).await,
        Err(Error::StaleResponse)
    ));
    paths(&f, &[ASSIGNMENT_PATH, LIKED, ASSIGNMENT_PATH]);
    no_tasks(&f);
}

#[tokio::test]
async fn missing_purchased_download_folder_returns_settings_branch_without_creating_task_or_folder()
{
    use Quality::ExHigh;

    let (f, receipt) = fixture([json!({"code":200,"ids":[301]}).into()]).await;
    f.client.liked_ids().await.unwrap();
    let choice = select(&f, &receipt, ExHigh, D::Ready).await;
    assert!(matches!(
        command(&f, &receipt, 0, start(&choice, None, ExHigh))
            .await
            .unwrap(),
        R::DirectoryRequired
    ));
    configure(&f).await;
    let folder = f.root.path().join("output");
    assert!(folder.is_dir());
    std::fs::remove_dir(&folder).unwrap();
    assert!(matches!(
        command(&f, &receipt, 0, start(&choice, None, ExHigh))
            .await
            .unwrap(),
        R::DirectoryRequired
    ));
    assert!(!folder.exists());
    no_tasks(&f);
    paths(&f, &[LIKED]);
}

#[tokio::test]
async fn no_old_command_or_forged_selection_can_start_add_or_create_and_selected_quality_is_frozen()
{
    let (f, receipt) = fixture([
        json!({"code":200,"ids":[301]}).into(),
        json!({"code":200,"data":{"id":301,"code":403}}).into(),
    ])
    .await;
    f.client.liked_ids().await.unwrap();
    configure(&f).await;
    assert_eq!(
        f.client
            .player_voice_song_download_prepare(&f.epoch, &f.play_id, &receipt, 0)
            .await
            .unwrap()
            .decision,
        D::ExtendedRightsUnsupported
    );
    assert!(
        f.client
            .player_voice_song_download_start(
                &f.epoch,
                &f.play_id,
                &receipt,
                0,
                DownloadOptions::default()
            )
            .await
            .is_err()
    );
    assert!(
        f.client
            .player_voice_song_download_choose_playlist(&f.epoch, &f.play_id, &receipt, 0, "81")
            .await
            .is_err()
    );
    assert!(
        f.client
            .player_voice_song_download_create_playlist(
                &f.epoch, &f.play_id, &receipt, 0, "合成", true
            )
            .await
            .is_err()
    );
    let fake = "0".repeat(32);
    for action in [
        start(&fake, None, Quality::ExHigh),
        C::ChoosePlaylist {
            selection: fake.clone(),
            playlist_id: "81".into(),
        },
        C::CreatePlaylist {
            selection: fake,
            name: "合成".into(),
            private: true,
        },
    ] {
        assert!(matches!(
            command(&f, &receipt, 0, action).await,
            Err(Error::StaleResponse)
        ));
    }
    let chosen = select(&f, &receipt, Quality::ExHigh, D::Ready).await;
    assert!(
        f.client
            .player_voice_song_download_start(
                &f.epoch,
                &f.play_id,
                &receipt,
                0,
                DownloadOptions::default()
            )
            .await
            .is_err()
    );
    assert!(matches!(
        command(&f, &receipt, 0, start(&chosen, None, Quality::Standard)).await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        command(&f, &receipt, 1, start(&chosen, None, Quality::ExHigh)).await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        command(
            &f,
            &receipt,
            0,
            C::Cancel {
                selection: chosen.clone()
            }
        )
        .await,
        Ok(R::Cancelled)
    ));
    assert!(matches!(
        command(&f, &receipt, 0, start(&chosen, None, Quality::ExHigh)).await,
        Err(Error::StaleResponse)
    ));
    paths(&f, &[LIKED]);
    no_tasks(&f);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
    let chosen = select(&f, &receipt, Quality::ExHigh, D::Ready).await;
    let before = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    let R::Started { result } = command(&f, &receipt, 0, start(&chosen, None, Quality::ExHigh))
        .await
        .unwrap()
    else {
        panic!("expected one admitted task");
    };
    assert_eq!(result.accepted.len(), 1);
    assert!(matches!(
        command(&f, &receipt, 0, start(&chosen, None, Quality::ExHigh)).await,
        Err(Error::StaleResponse)
    ));
    f.http.wait(STARTUP.len() + 3).await;
    paths(&f, &[LIKED, AUTH]);
    assert_exact_task(&f, &result.accepted[0], Quality::ExHigh).await;
    assert_eq!(
        before,
        serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap()
    );
}

#[tokio::test]
async fn explicit_quality_precedes_existing_destination_and_survives_known_add_false() {
    let (f, receipt) = fixture([
        json!({"code":200,"ids":[]}).into(),
        detail(81, false).into(),
        json!({"code":403}).into(),
        detail(81, false).into(),
        json!({"code":200,"data":{"id":301,"code":403}}).into(),
    ])
    .await;
    f.client.liked_ids().await.unwrap();
    configure(&f).await;
    let selected = select(&f, &receipt, Quality::Lossless, D::PlaylistRequired).await;
    assert!(matches!(
        command(&f, &receipt, 0, start(&selected, None, Quality::Lossless)).await,
        Err(Error::State(_))
    ));
    paths(&f, &[LIKED]);
    let R::Picker { picker } = command(
        &f,
        &receipt,
        0,
        C::ChoosePlaylist {
            selection: selected.clone(),
            playlist_id: "81".into(),
        },
    )
    .await
    .unwrap() else {
        panic!("expected exact destination result");
    };
    assert_eq!(picker.phase, PlaylistPhase::Rejected);
    assert!(picker.add_failed);
    let continuation = picker.continuation.unwrap();
    assert!(
        f.client
            .player_voice_song_download_start_from_playlist(
                &f.epoch,
                &f.play_id,
                &receipt,
                0,
                &continuation,
                DownloadOptions::default()
            )
            .await
            .is_err()
    );
    assert!(matches!(
        command(
            &f,
            &receipt,
            0,
            start(&selected, Some(continuation.clone()), Quality::Standard)
        )
        .await,
        Err(Error::InvalidInput(_))
    ));
    no_tasks(&f);
    paths(&f, &[LIKED, DETAIL, ADD, DETAIL]);
    let R::Started { result } = command(
        &f,
        &receipt,
        0,
        start(&selected, Some(continuation), Quality::Lossless),
    )
    .await
    .unwrap() else {
        panic!("known add false still permits exact independent download");
    };
    assert_eq!(result.accepted.len(), 1);
    f.http.wait(STARTUP.len() + 6).await;
    paths(&f, &[LIKED, DETAIL, ADD, DETAIL, AUTH]);
    let add = &f.http.calls()[STARTUP.len() + 3].payload;
    signed_payload(add, json!({"pid":"81","trackIds":r#"["301"]"#,"op":"add"}));
    assert_exact_task(&f, &result.accepted[0], Quality::Lossless).await;
}

#[tokio::test]
async fn private_create_does_not_override_quality_and_known_add_false_never_unlocks() {
    let (f, receipt) = fixture([
        json!({"code":200,"ids":[]}).into(),
        json!({"code":200,"playlist":[],"more":false}).into(),
        json!({"code":200,"id":84}).into(),
        json!({"code":200,"playlist":[{"id":84,"name":"合成私密歌单","userId":25,"privacy":10}],"more":false}).into(),
        detail(84, false).into(), json!({"code":403}).into(), detail(84, false).into(),
    ]).await;
    f.client.liked_ids().await.unwrap();
    configure(&f).await;
    let selection = select(&f, &receipt, Quality::HiRes, D::PlaylistRequired).await;
    let R::Creation { creation } = command(
        &f,
        &receipt,
        0,
        C::CreatePlaylist {
            selection: selection.clone(),
            name: "合成私密歌单".into(),
            private: true,
        },
    )
    .await
    .unwrap() else {
        panic!("expected explicit guarded create");
    };
    assert_eq!(creation.create_phase, PlaylistPhase::Confirmed);
    assert_eq!(creation.playlist_id.as_deref(), Some("84"));
    let add = creation.add.unwrap();
    assert_eq!(add.phase, PlaylistPhase::Rejected);
    assert!(add.continuation.is_none());
    assert!(matches!(
        command(&f, &receipt, 0, start(&selection, None, Quality::HiRes)).await,
        Err(Error::State(_))
    ));
    paths(&f, &[LIKED, LIST, CREATE, LIST, DETAIL, ADD, DETAIL]);
    no_tasks(&f);
}

#[tokio::test]
async fn retiring_quality_during_destination_preflight_and_replacing_play_cannot_dispatch() {
    let (held, release) = gate();
    let (f, receipt) = fixture([json!({"code":200,"ids":[]}).into(), held]).await;
    f.client.liked_ids().await.unwrap();
    let selection = select(&f, &receipt, Quality::Standard, D::PlaylistRequired).await;
    let (client, epoch, play_id, card, chosen) = (
        f.client.clone(),
        f.epoch.clone(),
        f.play_id.clone(),
        receipt.clone(),
        selection.clone(),
    );
    let pending = tokio::spawn(async move {
        client
            .player_voice_song_purchased_download(
                &epoch,
                &play_id,
                &card,
                0,
                C::ChoosePlaylist {
                    selection: chosen,
                    playlist_id: "81".into(),
                },
            )
            .await
    });
    f.http.wait(STARTUP.len() + 3).await;
    assert!(matches!(
        command(
            &f,
            &receipt,
            0,
            C::Cancel {
                selection: selection.clone()
            }
        )
        .await,
        Ok(R::Cancelled)
    ));
    assert!(release.send(Ok(reply(detail(81, false)))).is_ok());
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    paths(&f, &[LIKED, DETAIL]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
    let another = select(&f, &receipt, Quality::Standard, D::PlaylistRequired).await;
    assert_ne!(selection, another);
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(matches!(
        command(&f, &receipt, 0, start(&another, None, Quality::Standard)).await,
        Err(Error::StaleResponse)
    ));
    paths(&f, &[LIKED, DETAIL]);
    no_tasks(&f);
}

#[tokio::test]
async fn signed_cookie_rotation_during_destination_read_revokes_selected_quality_in_same_account() {
    let (held, release) = gate();
    let (f, receipt) = fixture([json!({"code":200,"ids":[]}).into(), held]).await;
    f.client.liked_ids().await.unwrap();
    let selection = select(&f, &receipt, Quality::ExHigh, D::PlaylistRequired).await;
    let (client, epoch, play_id, card, chosen) = (
        f.client.clone(),
        f.epoch.clone(),
        f.play_id.clone(),
        receipt.clone(),
        selection.clone(),
    );
    let pending = tokio::spawn(async move {
        client
            .player_voice_song_purchased_download(
                &epoch,
                &play_id,
                &card,
                0,
                C::ChoosePlaylist {
                    selection: chosen,
                    playlist_id: "81".into(),
                },
            )
            .await
    });
    f.http.wait(STARTUP.len() + 3).await;
    let mut response = reply(detail(81, false));
    response.set_cookies = vec!["MUSIC_U=purchased-rotation; Path=/; Secure".into()];
    assert!(release.send(Ok(response)).is_ok());
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(f.session.token().unwrap().generation.to_string(), f.epoch);
    assert!(matches!(
        command(&f, &receipt, 0, start(&selection, None, Quality::ExHigh)).await,
        Err(Error::StaleResponse)
    ));
    paths(&f, &[LIKED, DETAIL]);
    assert!(
        f.store
            .values("25", "playlist-mutations")
            .unwrap()
            .is_empty()
    );
    no_tasks(&f);
}

#[tokio::test]
async fn changing_account_after_selection_cannot_adopt_or_reuse_its_purchase() {
    let (f, receipt) = fixture([json!({"code":200,"ids":[301]}).into()]).await;
    f.client.liked_ids().await.unwrap();
    let selection = select(&f, &receipt, Quality::ExHigh, D::Ready).await;
    f.session
        .adopt(
            f.native.clone(),
            json!({"userId":26}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(matches!(
        command(&f, &receipt, 0, start(&selection, None, Quality::ExHigh)).await,
        Err(Error::StaleResponse)
    ));
    let epoch = f.session.token().unwrap().generation.to_string();
    assert_ne!(epoch, f.epoch);
    assert!(matches!(
        f.client
            .player_voice_song_purchased_download(
                &epoch,
                &f.play_id,
                &receipt,
                0,
                start(&selection, None, Quality::ExHigh)
            )
            .await,
        Err(Error::StaleResponse)
    ));
    paths(&f, &[LIKED]);
    no_tasks(&f);
    assert!(
        f.store
            .values("26", "download_tasks_v1")
            .unwrap()
            .is_empty()
    );
}
