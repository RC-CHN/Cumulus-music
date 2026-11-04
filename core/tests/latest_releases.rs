use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{LatestReleaseQuery, PlaySelection},
    },
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct ReleaseApi(Mutex<Vec<(String, Value, bool)>>);

impl Api for ReleaseApi {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .push((path.into(), body.clone(), cache));
            match (path, body["areaId"].as_str(), body["offset"].as_u64()) {
                ("/api/v2/discovery/new/songs", Some("536870911"), Some(0)) => Ok(json!({
                    "code":200, "data":[
                        {"id":"9007199254740993", "name":"可试听", "artists":[{"id":2,"name":"歌手"}],
                         "duration":23000, "album":{"id":6,"name":"专辑",
                         "picUrl":"https://p1.music.126.net/a.jpg?token=PRIVATE"},
                         "privilege":{"fee":1,"pl":0,"flag":1024}, "url":"https://example.invalid/PRIVATE"},
                        {"id":7,"name":"暂不可播放","privilege":{"pl":0,"flag":0}},
                        {"id":7,"name":"重复"},
                        {"name":"无 ID"}
                    ]
                })),
                ("/api/discovery/new/albums/area", None, Some(0)) if body["area"] == "ALL" => {
                    let week = (1..=126)
                        .map(|id| json!({"id":id,"name":"每周唱片"}))
                        .collect::<Vec<_>>();
                    Ok(json!({"code":200,"hasMore":false,"weekData":week,"monthData":[]}))
                }
                ("/api/discovery/new/albums/area", None, Some(0)) => Ok(json!({
                    "code":200, "hasMore":true,
                    "weekData":[{"id":21,"name":"本周"}],
                    "monthData":[{"albumId":42,"name":"合成唱片","artist":{"id":2,"name":"歌手"}},
                                 {"name":"不可投影的行"}]
                })),
                ("/api/discovery/new/albums/area", None, Some(2)) => Ok(json!({
                    "code":200, "hasMore":false, "monthData":[{"id":43,"name":"后续唱片"}]
                })),
                ("/api/discovery/new/albums/area", None, Some(9)) => Ok(json!({
                    "code":200, "hasMore":true,"monthData":[]
                })),
                _ => Err(Error::Response("unexpected latest-release request")),
            }
        })
    }
}

#[tokio::test]
async fn distinct_global_release_endpoints_preserve_service_cursor_and_safe_projection() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(ReleaseApi::default());
    let profiles = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(profiles.store())));
    session
        .adopt(
            api.clone(),
            json!({"userId":2,"nickname":"发行目录夹具"}),
            &session.token().unwrap(),
        )
        .unwrap();
    let client = MusicClient::with_session(session.clone(), profiles);
    let epoch = session.token().unwrap().generation.to_string();
    let query = |raw| serde_json::from_value::<LatestReleaseQuery>(raw).unwrap();
    let songs = json!({"kind":"songs","area":"536870911"});
    let read = |value| client.latest_releases(&epoch, query(value));
    let page = serde_json::to_value(read(songs.clone()).await.unwrap().value).unwrap();
    assert_eq!(page["tracks"][0]["id"], "9007199254740993");
    assert_eq!(page["tracks"][0]["fee"], 1);
    assert_eq!(page["tracks"][0]["albumId"], "6");
    assert_eq!(page["tracks"].as_array().unwrap().len(), 2);
    assert_eq!(page["playableIds"], json!(["9007199254740993"]));
    assert!(!page.to_string().contains("PRIVATE"));
    let row = |area: &str, revision: &Value, id: &str| {
        serde_json::from_value::<PlaySelection>(json!({
            "kind":"song_row_double_click","selection":{
                "kind":"latest_release","area":area,"revision":revision,
                "start_id":id,"quality":"standard"
            }
        }))
        .unwrap()
    };
    let revision = &page["revision"];
    assert!(revision.as_str().is_some_and(|value| value.len() == 32));
    assert!(matches!(
        client
            .play_selection(&epoch, row("0", revision, "9007199254740993"))
            .await,
        Err(Error::State("latest song list no longer available"))
    ));
    assert!(matches!(
        client
            .play_selection(&epoch, row("536870911", revision, "7"))
            .await,
        Err(Error::InvalidInput(
            "song is not in the playable release list"
        ))
    ));
    assert!(matches!(
        client
            .play_selection(
                &epoch,
                row("536870911", &json!("forged"), "9007199254740993")
            )
            .await,
        Err(Error::State("latest song list no longer available"))
    ));
    let observed_week_shape = serde_json::to_value(read(json!({
        "kind":"albums","area":"ALL","recommended":true,"cursor":{"year":2020,"month":1,"offset":0}
    })).await.unwrap().value).unwrap();
    assert_eq!(observed_week_shape["week"].as_array().unwrap().len(), 126);
    assert_eq!(observed_week_shape["albums"], json!([]));
    let first = serde_json::to_value(read(json!({
        "kind":"albums","area":"ZH","recommended":true,"cursor":{"year":2020,"month":1,"offset":0}
    })).await.unwrap().value).unwrap();
    assert_eq!(first["week"][0]["id"], "21");
    assert_eq!(first["albums"][0]["id"], "42");
    assert_eq!(first["next"], json!({"year":2020,"month":1,"offset":2}));
    let next = serde_json::to_value(
        read(json!({
            "kind":"albums","area":"ZH","recommended":false,"cursor":first["next"]
        }))
        .await
        .unwrap()
        .value,
    )
    .unwrap();
    assert_eq!(next["next"], json!({"year":2019,"month":12,"offset":0}));
    let all = serde_json::to_value(read(json!({
        "kind":"albums","area":"EA","recommended":false,"cursor":{"year":2020,"month":1,"offset":0}
    })).await.unwrap().value).unwrap();
    assert_eq!(all["albums"][0]["id"], "42");
    assert_eq!(all["albums"][1]["id"], "43");
    assert_eq!(all["next"], json!({"year":2019,"month":12,"offset":0}));
    assert!(matches!(read(json!({
        "kind":"albums","area":"ALL","recommended":false,"cursor":{"year":2020,"month":1,"offset":9}
    })).await, Err(Error::Response("new album cursor did not advance"))));
    assert!(matches!(read(json!({
        "kind":"albums","area":"ALL","recommended":false,"cursor":{"year":2020,"month":13,"offset":0}
    })).await, Err(Error::InvalidInput(_))));
    assert!(matches!(
        client.latest_releases("stale", query(songs)).await,
        Err(Error::StaleResponse)
    ));
    session
        .adopt(
            api.clone(),
            json!({"userId":99,"nickname":"另一夹具账号"}),
            &session.token().unwrap(),
        )
        .unwrap();
    let new_epoch = session.token().unwrap().generation.to_string();
    assert!(matches!(
        client
            .play_selection(&new_epoch, row("536870911", revision, "9007199254740993"))
            .await,
        Err(Error::State("latest song list no longer available"))
    ));
    assert_eq!(
        *api.0.lock().unwrap(),
        [
            (
                "/api/v2/discovery/new/songs".into(),
                json!({"areaId":"536870911","limit":100,"offset":0}),
                false
            ),
            (
                "/api/discovery/new/albums/area".into(),
                json!({"area":"ALL","year":2020,"month":1,"offset":0,"limit":10,"rcmd":true}),
                false
            ),
            (
                "/api/discovery/new/albums/area".into(),
                json!({"area":"ZH","year":2020,"month":1,"offset":0,"limit":10,"rcmd":true}),
                false
            ),
            (
                "/api/discovery/new/albums/area".into(),
                json!({"area":"ZH","year":2020,"month":1,"offset":2,"limit":50,"rcmd":false}),
                false
            ),
            (
                "/api/discovery/new/albums/area".into(),
                json!({"area":"EA","year":2020,"month":1,"offset":0,"limit":50,"rcmd":false}),
                false
            ),
            (
                "/api/discovery/new/albums/area".into(),
                json!({"area":"EA","year":2020,"month":1,"offset":2,"limit":50,"rcmd":false}),
                false
            ),
            (
                "/api/discovery/new/albums/area".into(),
                json!({"area":"ALL","year":2020,"month":1,"offset":9,"limit":50,"rcmd":false}),
                false
            )
        ]
    );
}
