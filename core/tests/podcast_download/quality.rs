use super::*;
use cumulus_core::{
    Error,
    account::{credentials::Credentials, profiles::ProfileRegistry},
    client::{
        MusicClient,
        dto::{DownloadCommand, PodcastLikedPage, PodcastLikedPageTarget, PodcastLikedQuery},
    },
    podcast::{PodcastAction, PodcastService},
    protocol::http::{ApiOrigin, ClientProfile, MusicHttpClient, RequestOptions, RoutingPolicy},
};
use support::http::ScriptHttp;

const RIGHTS: &str = "/api/dj/voices/download/check";
const MUSICS: &str = "/api/dj/program/song/musics";

fn offered() -> Value {
    json!({"code":200,"programs":[{"musics":[
        {"id":91,"bitrate":320000},
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
            {"quality":"exhigh","label":"极高（0b）"}
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

const LIKED: &str = "/api/content/my/liked/voice";
const DETAIL: &str = "/api/dj/program/detail";

fn liked_response(episodes: Vec<Value>) -> Reply {
    json!({"code":200,"data":{"data":episodes,"hasMore":false,"count":3}}).into()
}

fn reply_value(reply: Reply) -> Value {
    let Reply::Ready(Ok(value)) = reply else {
        panic!("not a ready fixture reply")
    };
    value
}

async fn open_liked(client: &MusicClient) -> PodcastLikedPage {
    let opened = client
        .podcast_liked_page("1", PodcastLikedQuery::Open {})
        .await
        .unwrap()
        .value;
    client
        .podcast_liked_page(
            "1",
            PodcastLikedQuery::Next {
                visit: opened.visit,
                revision: opened.revision,
                continuation: opened.continuation.unwrap(),
            },
        )
        .await
        .unwrap()
        .value
}

fn liked_target(page: &PodcastLikedPage, voice_id: &str) -> PodcastLikedPageTarget {
    PodcastLikedPageTarget {
        visit: page.visit.clone(),
        revision: page.revision.clone(),
        voice_id: voice_id.into(),
    }
}

async fn suspend_liked(client: &MusicClient, page: &PodcastLikedPage) {
    client
        .podcast_liked_page(
            "1",
            PodcastLikedQuery::Suspend {
                visit: page.visit.clone(),
                revision: page.revision.clone(),
            },
        )
        .await
        .unwrap();
}

async fn directory(f: &Fixture, client: &MusicClient) -> std::path::PathBuf {
    let output = f.root.path().join("paged-liked-output");
    client
        .download_command(
            "1",
            DownloadCommand::Configure {
                root: output.to_string_lossy().into_owned(),
                path_type: 0,
                file_type: 1,
            },
        )
        .await
        .unwrap();
    output
}

fn paths(f: &Fixture) -> Vec<String> {
    f.api.calls().into_iter().map(|call| call.path).collect()
}

#[tokio::test]
async fn liked_page_quality_rejects_unknown_foreign_withdrawn_and_gray_rows_before_reading_rights()
{
    let mut withdrawn = program();
    withdrawn["id"] = json!(82);
    withdrawn["displayStatus"] = json!("UNDER_SHELF");
    let mut gray = program();
    gray["id"] = json!(83);
    gray["grayDisplay"] = json!(true);
    let f = Fixture::new(vec![liked_response(vec![program(), withdrawn, gray])]);
    let host = client(&f);
    let foreign = client(&f);
    let page = open_liked(&host).await;
    for id in ["82", "83", "999"] {
        assert!(matches!(
            host.download_liked_page_voice_prepare("1", liked_target(&page, id))
                .await,
            Err(Error::QueueChanged)
        ));
    }
    assert!(matches!(
        foreign
            .download_liked_page_voice_prepare("1", liked_target(&page, "81"))
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        host.download_liked_page_voice_prepare("0", liked_target(&page, "81"))
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(paths(&f), [LIKED]);
    assert!(
        host.podcast_state("1", false)
            .await
            .unwrap()
            .value
            .liked_ids
            .is_none()
    );
    assert!(f.service.list().unwrap().is_empty());
}

#[tokio::test]
async fn liked_page_quality_rechecks_after_each_held_preparation_read() {
    for hold_musics in [false, true] {
        let (held, release) = support::gate();
        let mut replies = vec![liked_response(vec![program()])];
        if hold_musics {
            replies.push(permission(true));
        }
        replies.push(held);
        let f = Fixture::new(replies);
        let client = Arc::new(client(&f));
        let page = open_liked(&client).await;
        let target = liked_target(&page, "81");
        let reader = client.clone();
        let pending =
            tokio::spawn(
                async move { reader.download_liked_page_voice_prepare("1", target).await },
            );
        f.api.wait_for_calls(if hold_musics { 3 } else { 2 }).await;
        suspend_liked(&client, &page).await;
        release
            .send(Ok(if hold_musics {
                offered()
            } else {
                reply_value(permission(true))
            }))
            .unwrap();
        assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
        let expected = if hold_musics {
            vec![LIKED, RIGHTS, MUSICS]
        } else {
            vec![LIKED, RIGHTS]
        };
        assert_eq!(paths(&f), expected);
        assert!(f.service.list().unwrap().is_empty());
    }
}

#[tokio::test]
async fn liked_page_quality_cannot_commit_after_held_detail_and_route_or_generation_revocation() {
    for mutation in [false, true] {
        let (held, release) = support::gate();
        let mut replies = vec![
            liked_response(vec![program()]),
            permission(true),
            offered().into(),
            held,
        ];
        if mutation {
            replies.push(json!({"code":200,"data":{"resumePlayValue":0}}).into());
        }
        let f = Fixture::new(replies);
        let client = Arc::new(client(&f));
        let output = directory(&f, &client).await;
        let page = open_liked(&client).await;
        let receipt = client
            .download_liked_page_voice_prepare("1", liked_target(&page, "81"))
            .await
            .unwrap()
            .receipt
            .unwrap();
        let submitter = client.clone();
        let pending_receipt = receipt.clone();
        let pending = tokio::spawn(async move {
            submitter
                .download_command("1", confirm(&pending_receipt, "standard"))
                .await
        });
        f.api.wait_for_calls(4).await;
        if mutation {
            PodcastService::new(f.session.clone())
                .unwrap()
                .change_for(
                    &f.session.token().unwrap(),
                    PodcastAction::Resume,
                    "71",
                    false,
                )
                .await
                .unwrap();
        } else {
            suspend_liked(&client, &page).await;
            // An actual Back can reactivate the same prefix, but not an old confirmation.
            client
                .podcast_liked_page(
                    "1",
                    PodcastLikedQuery::Restore {
                        visit: page.visit.clone(),
                        revision: page.revision.clone(),
                    },
                )
                .await
                .unwrap();
        }
        release.send(Ok(reply_value(detail()))).unwrap();
        assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
        assert!(matches!(
            client
                .download_command("1", confirm(&receipt, "standard"))
                .await,
            Err(Error::StaleResponse)
        ));
        assert!(f.service.list().unwrap().is_empty());
        assert!(f.media.audio.calls().is_empty() && f.media.cover.calls().is_empty());
        assert!(
            std::fs::read_dir(output).unwrap().next().is_none(),
            "no staged directory left"
        );
        assert_eq!(&paths(&f)[..4], [LIKED, RIGHTS, MUSICS, DETAIL]);
        assert_eq!(paths(&f).len(), if mutation { 5 } else { 4 });
    }
}

#[tokio::test]
async fn liked_page_quality_accepts_only_exact_live_episode_and_rejects_changed_detail() {
    for change in ["available", "withdrawn", "gray", "media"] {
        let mut selected = program();
        match change {
            "withdrawn" => selected["displayStatus"] = json!("UNDER_SHELF"),
            "gray" => selected["grayDisplay"] = json!(true),
            "media" => selected["mainSong"]["id"] = json!(92),
            _ => (),
        }
        let mut replies = vec![
            liked_response(vec![program()]),
            permission(true),
            offered().into(),
            json!({"code":200,"program":selected}).into(),
        ];
        if change == "available" {
            replies.extend([detail(), permission(true), resource()]);
        }
        let f = Fixture::new(replies);
        let client = client(&f);
        let _output = directory(&f, &client).await;
        let page = open_liked(&client).await;
        let receipt = client
            .download_liked_page_voice_prepare("1", liked_target(&page, "81"))
            .await
            .unwrap()
            .receipt
            .unwrap();
        assert!(f.service.list().unwrap().is_empty());
        let result = client
            .download_command("1", confirm(&receipt, "standard"))
            .await;
        if change != "available" {
            assert!(matches!(result, Err(Error::QueueChanged)), "{change}");
            assert!(f.service.list().unwrap().is_empty());
            assert_eq!(paths(&f), [LIKED, RIGHTS, MUSICS, DETAIL]);
            continue;
        }
        let result = result.unwrap();
        assert!(result.failed.is_empty());
        assert_eq!(result.accepted.len(), 1);
        let downloaded = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let tasks = f.service.list().unwrap();
                if let Some(task) = tasks
                    .into_iter()
                    .find(|task| task.status == DownloadStatus::Complete)
                {
                    break task;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(downloaded.id, result.accepted[0]);
        assert_eq!(
            downloaded.origin,
            DownloadOrigin::Voice {
                voice_id: "81".into()
            }
        );
        assert_eq!(downloaded.song_id, "91");
        assert_eq!(std::fs::read(downloaded.target.unwrap()).unwrap(), AUDIO);
        let calls = f.api.calls();
        assert_eq!(
            paths(&f),
            [
                LIKED,
                RIGHTS,
                MUSICS,
                DETAIL,
                DETAIL,
                RIGHTS,
                "/api/song/enhance/download/url/v1"
            ]
        );
        assert_eq!(calls[1].payload, json!({"voiceIdsStr":["81"]}));
        assert_eq!(calls[2].payload, json!({"ids":["81"]}));
        assert_eq!(calls[6].payload["id"], "91");
    }
}
