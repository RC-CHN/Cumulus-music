use super::{
    ApiOrigin, CARD, COOKIE, Error, Fixture, HttpResponse, PlayerAction, Quality, STARTUP, json,
};
use cumulus_core::{
    client::dto::{DownloadCommand, SongDownloadState, VoiceSongGroupDownloadDecision as D},
    download::service::{DownloadOptions, DownloadService},
    library::{AudioMetadata, ListOptions, LocalLibrary, probe::AudioProbe},
};
use std::{path::Path, sync::Arc, time::Duration};

#[path = "group_download/purchased.rs"]
mod purchased;

const LIKED: &str = "/api/song/like/get";
const AUTH: &str = "/api/song/enhance/download/url/v1";
const TASKS: &str = "download_tasks_v1";
const BATCH: &str = "download_song_batch_v1";

fn ordinary(id: u64) -> serde_json::Value {
    json!({"id":id,"name":"合成普通歌曲","album":{},"privilege":{"id":id,"fee":0,"st":0,"dl":320000}})
}

fn card() -> HttpResponse {
    let mut rows: Vec<_> = (301..=309).map(ordinary).collect();
    rows[2]["privilege"]["st"] = json!(-100);
    rows[6]["privilege"]["dl"] = json!(0);
    rows[6]["privilege"]["pl"] = json!(320000);
    rows[7]["privilege"]["payed"] = json!(2);
    rows[8]["privilege"]["cs"] = json!(1);
    rows.push(ordinary(301));
    let purchased = |id, maxbr, flag| {
        json!({"id":id,"name":"合成已购歌曲","album":{},"privilege":{
            "id":id,"fee":4,"st":0,"dl":128000,"payed":2,"maxbr":maxbr,"flag":flag
        }})
    };
    rows.push(purchased(310, 320000, (1 << 12) | (1 << 21)));
    rows.push(purchased(311, 128000, 1 << 18));
    let mut unbought = purchased(312, 999000, 1 << 18);
    unbought["privilege"]["payed"] = json!(0);
    rows.push(unbought);
    rows.push(purchased(313, 999000, 1 << 16));
    json!({"code":200,"tracks":rows}).into()
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

async fn fixture(replies: impl IntoIterator<Item = HttpResponse>) -> (Fixture, String) {
    let f = Fixture::new(&[COOKIE], [card()].into_iter().chain(replies)).await;
    super::picker::config(&f);
    let view = f
        .client
        .player_voice_songs(&f.epoch, &f.play_id)
        .await
        .unwrap();
    let hints: Vec<_> = view
        .availability
        .iter()
        .map(|row| {
            serde_json::to_value(row).unwrap()["groupDownload"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        hints,
        [
            "ordinary",
            "ordinary",
            "omitted",
            "ordinary",
            "ordinary",
            "ordinary",
            "omitted",
            "purchased",
            "unsupported",
            "ordinary",
            "purchased",
            "purchased",
            "unsupported",
            "purchased",
        ]
    );
    let receipt = view.receipt.unwrap();
    (f, receipt)
}

struct Probe;
impl AudioProbe for Probe {
    fn inspect(&self, _: &Path) -> cumulus_core::Result<AudioMetadata> {
        Ok(AudioMetadata {
            format: Some("wav".into()),
            duration: 1.0,
            title: "合成本地歌曲".into(),
            album: String::new(),
            artists: vec![],
            bitrate: None,
            sample_rate: Some(44100),
            channels: Some(2),
            container: "wav".into(),
            cover_bytes: None,
        })
    }
}

fn add_local(f: &Fixture, id: u64) {
    let path = f.root.path().join("local.wav");
    std::fs::write(&path, b"fixture audio identity").unwrap();
    let library = LocalLibrary::with_probe(f.store.clone(), Arc::new(Probe)).unwrap();
    assert_eq!(library.scan(path).unwrap().updated, 1);
    let item = library.list(ListOptions::default()).unwrap().pop().unwrap();
    library
        .map_online(&f.session, &item.id, &json!(id))
        .unwrap();
}

fn existing(f: &Fixture, id: u64, status: &str) -> String {
    let task = DownloadService::new(f.session.clone())
        .unwrap()
        .create_default(&json!(id), DownloadOptions::default())
        .unwrap();
    let mut raw = serde_json::to_value(&task).unwrap();
    raw["status"] = json!(status);
    f.store.put("25", TASKS, &task.id, &raw).unwrap();
    task.id
}

#[tokio::test]
async fn group_download_uses_filtered_order_only_clicked_liked_anchor_and_saved_quality() {
    let denial = json!({"code":200,"data":{"code":403}});
    let (f, card) = fixture([
        json!({"code":200,"ids":[303]}).into(),
        denial.clone().into(),
        denial.into(),
    ])
    .await;
    f.client.liked_ids().await.unwrap();
    configure(&f).await;
    add_local(&f, 304);
    existing(&f, 305, "complete");
    existing(&f, 306, "paused");

    let before = serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap();
    let order = [1, 4, 9, 6, 3, 0, 5, 2];
    let plan = f
        .client
        .player_voice_songs_download_prepare(&f.epoch, &f.play_id, &card, 2, &order)
        .await
        .unwrap();
    assert_eq!(plan.decision, D::Ready);
    assert_eq!(plan.anchor_index, 2);
    assert_eq!(plan.eligible_indices, [1, 9]);
    assert_eq!(plan.omitted_indices, [4, 6, 3, 0, 5, 2]);
    assert_eq!(f.store.values("25", TASKS).unwrap().len(), 2);
    assert_eq!(f.http.calls().len(), STARTUP.len() + 2);

    // A later saved default applies at the explicit native start, without a
    // renderer choice, and is independent of the liked anchor's availability.
    let saved = f.client.download_quality_preferences(&f.epoch).unwrap();
    let mut options = saved.options;
    options.quality = Quality::ExHigh;
    options.cover = false;
    f.client
        .download_command(
            &f.epoch,
            DownloadCommand::Preferences {
                revision: saved.revision,
                options,
            },
        )
        .await
        .unwrap();
    let receipt = plan.selection_receipt.unwrap();
    let start = f
        .client
        .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &receipt)
        .await
        .unwrap();
    assert_eq!(start.operation_id.as_deref(), Some(receipt.as_str()));
    assert_eq!(start.accepted.len(), 2);
    let all = DownloadService::new(f.session.clone())
        .unwrap()
        .list()
        .unwrap();
    let new: Vec<_> = all
        .iter()
        .filter(|task| start.accepted.contains(&task.id))
        .collect();
    assert_eq!(
        new.iter().map(|task| task.id.as_str()).collect::<Vec<_>>(),
        start.accepted
    );
    assert_eq!(
        new.iter()
            .map(|task| task.song_id.as_str())
            .collect::<Vec<_>>(),
        ["302", "301"]
    );
    assert!(
        new.iter()
            .all(|task| task.options.quality == Quality::ExHigh && !task.options.cover)
    );

    f.http.wait(STARTUP.len() + 4).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if f.client.song_download_state(&f.epoch, "301").await.unwrap()
                == SongDownloadState::Failed
                && f.client.song_download_state(&f.epoch, "302").await.unwrap()
                    == SongDownloadState::Failed
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let calls = f.http.calls();
    assert_eq!(&calls[STARTUP.len()].path, CARD);
    assert_eq!(&calls[STARTUP.len() + 1].path, LIKED);
    let mut authorized = calls[STARTUP.len() + 2..]
        .iter()
        .map(|call| {
            assert_eq!(call.path, AUTH);
            assert_eq!(call.payload["level"], "exhigh");
            assert_eq!(call.payload["immerseType"], "c51");
            assert!(
                call.cookie
                    .as_deref()
                    .unwrap()
                    .contains("MUSIC_U=synthetic-voice")
            );
            call.payload["id"].as_str().unwrap()
        })
        .collect::<Vec<_>>();
    authorized.sort_unstable();
    assert_eq!(authorized, ["301", "302"]);
    let replay = f
        .client
        .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &receipt)
        .await
        .unwrap();
    assert_eq!(replay.accepted, start.accepted);
    assert_eq!(f.store.values("25", TASKS).unwrap().len(), 4);
    assert_eq!(f.store.values("25", BATCH).unwrap().len(), 1);
    assert_eq!(f.http.calls().len(), calls.len());
    assert!(
        f.store
            .values("25", "resource-mutations")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        before,
        serde_json::to_value(f.client.player_queue(&f.epoch).await.unwrap()).unwrap()
    );
}

#[tokio::test]
async fn group_download_rejects_nonordinary_stale_unliked_or_unconfigured_selections() {
    let (f, card) = fixture([json!({"code":200,"ids":[301]}).into()]).await;
    let prepare = |anchor, rows| {
        f.client
            .player_voice_songs_download_prepare(&f.epoch, &f.play_id, &card, anchor, rows)
    };
    assert_eq!(
        prepare(0, &[0, 1]).await.unwrap().decision,
        D::MembershipUnknown
    );
    f.client.liked_ids().await.unwrap();
    assert_eq!(
        prepare(0, &[0, 1]).await.unwrap().decision,
        D::DirectoryRequired
    );
    configure(&f).await;
    for (anchor, rows, expected) in [
        (1, &[0, 1][..], D::PlaylistRequired),
        (0, &[0, 1, 7], D::UnsupportedSelection),
        (0, &[0, 1, 8], D::UnsupportedSelection),
    ] {
        let plan = prepare(anchor, rows).await.unwrap();
        assert_eq!(plan.decision, expected);
        assert!(plan.selection_receipt.is_none());
    }
    for (anchor, rows) in [(1, &[0, 0][..]), (2, &[0, 1]), (0, &[0, 100])] {
        assert!(matches!(
            prepare(anchor, rows).await,
            Err(Error::InvalidInput(_))
        ));
    }
    let old = prepare(0, &[1, 0])
        .await
        .unwrap()
        .selection_receipt
        .unwrap();
    let current = prepare(0, &[0, 1])
        .await
        .unwrap()
        .selection_receipt
        .unwrap();
    assert!(matches!(
        f.client
            .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &old)
            .await,
        Err(Error::StaleResponse)
    ));
    f.client
        .player_voice_songs_download_cancel(&f.epoch, &f.play_id, &card, &current)
        .unwrap();
    assert!(matches!(
        f.client
            .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &current)
            .await,
        Err(Error::StaleResponse)
    ));
    let current = prepare(0, &[0, 1])
        .await
        .unwrap()
        .selection_receipt
        .unwrap();
    f.client
        .player_action(&f.epoch, PlayerAction::Stop)
        .await
        .unwrap();
    assert!(matches!(
        f.client
            .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &current)
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(f.store.values("25", TASKS).unwrap().is_empty());
    assert!(f.store.values("25", BATCH).unwrap().is_empty());
    assert_eq!(f.http.calls().len(), STARTUP.len() + 2);
}

#[tokio::test]
async fn group_download_rolls_back_all_work_on_late_unlike_or_duplicate() {
    let (f, card) = fixture([
        json!({"code":200,"ids":[301]}).into(),
        json!({"code":200,"ids":[]}).into(),
        json!({"code":200,"ids":[301]}).into(),
    ])
    .await;
    configure(&f).await;
    f.client.liked_ids().await.unwrap();
    let plan = f
        .client
        .player_voice_songs_download_prepare(&f.epoch, &f.play_id, &card, 0, &[0, 1])
        .await
        .unwrap();
    let receipt = plan.selection_receipt.unwrap();
    f.client.liked_ids().await.unwrap();
    assert!(matches!(
        f.client
            .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &receipt)
            .await,
        Err(Error::State(
            "SongCard batch requires a confirmed liked anchor"
        ))
    ));
    assert!(f.store.values("25", TASKS).unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(f.root.path().join("output"))
            .unwrap()
            .count(),
        0
    );

    f.client.liked_ids().await.unwrap();
    // The second row gets a new independent task after prepare. Inserting the
    // first then rejecting the second must leave only the pre-existing task.
    let existing_id = existing(&f, 302, "paused");
    assert!(matches!(
        f.client
            .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &receipt)
            .await,
        Err(Error::State("song already has a download"))
    ));
    assert_eq!(
        f.store
            .values("25", TASKS)
            .unwrap()
            .into_keys()
            .collect::<Vec<_>>(),
        std::slice::from_ref(&existing_id)
    );
    assert!(f.store.values("25", BATCH).unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(f.root.path().join("output"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>(),
        [format!(".cumulus-{existing_id}")]
    );
    add_local(&f, 301);
    assert!(matches!(
        f.client
            .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &receipt)
            .await,
        Err(Error::State("download selection gained a local file"))
    ));
    assert_eq!(f.store.values("25", TASKS).unwrap().len(), 1);
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(
            &target,
            &["MUSIC_U=rotated-synthetic-group; Path=/; Secure".into()],
            None,
            &revision,
        )
        .unwrap();
    assert!(matches!(
        f.client
            .player_voice_songs_download_start(&f.epoch, &f.play_id, &card, &receipt)
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(f.http.calls().len(), STARTUP.len() + 4);
}
