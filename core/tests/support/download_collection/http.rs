use super::*;
use crate::support::http::ScriptHttp;
use cumulus_core::{
    account::credentials::Credentials,
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient, RequestOptions, RoutingPolicy},
    storage::StateStore,
};

#[tokio::test]
async fn downloaded_collection_system_liked_http_uses_one_native_account_without_voice_experiment()
{
    for foreign in [false, true] {
        let f = Fixture::new();
        let (task, _) = f.completed().await;
        let credentials = Arc::new(
            Credentials::open(
                Arc::new(
                    StateStore::open(f.root.path().join("synthetic-native-credentials")).unwrap(),
                ),
                ApiOrigin::Pc,
            )
            .unwrap(),
        );
        let url = ApiOrigin::Pc.url();
        let revision = credentials.request_credentials(&url).unwrap().revision;
        let mut cookies = vec!["MUSIC_U=synthetic-native-download; Path=/; Secure".into()];
        if foreign {
            cookies.push("MUSIC_U=other-synthetic-account; Path=/eapi/song/like/; Secure".into());
        }
        credentials
            .record_response(&url, &cookies, None, &revision)
            .unwrap();
        let observation = |added: bool| {
            let mut row = list("83", 1, 5);
            row["trackIds"] = if added {
                json!([{"id":123,"v":0}])
            } else {
                json!([])
            };
            [
                json!({"code":200,"more":false,"playlist":[row.clone()]}),
                json!({"code":200,"playlist":row}),
                json!({"code":200,"ids":if added {json!([123])} else {json!([])}}),
            ]
        };
        let http = Arc::new(ScriptHttp::new(
            observation(false)
                .into_iter()
                .take(2)
                .chain(observation(false))
                .chain([json!({"code":200})])
                .chain(observation(true))
                .map(Into::into),
        ));
        let native = Arc::new(
            MusicHttpClient::with_backend(
                credentials,
                ClientProfile::default(),
                RequestOptions {
                    routing: RoutingPolicy::OfficialEapi,
                    ..Default::default()
                },
                None,
                http.clone(),
            )
            .unwrap(),
        );
        assert!(
            native.song_card_system_liked_scope_revision().is_err(),
            "no SongCard PC context"
        );
        f.session
            .adopt(native, json!({"userId":1}), &f.session.token().unwrap())
            .unwrap();
        let epoch = f.client.account().unwrap().epoch;
        let visit = f
            .client
            .download_collect_prepare(&epoch, &task)
            .unwrap()
            .value
            .receipt;
        let selection = f
            .client
            .download_collect_liked_destination(&epoch, &visit)
            .await;
        if foreign {
            assert!(matches!(selection, Err(Error::CredentialChanged)));
            assert!(http.calls().is_empty());
            continue;
        }
        let target = selection.unwrap().value.unwrap();
        let result = f
            .client
            .download_collect_liked(&epoch, &visit, &target.receipt, false)
            .await
            .unwrap()
            .value;
        assert_eq!(result.phase, PlaylistPhase::Confirmed);
        assert_eq!(result.verified_liked_track_ids, Some(vec!["123".into()]));
        let calls = http.calls();
        assert_eq!(
            calls
                .iter()
                .map(|call| call.path.as_str())
                .collect::<Vec<_>>(),
            [
                "/api/user/playlist",
                DETAIL,
                "/api/user/playlist",
                DETAIL,
                "/api/song/like/get",
                WRITE,
                "/api/user/playlist",
                DETAIL,
                "/api/song/like/get",
            ]
        );
        for call in &calls {
            assert!(
                call.cookie
                    .as_deref()
                    .unwrap()
                    .contains("MUSIC_U=synthetic-native-download")
            );
        }
        assert_eq!(calls[5].payload["pid"], "83");
        assert_eq!(calls[5].payload["trackIds"], "[\"123\"]");
        assert_eq!(calls[5].payload["op"], "add");
        assert!(calls[5].payload.get("imme").is_none());
    }
}
