mod support;
use cumulus_core::{
    Error, Result,
    account::{AccountApi, LikeContext, validate_settings},
    catalog::{CatalogApi, PlayOptions, SearchOptions},
    cloud::{AttachmentKind, AudioMetadata, CloudApi, UploadedAttachment},
    membership::MembershipApi,
    playlist::{PlaylistApi, TrackOperation},
    protocol::{
        api::Api,
        pagination::{self, PageBounds},
    },
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi};

fn kind(name: &str) -> AttachmentKind {
    match name {
        "cover" => AttachmentKind::Cover,
        "lyric" => AttachmentKind::Lyric,
        "cue" => AttachmentKind::Cue,
        _ => panic!("invalid fixture"),
    }
}

async fn execute(case: &Value, api: Arc<ScriptApi>) -> Result<Value> {
    let catalog = CatalogApi::new(api.clone());
    let playlist = PlaylistApi::new(api.clone());
    let account = AccountApi::new(api.clone());
    let membership = MembershipApi::new(api.clone());
    let cloud = CloudApi::new(api.clone());
    let args = &case["args"];
    let kwargs = &case["kwargs"];
    Ok(match case["method"].as_str().unwrap() {
        "account" => api.account().await?,
        "membership" => membership.info(args.as_array().unwrap().first()).await?,
        "pages" => json!(
            pagination::pages(
                api.as_ref(),
                args[0].as_str().unwrap(),
                args[1].clone(),
                args[2].as_str().unwrap(),
                PageBounds {
                    limit: kwargs["limit"].as_u64().unwrap() as usize,
                    max_pages: 1000
                }
            )
            .await?
        ),
        "playlists" => json!(playlist.list(&args[0]).await?),
        "playlist" => playlist.detail(&args[0]).await?,
        "songs" => serde_json::to_value(
            catalog
                .songs(
                    args[0].as_array().unwrap(),
                    args[1].as_u64().unwrap() as usize,
                )
                .await?,
        )?,
        "playlist_tracks" => serde_json::to_value(playlist.tracks(&args[0]).await?)?,
        "create_playlist" => json!(
            playlist
                .create(
                    &args[0],
                    args[1].as_str().unwrap(),
                    kwargs["private"].as_bool().unwrap_or(true)
                )
                .await?
        ),
        "rename_playlist" => playlist.rename(&args[0], args[1].as_str().unwrap()).await?,
        "describe_playlist" => {
            playlist
                .describe(&args[0], args[1].as_str().unwrap())
                .await?
        }
        "update_playlist_metadata" => {
            playlist
                .update_metadata(
                    &args[0],
                    kwargs["name"].as_str().unwrap(),
                    &kwargs["tags"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap().into())
                        .collect::<Vec<_>>(),
                    kwargs["description"].as_str().unwrap(),
                )
                .await?
        }
        "change_tracks" => {
            playlist
                .change_tracks(
                    &args[0],
                    args[1].as_array().unwrap(),
                    match args[2].as_str().unwrap() {
                        "add" => TrackOperation::Add,
                        "del" => TrackOperation::Delete,
                        _ => TrackOperation::Replace,
                    },
                )
                .await?
        }
        "delete_playlist" => playlist.delete(&args[0]).await?,
        "liked_ids" => json!(account.liked_ids(&args[0]).await?),
        "set_like" => {
            account
                .set_like(
                    &args[0],
                    &args[1],
                    args[2].as_bool().unwrap(),
                    LikeContext {
                        song_rq_refer: kwargs.get("song_rq_refer").cloned(),
                        rq_refer: kwargs.get("rq_refer").cloned(),
                        alg: kwargs.get("alg").cloned(),
                    },
                )
                .await?
        }
        "collected_albums" => json!(account.collected_albums().await?),
        "collected_artists" => json!(account.collected_artists().await?),
        "collect_album" => {
            account
                .collect_album(&args[0], args[1].as_bool().unwrap())
                .await?
        }
        "collect_artist" => {
            account
                .collect_artist(&args[0], args[1].as_bool().unwrap())
                .await?
        }
        "account_settings" => account.settings().await?,
        "update_account_settings" => account.update_settings(&args[0]).await?,
        "search" => {
            catalog
                .search(
                    args[0].as_str().unwrap(),
                    SearchOptions {
                        kind: kwargs["kind"].as_u64().unwrap() as u32,
                        limit: kwargs["limit"].as_u64().unwrap() as usize,
                        offset: kwargs["offset"].as_u64().unwrap() as usize,
                    },
                )
                .await?
        }
        "daily" => catalog.daily().await?,
        "album" => catalog.album(&args[0]).await?,
        "artist" => catalog.artist(&args[0]).await?,
        "lyric" => catalog.lyric(&args[0]).await?,
        "play_url" => json!(
            catalog
                .play_url(
                    &args[0],
                    PlayOptions {
                        level: kwargs["level"].as_str().unwrap(),
                        encode_type: kwargs["encode_type"].as_str().unwrap(),
                        trial_mode: kwargs["trial_mode"].as_i64().unwrap() as i32
                    }
                )
                .await?
        ),
        "download_url" => json!(
            catalog
                .download_url(&args[0], kwargs["level"].as_str().unwrap())
                .await?
        ),
        "cloud" => json!(cloud.list().await?),
        "cloud_check" => {
            cloud
                .check(
                    kwargs["md5"].as_str().unwrap(),
                    kwargs["length"].as_u64().unwrap(),
                    kwargs["file_name"].as_str().unwrap(),
                    kwargs["check_mute"].as_bool().unwrap(),
                )
                .await?
        }
        "cloud_allocate" => {
            cloud
                .allocate(args[0].as_str().unwrap(), args[1].as_str().unwrap())
                .await?
        }
        "nos_refresh" => json!(
            cloud
                .refresh_nos_token(args[0].as_str().unwrap(), args[1].as_str().unwrap())
                .await?
        ),
        "nos_allocate_attachment" => {
            cloud
                .allocate_attachment(
                    args[0].as_str().unwrap(),
                    kind(kwargs["kind"].as_str().unwrap()),
                )
                .await?
        }
        "cloud_metadata" => {
            let job = &args[0];
            let attachments: Vec<_> = kwargs["attachments"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| UploadedAttachment {
                    kind: kind(k),
                    document_id: v["resource_id"].clone(),
                })
                .collect();
            cloud
                .metadata(AudioMetadata {
                    upload_ticket: job["song_id"].as_str().unwrap(),
                    md5: job["md5"].as_str().unwrap(),
                    file_name: job["source"].as_str().unwrap().rsplit('/').next().unwrap(),
                    bitrate: kwargs["bitrate"].as_u64().unwrap(),
                    resource_id: job["resource_id"].as_str(),
                    object: Some((
                        job["bucket"].as_str().unwrap(),
                        job["object_key"].as_str().unwrap(),
                    )),
                    song: kwargs["song"].as_str().unwrap(),
                    artist: kwargs["artist"].as_str().unwrap(),
                    album: kwargs["album"].as_str().unwrap(),
                    attachments: &attachments,
                })
                .await?
        }
        "delete_cloud" => cloud.delete(args[0].as_array().unwrap()).await?,
        "publish_cloud" => cloud.publish(&args[0]).await?,
        "cloud_status" => cloud.status(args[0].as_array().unwrap()).await?,
        "cloud_download" => cloud.download(&args[0]).await?,
        method => panic!("unmapped fixture method {method}"),
    })
}

#[tokio::test]
async fn domain_apis_match_47_frozen_legacy_contracts() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/legacy-api.json")).unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 47);
    for case in fixture["cases"].as_array().unwrap() {
        let api = Arc::new(ScriptApi::new(
            case["responses"]
                .as_array()
                .unwrap()
                .iter()
                .cloned()
                .map(Reply::from),
        ));
        let output = execute(case, api.clone())
            .await
            .unwrap_or_else(|e| panic!("{} failed: {e}", case["method"]));
        assert_eq!(output, case["output"], "{} output", case["method"]);
        let calls: Vec<_> = api
            .calls()
            .iter()
            .map(|call| json!({"path":call.path,"payload":call.payload,"cache":call.cache}))
            .collect();
        assert_eq!(json!(calls), case["calls"], "{} requests", case["method"]);
    }
}

#[tokio::test]
async fn pagination_rejects_empty_repeated_malformed_and_truncated_results() {
    let cases = [
        vec![json!({"code":200,"data":[],"more":true})],
        vec![
            json!({"code":200,"data":[{"a":1,"b":2}],"more":true}),
            json!({"code":200,"data":[{"b":2,"a":1}],"more":true}),
        ],
        vec![json!({"code":200,"data":[1],"more":"true"})],
        vec![json!({"code":200,"data":[1],"more":true})],
        vec![json!({"code":200})],
    ];
    for responses in cases {
        let max_pages = responses.len();
        let api = ScriptApi::new(responses.into_iter().map(Reply::from));
        let error = pagination::pages(
            &api,
            "/api/test",
            json!({}),
            "data",
            PageBounds {
                limit: 1,
                max_pages,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(error, Error::Response(_)));
    }
    let api = ScriptApi::new([]);
    assert!(
        pagination::pages(
            &api,
            "/api/test",
            json!({}),
            "data",
            PageBounds {
                limit: 0,
                max_pages: 1
            }
        )
        .await
        .is_err()
    );
    assert!(api.calls().is_empty());
}

#[tokio::test]
async fn playlist_batch_preserves_partial_outcome_and_never_replays_writes() {
    let response = json!({"code":200,"/api/playlist/update/name":{"code":200},"/api/playlist/tags/update":{"code":403}});
    let api = Arc::new(ScriptApi::new([
        response.clone().into(),
        json!({"code":403}).into(),
        json!({"code":200}).into(),
    ]));
    let playlist = PlaylistApi::new(api.clone());
    let error = playlist
        .update_metadata(&json!(1), "test", &["tag".into()], "desc")
        .await
        .unwrap_err();
    match error {
        Error::Batch { responses, failed } => {
            assert_eq!(responses, response);
            assert_eq!(
                failed,
                ["/api/playlist/tags/update", "/api/playlist/desc/update"]
            );
        }
        _ => panic!("expected partial batch failure"),
    }
    assert!(
        playlist
            .create(&json!(1), "test", true)
            .await
            .unwrap_err()
            .is_service_code(403)
    );
    assert!(matches!(
        playlist.create(&json!(1), "test", true).await,
        Err(Error::Response(_))
    ));
    assert_eq!(api.calls().len(), 3);
}

#[tokio::test]
async fn invalid_names_tags_settings_and_scenes_stop_before_network() {
    let api = Arc::new(ScriptApi::new([]));
    let playlist = PlaylistApi::new(api.clone());
    assert!(playlist.create(&json!(1), "a#b", true).await.is_err());
    for tags in [
        vec!["a".into(), "a".into()],
        vec!["a;b".into()],
        vec!["".into()],
        vec!["a".into(), "b".into(), "c".into(), "d".into()],
    ] {
        assert!(
            playlist
                .update_metadata(&json!(1), "test", &tags, "")
                .await
                .is_err()
        );
    }
    assert!(
        playlist
            .update_metadata(&json!(1), "test", &[], &"中".repeat(501))
            .await
            .is_err()
    );
    for patch in [
        json!({}),
        json!({"personalizedRecommend":true}),
        json!({"shareSetting":true}),
        json!({"shareSetting":1.0}),
        json!({"allowLikedNotify":1}),
        json!({"personalServiceSetting":1}),
    ] {
        assert!(validate_settings(&patch).is_err());
        assert!(
            AccountApi::new(api.clone())
                .update_settings(&patch)
                .await
                .is_err()
        );
    }
    assert!(
        CatalogApi::new(api.clone())
            .play_url(
                &json!(2),
                PlayOptions {
                    trial_mode: 99,
                    ..Default::default()
                }
            )
            .await
            .is_err()
    );
    assert!(api.calls().is_empty());
}

#[tokio::test]
async fn cloud_rejects_conflicting_destinations_and_unusable_attachment_ids() {
    let api = Arc::new(ScriptApi::new([json!({"code":200,"data":{"key":"a","objectKey":"b"}}),json!({"code":200,"data":{"bucket":"fixture","objectKey":"a","token":"synthetic","docId":-1}}),json!({"code":200,"result":{"token":"bad\r\nheader"}})].map(Reply::from)));
    let cloud = CloudApi::new(api.clone());
    assert!(matches!(
        cloud.allocate("audio.flac", &"ab".repeat(16)).await,
        Err(Error::Response(_))
    ));
    assert!(matches!(
        cloud
            .allocate_attachment("words.lrc", AttachmentKind::Lyric)
            .await,
        Err(Error::Response(_))
    ));
    assert!(matches!(
        cloud.refresh_nos_token("fixture", "object").await,
        Err(Error::Response(_))
    ));
    assert!(
        cloud
            .allocate_attachment("image.svg", AttachmentKind::Cover)
            .await
            .is_err()
    );
    assert!(
        cloud
            .allocate("../audio.flac", &"ab".repeat(16))
            .await
            .is_err()
    );
    assert_eq!(api.calls().len(), 3);
}

#[tokio::test]
async fn song_batches_keep_missing_entries_and_never_confuse_local_tracks_with_catalog() {
    let api = Arc::new(ScriptApi::new([json!({"code":200,"playlist":{"trackIds":[{"id":"local-name","v":-1},{"id":3},{"id":2},{"id":3}]}}),json!({"code":200,"songs":[{"id":2}]})].map(Reply::from)));
    let result = PlaylistApi::new(api.clone())
        .tracks(&json!(1))
        .await
        .unwrap();
    assert_eq!(result.songs.songs, [None, Some(json!({"id":2})), None]);
    assert_eq!(result.songs.missing, ["3"]);
    assert_eq!(result.local_tracks, [json!({"id":"local-name","v":-1})]);
    let requested: Value =
        serde_json::from_str(api.calls()[1].payload["c"].as_str().unwrap()).unwrap();
    assert_eq!(requested, json!([{"id":"3","v":0},{"id":"2","v":0}]));
}
