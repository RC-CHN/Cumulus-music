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
    download::service::DownloadService,
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
    const LIST: &str = "/api/user/playlist/v2";
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
    let R::Destinations { page } = client
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
        [LIKED, LIST]
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
    assert!(
        DownloadService::new(session.clone())
            .unwrap()
            .list()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        http.calls().len(),
        6,
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
    assert!(matches!(result, R::Accepted { .. }));
    let tasks = DownloadService::new(session).unwrap().list().unwrap();
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.song_id.as_str())
            .collect::<Vec<_>>(),
        ["41", "46", "43"]
    );
    http.wait(10).await;
    let calls = http.calls();
    let trace: Vec<_> = calls.iter().map(|call| call.path.as_str()).collect();
    assert_eq!(
        trace[1..],
        [LIKED, LIST, DETAIL, ADD, DETAIL, DETAIL, AUTH, AUTH, AUTH]
    );
    let mut wire = calls[4].payload.clone();
    let header = wire.as_object_mut().unwrap().remove("header").unwrap();
    let header: Value = serde_json::from_str(header.as_str().unwrap()).unwrap();
    assert_eq!(header["appver"], "3.1.42.205594");
    assert_eq!(header["os"], "pc");
    assert_eq!(
        wire,
        json!({"op":"add","pid":"81","trackIds":r#"["43","41"]"#})
    );
}
