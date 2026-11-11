use super::*;
use cumulus_core::{
    Error,
    account::profiles::ProfileRegistry,
    client::{
        MusicClient,
        dto::{DownloadCommand, HistoryRowMenuTarget, HistorySelectionScope, HistoryVoiceFileMenu},
    },
    history::{HistoryKind, HistoryRecord, HistoryService},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use std::{path::PathBuf, sync::Mutex, time::Duration};

const DETAIL: &str = "/api/dj/program/detail";
const RIGHTS: &str = "/api/dj/voices/download/check";
const MEDIA: &str = "/api/song/enhance/download/url/v1";
const MUSICS: &str = "/api/dj/program/song/musics";

fn episode(voice: u64, media: u64) -> Value {
    let mut value = program();
    value["id"] = json!(voice);
    value["mainSong"]["id"] = json!(media);
    value["name"] = json!(format!("Episode {voice}"));
    value
}

fn record(f: &Fixture, voice: u64, media: u64, played_at: u64) {
    let token = f.session.token().unwrap();
    HistoryService::new(f.session.clone())
        .unwrap()
        .record_for(
            &token,
            HistoryKind::Voice,
            HistoryRecord::local(HistoryKind::Voice, episode(voice, media), played_at).unwrap(),
        )
        .unwrap();
}

fn client(f: &Fixture) -> MusicClient {
    MusicClient::with_session(
        f.session.clone(),
        ProfileRegistry::open(&f.root.path().join("state"), ApiOrigin::Pc).unwrap(),
    )
    .with_download_backend(f.media.clone())
}

async fn configure(f: &Fixture, client: &MusicClient, epoch: &str) -> PathBuf {
    let output = f.root.path().join("history-output");
    client
        .download_command(
            epoch,
            DownloadCommand::Configure {
                root: output.to_string_lossy().into_owned(),
                path_type: 0,
                file_type: 1,
            },
        )
        .await
        .unwrap();
    output.canonicalize().unwrap()
}

fn confirm(receipt: &str) -> DownloadCommand {
    serde_json::from_value(json!({
        "kind":"confirm_voice","receipt":receipt,"quality":"higher","tags":false,"cover":false
    }))
    .unwrap()
}

async fn selection(client: &MusicClient, epoch: &str, order: &[&str]) -> HistorySelectionScope {
    let page = client
        .history_page(epoch, HistoryKind::Voice, false)
        .await
        .unwrap()
        .value;
    let ids: Vec<_> = page.entries.into_iter().map(|row| row.id).collect();
    assert!(ids.iter().map(String::as_str).eq(order.iter().copied()));
    HistorySelectionScope {
        revision: page.revision,
        ids,
    }
}

fn row_menu(revision: &str, clicked: &str, gesture: &[&str]) -> HistoryRowMenuTarget {
    HistoryRowMenuTarget {
        history_kind: HistoryKind::Voice,
        revision: revision.into(),
        clicked_id: clicked.into(),
        selected_ids: gesture.iter().map(|id| (*id).into()).collect(),
    }
}

#[tokio::test]
async fn history_voice_quality_checks_display_order_and_discard_or_revision_before_create() {
    let f = Fixture::new(vec![]);
    let client = client(&f);
    configure(&f, &client, "1").await;
    record(&f, 82, 92, 200);
    record(&f, 81, 91, 100);
    let target = selection(&client, "1", &["82", "81"]).await;

    for ids in [["81", "82"], ["82", "82"], ["82", "999"]] {
        assert!(matches!(
            client
                .download_history_voice_prepare(
                    "1",
                    HistorySelectionScope {
                        revision: target.revision.clone(),
                        ids: ids.into_iter().map(str::to_owned).collect(),
                    }
                )
                .await,
            Err(Error::QueueChanged)
        ));
    }
    assert!(matches!(
        client
            .download_history_voice_prepare(
                "1",
                HistorySelectionScope {
                    revision: target.revision.clone(),
                    ids: vec![],
                }
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        client
            .download_history_voice_prepare(
                "1",
                HistorySelectionScope {
                    revision: "stale".into(),
                    ..target.clone()
                }
            )
            .await,
        Err(Error::QueueChanged)
    ));
    assert!(matches!(
        client
            .download_history_voice_prepare("0", target.clone())
            .await,
        Err(Error::StaleResponse)
    ));

    let prepared = client
        .download_history_voice_prepare("1", target.clone())
        .await
        .unwrap();
    assert_eq!(prepared.admitted_count, Some(2));
    assert_eq!(
        serde_json::to_value(&prepared).unwrap()["choices"],
        json!([
            {"quality":"standard","label":"普通（128kbit/s）"},
            {"quality":"higher","label":"较高（192kbit/s）"},
            {"quality":"exhigh","label":"极高（320kbit/s）"}
        ])
    );
    let receipt = prepared.receipt.unwrap();
    client.download_voice_discard("1", &receipt).unwrap();
    assert!(matches!(
        client.download_command("1", confirm(&receipt)).await,
        Err(Error::StaleResponse)
    ));

    let receipt = client
        .download_history_voice_prepare("1", target)
        .await
        .unwrap()
        .receipt
        .unwrap();
    // Replaying the same episode changes only its position in the displayed
    // history. An old confirmation still cannot create an unchanged task.
    record(&f, 81, 91, 300);
    selection(&client, "1", &["81", "82"]).await;
    assert!(matches!(
        client.download_command("1", confirm(&receipt)).await,
        Err(Error::QueueChanged)
    ));
    assert!(f.api.calls().is_empty());
    assert!(f.service.list().unwrap().is_empty());
    assert!(f.media.audio.calls().is_empty() && f.media.cover.calls().is_empty());
}

#[tokio::test]
async fn history_voice_confirmation_rechecks_history_after_held_detail_before_creating() {
    let (held, release) = support::gate();
    let f = Fixture::new(vec![held]);
    let client = Arc::new(client(&f));
    let output = configure(&f, &client, "1").await;
    record(&f, 81, 91, 100);
    record(&f, 82, 92, 200);
    let target = selection(&client, "1", &["82", "81"]).await;
    let receipt = client
        .download_history_voice_prepare("1", target)
        .await
        .unwrap()
        .receipt
        .unwrap();
    assert!(f.api.calls().is_empty());
    let submit = client.clone();
    let in_flight = receipt.clone();
    let pending =
        tokio::spawn(async move { submit.download_command("1", confirm(&in_flight)).await });
    f.api.wait_for_calls(1).await;
    record(&f, 83, 93, 300);
    release
        .send(Ok(json!({"code":200,"program":episode(82, 92)})))
        .unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
    assert!(matches!(
        client.download_command("1", confirm(&receipt)).await,
        Err(Error::StaleResponse)
    ));
    let calls = f.api.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, DETAIL);
    assert_eq!(calls[0].payload["id"], "82");
    assert!(f.service.list().unwrap().is_empty());
    assert!(f.media.audio.calls().is_empty() && f.media.cover.calls().is_empty());
    assert!(std::fs::read_dir(output).unwrap().next().is_none());
}

#[tokio::test]
async fn history_voice_row_download_uses_gesture_order_and_rejects_a_changed_history() {
    let (held, release) = support::gate();
    let f = Fixture::new(vec![held]);
    let client = Arc::new(client(&f));
    let output = configure(&f, &client, "1").await;
    record(&f, 81, 91, 100);
    record(&f, 82, 92, 200);
    record(&f, 83, 93, 300);
    let scope = selection(&client, "1", &["83", "82", "81"]).await;
    assert!(matches!(
        client
            .download_history_row_voice_prepare(
                "1",
                row_menu(&scope.revision, "999", &["81", "82"])
            )
            .await,
        Err(Error::QueueChanged)
    ));
    // Modifier right-click keeps the existing two rows even though it
    // anchors the menu on a different programme. The first selected row
    // was gestured last in display order and must be the first admission.
    let prepared = client
        .download_history_row_voice_prepare("1", row_menu(&scope.revision, "83", &["81", "82"]))
        .await
        .unwrap();
    assert_eq!(prepared.admitted_count, Some(2));
    assert_eq!(prepared.choices.len(), 3);
    assert!(f.api.calls().is_empty());
    let receipt = prepared.receipt.unwrap();
    let submit = client.clone();
    let pending =
        tokio::spawn(async move { submit.download_command("1", confirm(&receipt)).await });
    f.api.wait_for_calls(1).await;
    record(&f, 82, 92, 400);
    release
        .send(Ok(json!({"code":200,"program":episode(81, 91)})))
        .unwrap();
    assert!(matches!(pending.await.unwrap(), Err(Error::QueueChanged)));
    let calls = f.api.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].path, DETAIL);
    assert_eq!(calls[0].payload, json!({"id":"81"}));
    assert!(f.service.list().unwrap().is_empty());
    assert!(f.media.audio.calls().is_empty());
    assert!(std::fs::read_dir(output).unwrap().next().is_none());
}

#[tokio::test]
async fn history_voice_row_reveals_only_the_exact_clicked_publication_and_filters_group() {
    const COMMENTS: &str = "/api/resource/commentInfo/list";
    let count = || json!({"code":200,"data":[{"threadId":"A_DJ_1_81","commentCount":27}]}).into();
    let f = Fixture::new(vec![
        detail(),
        detail(),
        permission(true),
        resource(),
        count(),
        count(),
        json!({"code":200,"data":{"allowDownloadVoiceIds":["82"],"forbidDownloadVoiceIds":[],
            "downloadCheckResultList":[{"voiceId":"82","canDownload":true}]}})
        .into(),
        json!({"code":200,"programs":[{"musics":[{"bitrate":192000,"size":1048576}]}]}).into(),
    ]);
    let task = f
        .service
        .create_voice(
            &json!(81),
            f.destination(),
            DownloadOptions {
                tags: false,
                cover: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let task = f
        .service
        .run(&task.id, DownloadRunOptions::default())
        .await
        .unwrap();
    let path = task.target.as_ref().unwrap();
    assert_eq!(task.status, DownloadStatus::Complete);
    let client = client(&f);
    configure(&f, &client, "1").await;
    record(&f, 81, 91, 100);
    record(&f, 82, 92, 200);
    let scope = selection(&client, "1", &["82", "81"]).await;
    let completed = row_menu(&scope.revision, "81", &["82", "81"]);
    let other = row_menu(&scope.revision, "82", &["82", "81"]);
    assert!(matches!(
        client
            .download_history_row_voice_prepare("1", completed.clone())
            .await,
        Err(Error::QueueChanged)
    ));

    let offer = client
        .history_row_menu_info("1", completed.clone())
        .await
        .unwrap();
    let Some(HistoryVoiceFileMenu::Reveal { receipt }) = offer.value.voice_file else {
        panic!("expected a native reveal offer");
    };
    assert!(
        client
            .history_row_voice_file_reveal("1", other.clone(), &receipt)
            .await
            .is_err()
    );
    let lease = client
        .history_row_voice_file_reveal("1", completed.clone(), &receipt)
        .await
        .unwrap();
    assert_eq!(lease.path(), path);
    assert!(lease.check_current(&client).is_ok());
    assert!(
        client
            .history_row_voice_file_reveal("1", completed.clone(), &receipt)
            .await
            .is_err()
    );

    let offer = client
        .history_row_menu_info("1", completed.clone())
        .await
        .unwrap();
    let Some(HistoryVoiceFileMenu::Reveal { receipt }) = offer.value.voice_file else {
        panic!("expected another reveal offer");
    };
    let mut bytes = std::fs::read(path).unwrap();
    bytes[100] ^= 1;
    std::fs::write(path, bytes).unwrap();
    assert!(matches!(
        client
            .history_row_voice_file_reveal("1", completed, &receipt)
            .await,
        Err(Error::DownloadedFileMissing)
    ));

    // Pointing at the uncompleted row preserves the same group, but only
    // its single surviving programme is eligible for quality preparation.
    let prepared = client
        .download_history_row_voice_prepare("1", other)
        .await
        .unwrap();
    assert_eq!(prepared.admitted_count, Some(1));
    assert_eq!(
        serde_json::to_value(&prepared).unwrap()["choices"],
        json!([{"quality":"higher","label":"较高（1.0M）"}])
    );
    client
        .download_voice_discard("1", &prepared.receipt.unwrap())
        .unwrap();
    assert_eq!(f.service.list().unwrap().len(), 1);
    let calls = f.api.calls();
    let tail = &calls[4..];
    assert_eq!(
        tail.iter().map(|c| c.path.as_str()).collect::<Vec<_>>(),
        [COMMENTS, COMMENTS, RIGHTS, MUSICS]
    );
    assert_eq!(tail[2].payload, json!({"voiceIdsStr":["82"]}));
    assert_eq!(tail[3].payload, json!({"ids":["82"]}));
}

#[derive(Default)]
struct BatchApi {
    calls: Mutex<Vec<(&'static str, u64)>>,
}

fn numeric(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .unwrap()
}

impl Api for BatchApi {
    fn request<'a>(&'a self, path: &'a str, body: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert!(!cache);
            // Authorize and create can interleave across tasks; each reply is
            // keyed by its actual request rather than by a scheduler order.
            let (endpoint, id, response) = match path {
                DETAIL => {
                    let id = numeric(&body["id"]);
                    assert!(matches!(id, 82 | 83));
                    (
                        DETAIL,
                        id,
                        json!({"code":200,"program":episode(id, id + 10)}),
                    )
                }
                RIGHTS => {
                    let ids = body["voiceIdsStr"].as_array().unwrap();
                    assert_eq!(ids.len(), 1);
                    let id = numeric(&ids[0]);
                    assert!(matches!(id, 82 | 83));
                    (
                        RIGHTS,
                        id,
                        json!({"code":200,"data":{
                            "allowDownloadVoiceIds":[id],
                            "forbidDownloadVoiceIds":[],
                            "downloadCheckResultList":[{"voiceId":id,"canDownload":true}]
                        }}),
                    )
                }
                MEDIA => {
                    let id = numeric(&body["id"]);
                    assert!(matches!(id, 92 | 93));
                    assert_eq!(body["level"], "higher");
                    let Reply::Ready(Ok(mut value)) = resource() else {
                        unreachable!()
                    };
                    value["data"]["id"] = json!(id);
                    value["data"]["level"] = json!("higher");
                    (MEDIA, id, value)
                }
                _ => panic!("unexpected history voice endpoint: {path}"),
            };
            self.calls.lock().unwrap().push((endpoint, id));
            Ok(response)
        })
    }
}

#[tokio::test]
async fn history_voice_confirmation_filters_owned_download_and_uses_current_episode_media() {
    let f = Fixture::new(vec![detail(), detail(), permission(true), resource()]);
    let existing = f
        .service
        .create_voice(
            &json!(81),
            f.destination(),
            DownloadOptions {
                tags: false,
                cover: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let existing = f
        .service
        .run(&existing.id, DownloadRunOptions::default())
        .await
        .unwrap();
    assert_eq!(existing.status, DownloadStatus::Complete);
    let prior_media = f.media.audio.calls().len();

    let api = Arc::new(BatchApi::default());
    let old = f.session.token().unwrap();
    f.session
        .adopt(api.clone(), json!({"userId":1}), &old)
        .unwrap();
    let client = client(&f);
    let epoch = client.account().unwrap().epoch;
    let output = configure(&f, &client, &epoch).await;
    let service =
        DownloadService::with_backend(f.session.clone(), f.media.clone(), 1024 * 1024).unwrap();
    record(&f, 82, 892, 100);
    record(&f, 81, 91, 200);
    record(&f, 83, 893, 300);
    let target = selection(&client, &epoch, &["83", "81", "82"]).await;
    let prepared = client
        .download_history_voice_prepare(&epoch, target)
        .await
        .unwrap();
    assert_eq!(prepared.admitted_count, Some(2));
    assert!(api.calls.lock().unwrap().is_empty());
    let receipt = prepared.receipt.unwrap();
    let result = client
        .download_command(&epoch, confirm(&receipt))
        .await
        .unwrap();
    assert!(result.failed.is_empty());
    assert_eq!(result.accepted.len(), 2);
    assert!(matches!(
        client.download_command(&epoch, confirm(&receipt)).await,
        Err(Error::StaleResponse)
    ));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let tasks: Vec<_> = result
                .accepted
                .iter()
                .map(|id| service.get(id).unwrap())
                .collect();
            for task in &tasks {
                assert_ne!(task.status, DownloadStatus::Error);
            }
            if tasks
                .iter()
                .all(|task| task.status == DownloadStatus::Complete)
            {
                for (task, (voice, media)) in tasks.iter().zip([("83", "93"), ("82", "92")]) {
                    assert_eq!(
                        task.origin,
                        DownloadOrigin::Voice {
                            voice_id: voice.into()
                        }
                    );
                    assert_eq!(task.song_id, media);
                    assert_eq!(task.options.quality.as_str(), "higher");
                    let file = task.target.as_ref().unwrap();
                    assert!(file.starts_with(&output) && file.is_file());
                }
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(service.list().unwrap().len(), 3);
    let retained = service.get(&existing.id).unwrap();
    assert_eq!(retained.authorization_count, existing.authorization_count);
    assert_eq!(retained.target, existing.target);
    assert!(retained.target.unwrap().is_file());
    assert_eq!(f.api.calls().len(), 4);
    assert_eq!(f.media.audio.calls().len(), prior_media + 2);
    assert!(f.media.cover.calls().is_empty());
    let mut calls = api.calls.lock().unwrap().clone();
    calls.sort_unstable();
    let mut expected = vec![
        (DETAIL, 82),
        (DETAIL, 82),
        (DETAIL, 83),
        (DETAIL, 83),
        (RIGHTS, 82),
        (RIGHTS, 83),
        (MEDIA, 92),
        (MEDIA, 93),
    ];
    expected.sort_unstable();
    assert_eq!(calls, expected);
}
