use super::*;
use cumulus_core::{
    Error,
    account::{credentials::Credentials, profiles::ProfileRegistry},
    client::{MusicClient, dto::DownloadCommand},
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient, RequestOptions, RoutingPolicy},
};
use support::http::ScriptHttp;

const RIGHTS: &str = "/api/dj/voices/download/check";
const MUSICS: &str = "/api/dj/program/song/musics";

fn offered() -> Value {
    json!({"code":200,"programs":[{"musics":[
        {"id":91,"bitrate":320000,"size":3145728},
        {"id":91,"bitrate":192000,"size":2359296},
        {"id":91,"bitrate":128000,"size":2048},
        {"id":91,"bitrate":64000,"size":1024},
        {"id":91,"bitrate":0,"size":999999}
    ]}]})
}

fn confirm(receipt: &str, level: &str) -> DownloadCommand {
    serde_json::from_value(json!({
        "kind":"confirm_voice","receipt":receipt,"quality":level,"tags":false,"cover":false
    }))
    .unwrap()
}

fn client(f: &Fixture) -> MusicClient {
    MusicClient::with_session(
        f.session.clone(),
        ProfileRegistry::open(&f.root.path().join("state"), ApiOrigin::Pc).unwrap(),
    )
    .with_download_backend(f.media.clone())
}

#[tokio::test]
async fn voice_quality_single_prepares_over_signed_pc_then_uses_the_real_media_id() {
    let f = Fixture::new(vec![]);
    let credentials = Arc::new(
        Credentials::open(
            Arc::new(StateStore::open(f.root.path().join("voice-quality-creds")).unwrap()),
            ApiOrigin::Pc,
        )
        .unwrap(),
    );
    let url = ApiOrigin::Pc.url();
    let revision = credentials.request_credentials(&url).unwrap().revision;
    credentials
        .record_response(
            &url,
            &["MUSIC_U=synthetic-voice-quality; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    let Reply::Ready(Ok(rights)) = permission(true) else {
        unreachable!()
    };
    let Reply::Ready(Ok(detail)) = detail() else {
        unreachable!()
    };
    let Reply::Ready(Ok(mut media)) = resource() else {
        unreachable!()
    };
    media["data"]["level"] = json!("higher");
    let http = Arc::new(ScriptHttp::new(
        [
            rights.clone(),
            offered(),
            detail.clone(),
            detail,
            rights,
            media,
        ]
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
    let current = f.session.token().unwrap();
    f.session
        .adopt(native, json!({"userId":1}), &current)
        .unwrap();
    let client = client(&f);
    let epoch = client.account().unwrap().epoch;
    client
        .download_command(
            &epoch,
            DownloadCommand::Configure {
                root: f.root.path().join("output").to_string_lossy().into_owned(),
                path_type: 0,
                file_type: 1,
            },
        )
        .await
        .unwrap();
    let plan = client
        .download_voice_prepare(&epoch, vec!["81".into()])
        .await
        .unwrap();
    let projected = serde_json::to_value(&plan).unwrap();
    assert_eq!(projected["state"], "ready");
    assert_eq!(
        projected["choices"],
        json!([
            {"quality":"standard","label":"普通（1KB）"},
            {"quality":"higher","label":"较高（2.3M）"},
            {"quality":"exhigh","label":"极高（3.0M）"}
        ])
    );
    let svc =
        DownloadService::with_backend(f.session.clone(), f.media.clone(), 1024 * 1024).unwrap();
    assert!(svc.list().unwrap().is_empty());
    assert_eq!(http.calls().len(), 2);
    let receipt = plan.receipt.unwrap();
    assert!(matches!(
        client
            .download_command(&epoch, confirm(&receipt, "lossless"))
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(http.calls().len(), 2);
    let result = client
        .download_command(&epoch, confirm(&receipt, "higher"))
        .await
        .unwrap();
    assert_eq!(result.accepted.len(), 1);
    assert!(matches!(
        client
            .download_command(&epoch, confirm(&receipt, "higher"))
            .await,
        Err(Error::StaleResponse)
    ));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let tasks = svc.list().unwrap();
            if tasks
                .first()
                .is_some_and(|task| task.status == DownloadStatus::Complete)
            {
                let task = &tasks[0];
                assert_eq!(task.id, result.accepted[0]);
                assert_eq!(task.song_id, "91");
                assert_eq!(
                    task.origin,
                    DownloadOrigin::Voice {
                        voice_id: "81".into()
                    }
                );
                assert_eq!(task.options.quality.as_str(), "higher");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let calls = http.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [
            RIGHTS,
            MUSICS,
            "/api/dj/program/detail",
            "/api/dj/program/detail",
            RIGHTS,
            "/api/song/enhance/download/url/v1",
        ]
    );
    // The domain passes arrays; M/48.f form normalization encodes the
    // bracketed numeric list as a string inside the signed EAPI payload.
    assert_eq!(calls[0].payload["voiceIdsStr"], "[81]");
    assert_eq!(calls[1].payload["ids"], "[81]");
    assert_eq!(calls[5].payload["id"], "91");
    assert_eq!(calls[5].payload["level"], "higher");
    assert!(calls.iter().all(|call| {
        call.cookie
            .as_deref()
            .unwrap_or("")
            .contains("MUSIC_U=synthetic-voice-quality")
    }));
}

#[tokio::test]
async fn voice_quality_unavailable_never_uses_saved_options_and_batch_is_fixed() {
    let f = Fixture::new(vec![
        permission(false),
        permission(true),
        json!({"code":200,"programs":[{"musics":[{"size":2048,"bitrate":0}]}]}).into(),
        permission(true),
        json!({"code":200}).into(),
        Reply::Ready(Err(Error::Transport("fixture failed".into()))),
    ]);
    let client = client(&f);
    for status in ["restricted", "unavailable", "unavailable"] {
        let plan = client
            .download_voice_prepare("1", vec!["81".into()])
            .await
            .unwrap();
        assert!(plan.receipt.is_none() && plan.choices.is_empty());
        assert_eq!(serde_json::to_value(plan).unwrap()["state"], status);
    }
    assert!(matches!(
        client.download_voice_prepare("1", vec!["81".into()]).await,
        Err(Error::Transport(_))
    ));
    assert_eq!(
        f.api
            .calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [RIGHTS, RIGHTS, MUSICS, RIGHTS, MUSICS, RIGHTS]
    );
    let plan = client
        .download_voice_prepare("1", vec!["82".into(), "81".into(), "82".into()])
        .await
        .unwrap();
    assert_eq!(f.api.calls().len(), 6);
    assert_eq!(
        serde_json::to_value(&plan).unwrap()["choices"],
        json!([
            {"quality":"standard","label":"普通（128kbit/s）"},
            {"quality":"higher","label":"较高（192kbit/s）"},
            {"quality":"exhigh","label":"极高（320kbit/s）"}
        ])
    );
    let receipt = plan.receipt.unwrap();
    client.download_voice_discard("1", &receipt).unwrap();
    assert!(matches!(
        client
            .download_command("1", confirm(&receipt, "standard"))
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.service.list().unwrap().is_empty());
}

#[tokio::test]
async fn voice_quality_stale_reads_cannot_replace_new_preparation_or_cross_account() {
    let (held, release) = support::gate();
    let f = Fixture::new(vec![permission(true), held]);
    let client = Arc::new(client(&f));
    let old = {
        let client = client.clone();
        tokio::spawn(async move { client.download_voice_prepare("1", vec!["81".into()]).await })
    };
    f.api.wait_for_calls(2).await;
    let fresh = client
        .download_voice_prepare("1", vec!["82".into(), "83".into()])
        .await
        .unwrap();
    release.send(Ok(offered())).unwrap();
    assert!(matches!(old.await.unwrap(), Err(Error::StaleResponse)));
    let fresh = fresh.receipt.unwrap();
    assert!(
        matches!(
            client
                .download_command("1", confirm(&fresh, "lossless"))
                .await,
            Err(Error::InvalidInput(_))
        ),
        "new receipt remained authoritative after late old reply"
    );
    f.session
        .adopt(
            f.api.clone(),
            json!({"userId":2}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let epoch = client.account().unwrap().epoch;
    assert!(matches!(
        client
            .download_command(&epoch, confirm(&fresh, "standard"))
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.api.calls().len(), 2);
}

#[test]
fn voice_quality_confirmation_never_accepts_substituted_ids_or_container() {
    for extra in [
        json!({"voice_ids":["999"]}),
        json!({"container":"ncm"}),
        json!({"options":{"quality":"lossless"}}),
    ] {
        let mut value = json!({
            "kind":"confirm_voice","receipt":"receipt","quality":"standard","tags":false,"cover":false
        });
        value
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        assert!(serde_json::from_value::<DownloadCommand>(value).is_err());
    }
    assert!(
        serde_json::from_value::<DownloadCommand>(
            json!({"kind":"start_voice","voice_ids":["81"],"options":DownloadOptions::default()})
        )
        .is_err()
    );
}

#[tokio::test]
async fn voice_quality_batch_admits_only_frozen_episode_ids_in_original_order() {
    struct Batch;
    impl cumulus_core::protocol::api::Api for Batch {
        fn request<'a>(
            &'a self,
            path: &'a str,
            body: Value,
            cache: bool,
        ) -> cumulus_core::protocol::api::ApiFuture<'a, Value> {
            Box::pin(async move {
                assert!(!cache);
                match path {
                    "/api/dj/program/detail" => {
                        let (id, main) = match body["id"].as_str() {
                            Some("82") => (82, 92),
                            Some("81") => (81, 91),
                            _ => panic!("unexpected episode identity"),
                        };
                        let mut episode = program();
                        episode["id"] = json!(id);
                        episode["mainSong"]["id"] = json!(main);
                        Ok(json!({"code":200,"program":episode}))
                    }
                    RIGHTS => {
                        let id = body["voiceIdsStr"][0].as_str().unwrap();
                        assert!(matches!(id, "81" | "82"));
                        Ok(json!({"code":200,"data":{
                            "allowDownloadVoiceIds":[],
                            "forbidDownloadVoiceIds":[id],
                            "downloadCheckResultList":[{"voiceId":id,"canDownload":false}]
                        }}))
                    }
                    _ => panic!("batch preparation/denied worker issued unexpected read: {path}"),
                }
            })
        }
    }

    let f = Fixture::new(vec![]);
    f.session
        .adopt(
            Arc::new(Batch),
            json!({"userId":1}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let client = client(&f);
    let epoch = client.account().unwrap().epoch;
    client
        .download_command(
            &epoch,
            DownloadCommand::Configure {
                root: f
                    .root
                    .path()
                    .join("batch-output")
                    .to_string_lossy()
                    .into_owned(),
                path_type: 0,
                file_type: 1,
            },
        )
        .await
        .unwrap();
    let plan = client
        .download_voice_prepare(&epoch, vec!["82".into(), "81".into(), "82".into()])
        .await
        .unwrap();
    let result = client
        .download_command(&epoch, confirm(&plan.receipt.unwrap(), "exhigh"))
        .await
        .unwrap();
    assert!(result.failed.is_empty());
    let svc =
        DownloadService::with_backend(f.session.clone(), f.media.clone(), 1024 * 1024).unwrap();
    let tasks = svc.list().unwrap();
    let admitted: Vec<_> = result
        .accepted
        .iter()
        .map(|id| {
            let task = tasks.iter().find(|task| task.id == *id).unwrap();
            assert_eq!(task.options.quality.as_str(), "exhigh");
            (&task.origin, task.song_id.as_str())
        })
        .collect();
    assert_eq!(
        admitted,
        [
            (
                &DownloadOrigin::Voice {
                    voice_id: "82".into()
                },
                "92"
            ),
            (
                &DownloadOrigin::Voice {
                    voice_id: "81".into()
                },
                "91"
            ),
        ]
    );
}
