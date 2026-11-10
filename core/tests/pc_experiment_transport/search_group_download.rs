//! Sealed PC cookies and real download task admission from native search rows.
use super::{HttpResponse, fixture_registry, vinyl_http, vinyl_http_with_credentials};
use cumulus_core::{
    Error,
    account::AccountSession,
    client::{
        MusicClient,
        dto::{
            DownloadCommand, PlaylistCollectSort, SearchCategory, SearchGroupDownloadCommand as C,
            SearchGroupDownloadDecision as D, SearchGroupDownloadReply as R, SearchResultQuery,
            SongDownloadState,
        },
    },
    download::service::{DownloadAction, DownloadOptions, DownloadService, DownloadStatus},
    membership::Quality,
    protocol::{http::ApiOrigin, mconfig::Snapshot},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};

const COOKIE: &str = "MUSIC_U=synthetic-search-group-download; Path=/; Secure";
const AUTH: &str = "/api/song/enhance/download/url/v1";
const LIKED: &str = "/api/song/like/get";

fn search_page() -> HttpResponse {
    search_page_with_destination(false)
}

fn search_page_with_destination(destination: bool) -> HttpResponse {
    let songs: Vec<_> = (41..=45 + u64::from(destination))
        .map(|id| {
            let mut row = json!({"baseInfo":{"simpleSongData":{"id":id,
            "name":format!("合成搜索下载歌曲 {id}"),"dt":120000,
            "ar":[{"id":101,"name":"合成歌手"}],"al":{"id":123,"name":"合成专辑"},
            "privilege":{"id":id,"fee":0,"st":if id==42 {-100}else{0},
              "pl":if id==41 || id==42 {0}else{320000},
              "dl":if id==44 {0}else{320000},"cs":id==45}
            }}});
            let privilege = &mut row["baseInfo"]["simpleSongData"]["privilege"];
            if destination && id == 43 {
                privilege["subp"] = json!(0);
            } else if id == 46 {
                privilege["subp"] = json!(-1);
            }
            row
        })
        .collect();
    json!({"code":200,"data":{"totalCount":songs.len(),"resources":songs,"hasMore":false}}).into()
}

fn client(
    root: &Path,
    api: Arc<dyn cumulus_core::protocol::api::Api>,
) -> (MusicClient, Arc<AccountSession>, String) {
    let registry = fixture_registry(&root.join("host"));
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":71}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    (
        MusicClient::with_session(session.clone(), registry),
        session,
        epoch,
    )
}
async fn search(client: &MusicClient, epoch: &str) -> Value {
    let value = client
        .search_results(
            epoch,
            SearchResultQuery {
                keyword: "合成".into(),
                category: SearchCategory::Songs,
                offset: 0,
                limit: 6,
                correct: true,
                channel: None,
            },
        )
        .await
        .unwrap()
        .value;
    serde_json::to_value(&value).unwrap()["sections"][0]["items"].clone()
}
fn target(rows: &Value, index: usize, receipt: &str) -> Value {
    json!({"trackId":rows[index]["track"]["id"],"receipt":rows[index][receipt]})
}
fn prepare(rows: &Value, anchor: usize, selection: &[usize]) -> C {
    let download_row = |index| {
        json!({
            "trackId": rows[index]["track"]["id"],
            "receipt": rows[index]["groupDownload"]["receipt"],
        })
    };
    serde_json::from_value(json!({"kind":"prepare","anchor":download_row(anchor),
        "rows":selection.iter().map(|&i| download_row(i)).collect::<Vec<_>>() }))
    .unwrap()
}
async fn directory(client: &MusicClient, epoch: &str, root: &Path) {
    client
        .download_command(
            epoch,
            DownloadCommand::Configure {
                root: root.join("output").to_str().unwrap().into(),
                path_type: 0,
                file_type: 0,
            },
        )
        .await
        .unwrap();
}
async fn decision(client: &MusicClient, epoch: &str, action: C, expected: D) -> Option<String> {
    let R::Prepared {
        decision, receipt, ..
    } = client.search_group_download(epoch, action).await.unwrap()
    else {
        panic!("missing native preparation")
    };
    assert!(decision == expected, "native decision differed");
    receipt
}

#[tokio::test]
async fn search_group_download_uses_independent_rights_clicked_like_and_saved_order() {
    let root = tempfile::tempdir().unwrap();
    let denial = json!({"code":200,"data":{"code":403}});
    let (api, http) = vinyl_http(
        root.path(),
        &[COOKIE],
        [
            search_page(),
            json!({"code":200,"ids":[42]}).into(),
            denial.clone().into(),
            denial.into(),
        ],
    );
    let (client, session, epoch) = client(root.path(), api);
    let rows = search(&client, &epoch).await;
    assert!(
        rows[0]["groupNextRight"].is_null(),
        "play bitrate is not download authority"
    );
    assert_eq!(rows[0]["groupDownload"]["right"], "ordinary");
    assert_eq!(rows[1]["groupDownload"]["right"], "copyright_blocked");
    assert_eq!(rows[2]["groupDownload"]["right"], "ordinary");
    assert!(rows[3]["groupDownload"].is_null());
    assert!(rows[4]["groupDownload"].is_null());
    let play = target(&rows, 0, "queueDropReceipt");
    assert_ne!(play["receipt"], rows[0]["groupDownload"]["receipt"]);
    let mut forged = json!({"kind":"prepare","anchor":play, "rows":[play, target(&rows, 2, "queueDropReceipt")]});
    let parsed = serde_json::from_value(forged.clone()).unwrap();
    assert!(matches!(
        client.search_group_download(&epoch, parsed).await,
        Err(Error::StaleResponse)
    ));
    forged["quality"] = json!("lossless");
    assert!(serde_json::from_value::<C>(forged).is_err());

    client.liked_ids().await.unwrap();
    directory(&client, &epoch, root.path()).await;
    let saved = client.download_quality_preferences(&epoch).unwrap();
    let mut options = saved.options;
    options.quality = Quality::ExHigh;
    options.cover = false;
    client
        .download_command(
            &epoch,
            DownloadCommand::Preferences {
                revision: saved.revision,
                options,
            },
        )
        .await
        .unwrap();
    let reply = client
        .search_group_download(&epoch, prepare(&rows, 1, &[2, 1, 0]))
        .await
        .unwrap();
    let R::Prepared {
        decision: D::Ready,
        receipt: Some(receipt),
        eligible_count: 2,
        omitted_count: 1,
        ..
    } = reply
    else {
        panic!("only the two ordinary songs survive");
    };
    assert_eq!(
        http.calls().len(),
        2,
        "preparation uses no writes or media calls"
    );
    assert!(matches!(
        client
            .search_group_download(
                &epoch,
                C::Status {
                    receipt: receipt.clone()
                }
            )
            .await
            .unwrap(),
        R::NotStarted
    ));
    let R::Accepted { result } = client
        .search_group_download(
            &epoch,
            C::Start {
                receipt: receipt.clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("not accepted")
    };
    assert_eq!(result.operation_id.as_deref(), Some(receipt.as_str()));
    let tasks = DownloadService::new(session).unwrap().list().unwrap();
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.song_id.as_str())
            .collect::<Vec<_>>(),
        ["43", "41"]
    );
    assert!(
        tasks
            .iter()
            .all(|task| task.options.quality == Quality::ExHigh && !task.options.cover)
    );
    assert_eq!(
        tasks.iter().map(|task| task.id.clone()).collect::<Vec<_>>(),
        result.accepted
    );
    for command in [
        C::Status {
            receipt: receipt.clone(),
        },
        C::Start {
            receipt: receipt.clone(),
        },
    ] {
        let R::Accepted { result: again } =
            client.search_group_download(&epoch, command).await.unwrap()
        else {
            panic!("missing readback")
        };
        assert_eq!(again.accepted, result.accepted);
    }
    http.wait(4).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if client.song_download_state(&epoch, "43").await.unwrap() == SongDownloadState::Failed
                && client.song_download_state(&epoch, "41").await.unwrap()
                    == SongDownloadState::Failed
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let calls = http.calls();
    assert_eq!(
        calls.len(),
        4,
        "no destination/like/write or duplicate download URL"
    );
    assert_eq!(calls[1].path, LIKED);
    let mut ids: Vec<_> = calls[2..]
        .iter()
        .map(|call| {
            assert_eq!(call.path, AUTH);
            assert_eq!(call.payload["level"], "exhigh");
            assert_eq!(call.payload["immerseType"], "c51");
            assert!(call.payload.get("trialMode").is_none());
            call.payload["id"].as_str().unwrap().to_owned()
        })
        .collect();
    ids.sort();
    assert_eq!(ids, ["41", "43"]);
}

#[tokio::test]
async fn search_group_download_requires_known_anchor_directory_and_live_credentials() {
    let root = tempfile::tempdir().unwrap();
    let (api, http, credentials) = vinyl_http_with_credentials(
        root.path(),
        &[COOKIE],
        [
            search_page(),
            json!({"code":200,"ids":[42]}).into(),
            search_page(),
        ],
    );
    let (client, session, epoch) = client(root.path(), api);
    let old = search(&client, &epoch).await;
    assert!(
        decision(
            &client,
            &epoch,
            prepare(&old, 1, &[1, 0]),
            D::MembershipUnknown
        )
        .await
        .is_none()
    );
    client.liked_ids().await.unwrap();
    let playlist = decision(
        &client,
        &epoch,
        prepare(&old, 0, &[0, 1]),
        D::PlaylistRequired,
    )
    .await
    .unwrap();
    assert!(matches!(
        client
            .search_group_download(&epoch, C::Start { receipt: playlist })
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(
        decision(
            &client,
            &epoch,
            prepare(&old, 1, &[1, 0]),
            D::DirectoryRequired
        )
        .await
        .is_none()
    );
    directory(&client, &epoch, root.path()).await;
    let receipt = decision(&client, &epoch, prepare(&old, 1, &[1, 0]), D::Ready)
        .await
        .unwrap();
    assert!(matches!(
        client
            .search_group_download(
                &epoch,
                C::Cancel {
                    receipt: receipt.clone()
                }
            )
            .await
            .unwrap(),
        R::Cancelled
    ));
    assert!(matches!(
        client
            .search_group_download(&epoch, C::Start { receipt })
            .await,
        Err(Error::StaleResponse)
    ));
    let live = search(&client, &epoch).await;
    assert!(matches!(
        client
            .search_group_download(&epoch, prepare(&old, 1, &[1, 0]))
            .await,
        Err(Error::StaleResponse)
    ));
    let receipt = decision(&client, &epoch, prepare(&live, 1, &[1, 0]), D::Ready)
        .await
        .unwrap();
    let target = ApiOrigin::Pc.url();
    let previous = credentials.request_credentials(&target).unwrap();
    credentials
        .record_response(
            &target,
            &["MUSIC_U=rotated-synthetic-search; Path=/; Secure".into()],
            None,
            &previous.revision,
        )
        .unwrap();
    assert!(matches!(
        client
            .search_group_download(&epoch, C::Start { receipt })
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(
        DownloadService::new(session)
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
    assert_eq!(http.calls().len(), 3);
}

#[tokio::test]
async fn search_group_download_owned_playlist_uses_second_filter_and_read_only_recovery() {
    owned_playlist(false).await;
}

#[tokio::test]
async fn search_group_download_owned_playlist_mixes_existing_task_in_reverse_order() {
    owned_playlist(true).await;
}

async fn owned_playlist(mixed: bool) {
    const LIST: &str = "/api/user/playlist/v2";
    const STAR_LIST: &str = "/api/user/playlist";
    const DETAIL: &str = "/api/v6/playlist/detail";
    const ADD: &str = "/api/v1/playlist/manipulate/tracks";
    let summary = |id, owner, special| {
        json!({
            "id":id,"userId":owner,"name":"合成下载目的地","specialType":special,"privacy":10
        })
    };
    let detail = |ids: &[u64]| {
        let mut row = summary(81, 71, 0);
        row["tags"] = json!([]);
        row["trackIds"] = json!(ids.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
        json!({"code":200,"playlist":row}).into()
    };
    let denied = || json!({"code":200,"data":{"code":403}}).into();
    let root = tempfile::tempdir().unwrap();
    let (api, http) = vinyl_http(root.path(), &[COOKIE], [
        search_page_with_destination(true),
        json!({"code":200,"ids":[43]}).into(),
        json!({"code":200,"playlist":[summary(81,71,0),summary(82,76,0),summary(83,71,5)],"more":false}).into(),
        json!({"code":200,"playlist":[],"more":false}).into(),
        detail(&[]),
        HttpResponse::Ready(Err(Error::Transport("synthetic lost search add".into()))),
        detail(&[]),
        detail(&[43,41]),
        denied(), denied(), denied(),
    ]);
    StateStore::open(root.path().join("config"))
        .unwrap()
        .put(
            "transport",
            "pc-mconfig-v1",
            "pc",
            &serde_json::to_value(Snapshot {
                origin: ApiOrigin::Pc,
                version: 1,
                bucket: String::new(),
                generation: 1,
                source_url: Some("https://p1.music.126.net/obj/fixture.data".into()),
                data: Some(json!({})),
            })
            .unwrap(),
        )
        .unwrap();
    let (client, session, epoch) = client(root.path(), api);
    let rows = search(&client, &epoch).await;
    assert_eq!(rows[5]["track"]["id"], "46");
    assert_eq!(rows[5]["groupDownload"]["right"], "ordinary");
    assert_eq!(rows[5]["groupDownload"].as_object().unwrap().len(), 2);
    client.liked_ids().await.unwrap();
    directory(&client, &epoch, root.path()).await;
    let service = DownloadService::new(session.clone()).unwrap();
    let old = mixed.then(|| {
        let task = service
            .create_default(
                &json!("46"),
                DownloadOptions {
                    quality: Quality::Lossless,
                    cover: false,
                    ..Default::default()
                },
            )
            .unwrap();
        service.request(&task.id, DownloadAction::Pause).unwrap()
    });
    let receipt = decision(
        &client,
        &epoch,
        prepare(&rows, 1, &[2, 5, 1, 0]),
        D::PlaylistRequired,
    )
    .await
    .unwrap();
    let command = |kind, extra: Value| {
        let mut dto = json!({"kind":kind,"receipt":receipt});
        dto.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value::<C>(dto).unwrap()
    };
    let choose = |id| command("choose", json!({"playlistId":id}));
    assert!(matches!(
        client.search_group_download(&epoch, choose("81")).await,
        Err(Error::StaleResponse)
    ));
    let R::Destinations {
        page,
        system_liked_receipt,
        ..
    } = client
        .search_group_download(
            &epoch,
            C::Destinations {
                receipt: receipt.clone(),
                sort: PlaylistCollectSort::Default,
                page: 0,
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected verified destinations")
    };
    assert!(system_liked_receipt.is_none());
    assert_eq!(
        page.value
            .playlists
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["81"]
    );
    for id in ["82", "83"] {
        assert!(matches!(
            client.search_group_download(&epoch, choose(id)).await,
            Err(Error::StaleResponse)
        ));
    }
    assert_eq!(
        http.calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>()[1..],
        [LIKED, LIST, STAR_LIST]
    );
    for _ in 0..2 {
        let reply = client
            .search_group_download(&epoch, choose("81"))
            .await
            .unwrap();
        let value = serde_json::to_value(reply).unwrap();
        assert_eq!(value["result"]["phase"], "uncertain");
        assert!(value["result"]["continuation"].is_null());
    }
    let before_readback = service.list().unwrap();
    assert_eq!(before_readback.len(), usize::from(mixed));
    if let Some(old) = &old {
        assert_eq!(before_readback[0].id, old.id);
        assert_eq!(before_readback[0].status, DownloadStatus::Paused);
    }
    assert_eq!(
        http.calls().len(),
        7,
        "one write; reselect never repeats it"
    );
    let reply = client
        .search_group_download(&epoch, command("reconcile", json!({})))
        .await
        .unwrap();
    let value = serde_json::to_value(reply).unwrap();
    assert_eq!(value["result"]["phase"], "confirmed");
    let permit = value["result"]["continuation"].as_str().unwrap();
    assert!(matches!(
        client
            .search_group_download(
                &epoch,
                command("start_from_playlist", json!({"continuation":"forged"}))
            )
            .await,
        Err(Error::StaleResponse)
    ));
    let result = client
        .search_group_download(
            &epoch,
            command("start_from_playlist", json!({"continuation":permit})),
        )
        .await
        .unwrap();
    let R::Accepted { result } = result else {
        panic!("mixed batch not admitted")
    };
    let observed: Vec<_> = result
        .accepted
        .iter()
        .map(|id| service.get(id).unwrap())
        .collect();
    assert_eq!(
        observed
            .iter()
            .map(|task| task.song_id.as_str())
            .collect::<Vec<_>>(),
        ["41", "46", "43"]
    );
    if let Some(old) = &old {
        assert_eq!(observed[1].id, old.id);
        assert_eq!(observed[1].created_ms, old.created_ms);
        assert_eq!(observed[1].work, old.work);
        assert_eq!(observed[1].options.quality, Quality::Lossless);
    }
    let R::Accepted { result: readback } = client
        .search_group_download(
            &epoch,
            C::Status {
                receipt: receipt.clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("missing local readback")
    };
    assert_eq!(result.accepted, readback.accepted);
    http.wait(11).await;
    let calls = http.calls();
    let trace: Vec<_> = calls.iter().map(|call| call.path.as_str()).collect();
    assert_eq!(
        trace[1..],
        [
            LIKED, LIST, STAR_LIST, DETAIL, ADD, DETAIL, DETAIL, AUTH, AUTH, AUTH
        ]
    );
    let mut wire = calls[5].payload.clone();
    let header = wire.as_object_mut().unwrap().remove("header").unwrap();
    let header: Value = serde_json::from_str(header.as_str().unwrap()).unwrap();
    assert_eq!(header["appver"], "3.1.42.205594");
    assert_eq!(header["os"], "pc");
    assert_eq!(
        wire,
        json!({"op":"add","pid":"81","trackIds":r#"["43","41"]"#})
    );
}

#[tokio::test]
async fn search_group_download_system_liked_requires_two_coherent_membership_reads() {
    const LIST: &str = "/api/user/playlist/v2";
    const STAR: &str = "/api/user/playlist";
    const DETAIL: &str = "/api/v6/playlist/detail";
    const ADD: &str = "/api/v1/playlist/manipulate/tracks";
    let star = json!({"id":83,"userId":71,"creator":{"userId":71},"name":"合成系统喜欢",
        "specialType":5,"privacy":10,"tags":[]});
    let list = || json!({"code":200,"playlist":[star.clone()],"more":false}).into();
    let detail = |tracks: &[u64]| {
        let mut row = star.clone();
        row["trackIds"] = json!(tracks.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
        json!({"code":200,"playlist":row}).into()
    };
    let likes = |ids: &[u64]| json!({"code":200,"ids":ids}).into();
    let deny = || json!({"code":200,"data":{"code":403}}).into();
    let root = tempfile::tempdir().unwrap();
    let (api, http) = vinyl_http(
        root.path(),
        &[COOKIE],
        [
            search_page_with_destination(true),
            likes(&[43]),
            list(),
            list(),
            detail(&[43]),
            list(),
            list(),
            detail(&[43]),
            list(),
            detail(&[43]),
            likes(&[43]),
            HttpResponse::Ready(Err(Error::Transport(
                "sealed star add acknowledgement lost".into(),
            ))),
            list(),
            detail(&[43, 41]),
            likes(&[43]),
            list(),
            detail(&[43, 41]),
            likes(&[43, 41]),
            deny(),
            deny(),
            deny(),
        ],
    );
    StateStore::open(root.path().join("config"))
        .unwrap()
        .put(
            "transport",
            "pc-mconfig-v1",
            "pc",
            &serde_json::to_value(Snapshot {
                origin: ApiOrigin::Pc,
                version: 1,
                bucket: String::new(),
                generation: 1,
                source_url: Some("https://p1.music.126.net/obj/fixture.data".into()),
                data: Some(json!({})),
            })
            .unwrap(),
        )
        .unwrap();
    let (client, session, epoch) = client(root.path(), api);
    let rows = search(&client, &epoch).await;
    client.liked_ids().await.unwrap();
    directory(&client, &epoch, root.path()).await;
    let receipt = decision(
        &client,
        &epoch,
        prepare(&rows, 1, &[2, 5, 1, 0]),
        D::PlaylistRequired,
    )
    .await
    .unwrap();
    let choices = || {
        client.search_group_download(
            &epoch,
            C::Destinations {
                receipt: receipt.clone(),
                sort: PlaylistCollectSort::Default,
                page: 0,
            },
        )
    };
    let choose_liked = |destination_receipt: &str| {
        client.search_group_download(
            &epoch,
            C::ChooseLiked {
                receipt: receipt.clone(),
                destination_receipt: destination_receipt.into(),
            },
        )
    };
    let R::Destinations {
        page,
        system_liked_receipt: Some(old),
        ..
    } = choices().await.unwrap()
    else {
        panic!("missing verified system liked destination")
    };
    assert!(page.value.playlists.is_empty());
    let R::Destinations {
        system_liked_receipt: Some(grant),
        ..
    } = choices().await.unwrap()
    else {
        panic!("missing refreshed star grant")
    };
    assert_ne!(old, grant);
    for forged in [&old, "83"] {
        assert!(matches!(
            choose_liked(forged).await,
            Err(Error::StaleResponse)
        ));
    }
    assert!(matches!(
        client
            .search_group_download(
                &epoch,
                C::Choose {
                    receipt: receipt.clone(),
                    playlist_id: "83".into()
                }
            )
            .await,
        Err(Error::StaleResponse)
    ));
    for _ in 0..2 {
        let result = serde_json::to_value(choose_liked(&grant).await.unwrap()).unwrap();
        assert_eq!(result["result"]["phase"], "uncertain");
        assert!(result["result"]["continuation"].is_null());
    }
    assert!(
        DownloadService::new(session.clone())
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
    let result = client
        .search_group_download(
            &epoch,
            C::Reconcile {
                receipt: receipt.clone(),
            },
        )
        .await
        .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(result["result"]["phase"], "confirmed");
    assert_eq!(result["result"]["likedIds"], json!(["43", "41"]));
    let continuation = result["result"]["continuation"]
        .as_str()
        .unwrap()
        .to_owned();
    let result = client
        .search_group_download(
            &epoch,
            C::StartFromPlaylist {
                receipt,
                continuation,
            },
        )
        .await
        .unwrap();
    assert!(matches!(result, R::Accepted { .. }));
    let tasks = DownloadService::new(session.clone())
        .unwrap()
        .list()
        .unwrap();
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.song_id.as_str())
            .collect::<Vec<_>>(),
        ["41", "46", "43"]
    );
    let records = cumulus_core::playlist::PlaylistService::new(session.clone())
        .unwrap()
        .operations(&session.token().unwrap())
        .unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].system_liked_group);
    assert_eq!(records[0].before["origin"], "download");
    http.wait(21).await;
    let calls = http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>()[1..],
        [
            LIKED, LIST, STAR, DETAIL, LIST, STAR, DETAIL, STAR, DETAIL, LIKED, ADD, STAR, DETAIL,
            LIKED, STAR, DETAIL, LIKED, AUTH, AUTH, AUTH
        ]
    );
    assert_eq!(calls[11].payload["pid"], "83");
    assert_eq!(calls[11].payload["trackIds"], r#"["43","41"]"#);
}

#[tokio::test]
async fn search_group_download_create_uses_acknowledged_id_and_never_replays_writes() {
    const LIST: &str = "/api/user/playlist/v2";
    const OWNED: &str = "/api/user/playlist";
    const DETAIL: &str = "/api/v6/playlist/detail";
    const CREATE: &str = "/api/playlist/create";
    const ADD: &str = "/api/v1/playlist/manipulate/tracks";
    const NAME: &str = "合成新下载歌单";

    for (create_code, add_code, delayed) in [
        (None, 403, true),
        (Some(403), 403, true),
        (Some(200), 403, true),
        (Some(200), 200, true),
        (Some(200), 200, false),
    ] {
        let acknowledged = create_code == Some(200);
        let rejected = create_code == Some(403);
        let root = tempfile::tempdir().unwrap();
        let row = json!({"id":91,"userId":71,"name":NAME,"privacy":10,"specialType":0});
        let list = |exists| {
            json!({"code":200,"playlist":if exists {vec![row.clone()]} else {vec![]},"more":false})
                .into()
        };
        let detail = |ids: &[u64]| {
            let mut row = row.clone();
            row["tags"] = json!([]);
            row["trackIds"] = json!(ids.iter().map(|id| json!({"id":id})).collect::<Vec<_>>());
            json!({"code":200,"playlist":row}).into()
        };
        let mut replies = vec![
            search_page_with_destination(true),
            json!({"code":200,"ids":[43]}).into(),
            list(false), // regular picker destinations
            list(false), // system-liked verification
            list(false), // fresh owned preflight before the create
            match create_code {
                Some(200) => json!({"code":200,"id":91}).into(),
                Some(code) => json!({"code":code}).into(),
                None => HttpResponse::Ready(Err(Error::Transport(
                    "synthetic lost search create".into(),
                ))),
            },
            list(!delayed), // verification may not observe the new playlist yet
        ];
        if delayed && !rejected {
            replies.push(list(true)); // explicit read-only recovery
        }
        if acknowledged {
            replies.extend([
                detail(&[]),
                json!({"code":add_code}).into(),
                detail(if add_code == 200 { &[43, 41] } else { &[] }),
            ]);
        }
        if delayed && add_code == 200 {
            replies.extend([
                json!({"code":200,"data":{"code":403}}).into(),
                json!({"code":200,"data":{"code":403}}).into(),
                json!({"code":200,"data":{"code":403}}).into(),
            ]);
        }
        let (api, http) = vinyl_http(root.path(), &[COOKIE], replies);
        StateStore::open(root.path().join("config"))
            .unwrap()
            .put(
                "transport",
                "pc-mconfig-v1",
                "pc",
                &serde_json::to_value(Snapshot {
                    origin: ApiOrigin::Pc,
                    version: 1,
                    bucket: String::new(),
                    generation: 1,
                    source_url: Some("https://p1.music.126.net/obj/fixture.data".into()),
                    data: Some(json!({})),
                })
                .unwrap(),
            )
            .unwrap();
        let (client, session, epoch) = client(root.path(), api);
        let rows = search(&client, &epoch).await;
        client.liked_ids().await.unwrap();
        directory(&client, &epoch, root.path()).await;
        let receipt = decision(
            &client,
            &epoch,
            prepare(&rows, 1, &[2, 5, 1, 0]),
            D::PlaylistRequired,
        )
        .await
        .unwrap();
        let command = |kind, args: Value| {
            let mut value = json!({"kind":kind,"receipt":receipt});
            value
                .as_object_mut()
                .unwrap()
                .extend(args.as_object().unwrap().clone());
            client.search_group_download(&epoch, serde_json::from_value::<C>(value).unwrap())
        };
        let create = || {
            command(
                "create_playlist",
                json!({"name":NAME,"privatePlaylist":true}),
            )
        };
        let add = || command("add_created", json!({}));
        let tasks = DownloadService::new(session).unwrap();
        assert!(matches!(create().await, Err(Error::StaleResponse)));
        assert!(matches!(
            command("destinations", json!({"page":0,"sort":"default"}))
                .await
                .unwrap(),
            R::Destinations {
                can_create: true,
                ..
            }
        ));
        assert!(matches!(add().await, Err(Error::StaleResponse)));
        let first = serde_json::to_value(create().await.unwrap()).unwrap()["result"].clone();
        if !delayed {
            assert_eq!(first["createPhase"], "confirmed");
            assert_eq!(first["playlistId"], "91");
            assert_eq!(first["add"]["phase"], "confirmed");
            assert!(first["add"]["continuation"].is_string());
            assert!(tasks.list().unwrap().is_empty());
            assert_eq!(
                serde_json::to_value(create().await.unwrap()).unwrap()["result"],
                first
            );
            assert_eq!(
                http.calls()
                    .iter()
                    .map(|c| c.path.as_str())
                    .collect::<Vec<_>>()[1..],
                [
                    LIKED, LIST, OWNED, OWNED, CREATE, OWNED, DETAIL, ADD, DETAIL
                ]
            );
            continue;
        }
        if rejected {
            assert_eq!(first["createPhase"], "rejected");
        } else {
            assert!(matches!(
                first["createPhase"].as_str(),
                Some("uncertain" | "verifying")
            ));
        }
        assert_eq!(first["canAdd"], false);
        assert!(first["add"].is_null());
        assert_eq!(
            serde_json::to_value(create().await.unwrap()).unwrap()["result"],
            first,
            "a repeated create is only a local read"
        );
        assert_eq!(http.calls().len(), 7);
        assert!(matches!(add().await, Err(Error::StaleResponse)));
        assert!(matches!(
            command("choose", json!({"playlistId":"91"})).await,
            Err(Error::StaleResponse)
        ));
        let recovered = serde_json::to_value(
            command("creation_status", json!({"reconcile":true}))
                .await
                .unwrap(),
        )
        .unwrap()["result"]
            .clone();
        if rejected {
            assert_eq!(recovered, first);
            assert!(matches!(add().await, Err(Error::StaleResponse)));
            assert!(tasks.list().unwrap().is_empty());
            assert_eq!(http.calls().len(), 7, "definite rejection stays terminal");
            continue;
        }
        assert_eq!(recovered["createPhase"], "confirmed");
        assert_eq!(recovered["canAdd"], acknowledged);
        assert!(recovered["add"].is_null());
        assert!(tasks.list().unwrap().is_empty());
        assert_eq!(
            http.calls().len(),
            8,
            "readback never sends the not-yet-sent add"
        );
        let expected = [LIKED, LIST, OWNED, OWNED, CREATE, OWNED, OWNED];
        let calls = http.calls();
        assert_eq!(
            calls.iter().map(|c| c.path.as_str()).collect::<Vec<_>>()[1..],
            expected
        );
        let mut create_wire = calls[5].payload.clone();
        create_wire.as_object_mut().unwrap().remove("header");
        assert_eq!(create_wire, json!({"uid":"71","name":NAME,"privacy":"10"}));
        if !acknowledged {
            assert!(recovered["playlistId"].is_null());
            assert!(matches!(add().await, Err(Error::StaleResponse)));
            assert!(tasks.list().unwrap().is_empty());
            assert_eq!(http.calls().len(), 8);
            continue;
        }
        assert_eq!(recovered["playlistId"], "91");
        let picked = serde_json::to_value(add().await.unwrap()).unwrap()["result"].clone();
        assert_eq!(
            picked["add"]["phase"],
            if add_code == 200 {
                "confirmed"
            } else {
                "rejected"
            }
        );
        assert_eq!(picked["canAdd"], false);
        assert_eq!(
            serde_json::to_value(add().await.unwrap()).unwrap()["result"],
            picked
        );
        assert!(tasks.list().unwrap().is_empty());
        let calls = http.calls();
        let mut add_wire = calls[9].payload.clone();
        add_wire.as_object_mut().unwrap().remove("header");
        assert_eq!(
            add_wire,
            json!({"op":"add","pid":"91","trackIds":r#"["43","41"]"#})
        );
        if add_code != 200 {
            assert_eq!(picked["add"]["addFailed"], true);
            assert!(picked["add"]["continuation"].is_null());
            assert_eq!(
                calls.iter().map(|c| c.path.as_str()).collect::<Vec<_>>()[8..],
                [DETAIL, ADD, DETAIL]
            );
            continue;
        }
        let permit = picked["add"]["continuation"].as_str().unwrap();
        assert!(matches!(
            command("start_from_playlist", json!({"continuation":permit}))
                .await
                .unwrap(),
            R::Accepted { .. }
        ));
        assert_eq!(
            tasks
                .list()
                .unwrap()
                .iter()
                .map(|row| row.song_id.as_str())
                .collect::<Vec<_>>(),
            ["41", "46", "43"]
        );
        http.wait(14).await;
        let calls = http.calls();
        assert_eq!(
            calls.iter().map(|c| c.path.as_str()).collect::<Vec<_>>()[8..],
            [DETAIL, ADD, DETAIL, AUTH, AUTH, AUTH]
        );
    }
}
