use super::*;
use crate::support::{http::reply, transfer::Origin};
use cumulus_core::download::service::DownloadRunOptions;
use md5::{Digest, Md5};

#[tokio::test]
async fn same_purchased_tier_readmits_original_task_and_verified_partial_media() {
    const DATA: &[u8] = include_bytes!("../../fixtures/tag_audio/seed.flac");
    let root = tempfile::tempdir().unwrap();
    let auth = json!({"code":200,"data":{"code":200,"size":DATA.len(),
        "md5":hex::encode(Md5::digest(DATA)),"type":"flac","level":"hires",
        "url":"https://m.test.music.126.net/media?authorization=synthetic"}});
    let detail = |id| {
        json!({"code":200,"songs":[{"id":id,"name":format!("合成 {id}")}],
        "privileges":[]})
    };
    // Both workers may authorize concurrently and the fresh song also reads
    // detail. Resolve only those three sealed calls, in either valid order.
    let worker_replies = (0..3).map(|_| {
        let auth = auth.clone();
        HttpResponse::ForCall(Box::new(move |call| match call.path.as_str() {
            AUTH => Ok(reply(auth)),
            "/api/v3/song/detail" => Ok(reply(detail(43))),
            other => panic!("unexpected pending download request: {other}"),
        }))
    });
    let (api, http) = vinyl_http(
        root.path(),
        &[COOKIE],
        [
            purchased_search_page(),
            json!({"code":200,"ids":[42]}).into(),
            auth.clone().into(),
            detail(41).into(),
        ]
        .into_iter()
        .chain(worker_replies),
    );
    let media = Arc::new(Origin::new(DATA.to_vec()));
    let (client, session, epoch) = client(root.path(), api);
    let client = client.with_download_backend(media.clone());
    let rows = search(&client, &epoch).await;
    client.liked_ids().await.unwrap();
    directory(&client, &epoch, root.path()).await;
    let preferences = client.download_quality_preferences(&epoch).unwrap();
    let mut options = preferences.options;
    options.tags = false;
    options.cover = false;
    client
        .download_command(
            &epoch,
            DownloadCommand::Preferences {
                revision: preferences.revision,
                options: options.clone(),
            },
        )
        .await
        .unwrap();
    options.quality = Quality::HiRes;
    let service = DownloadService::with_backend(session, media.clone(), 512 * 1024 * 1024).unwrap();
    let old = service.create_default(&json!(41), options).unwrap();
    let old = service
        .run(
            &old.id,
            DownloadRunOptions {
                byte_budget: Some(256),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!((old.status, old.bytes), (DownloadStatus::Paused, 256));
    assert_eq!(http.calls().len(), 4);
    assert_eq!(
        media.calls().iter().map(|c| c.offset).collect::<Vec<_>>(),
        [0]
    );

    async fn prepare_same(client: &MusicClient, epoch: &str, rows: &Value) -> String {
        let R::Prepared {
            decision: D::PurchasedQualityRequired,
            receipt: Some(receipt),
            eligible_count: 2,
            omitted_count: 1,
            existing_purchased_quality: Some(Quality::HiRes),
            purchased_qualities,
            ..
        } = client
            .search_group_download(epoch, prepare(rows, 1, &[0, 2, 1]))
            .await
            .unwrap()
        else {
            panic!("a verified old and one new task should reach the chooser")
        };
        assert_eq!(
            purchased_qualities,
            [Quality::HiRes, Quality::ExHigh, Quality::Standard]
        );
        receipt
    }

    let receipt = prepare_same(&client, &epoch, &rows).await;
    let confirm = |receipt: &str, quality| C::ConfirmQuality {
        receipt: receipt.into(),
        quality,
    };
    assert!(
        matches!(client.search_group_download(&epoch, confirm(&receipt, Quality::ExHigh)).await.unwrap(),
        R::ExistingQualityMismatch { required_quality: Quality::HiRes, receipt: ref echoed, .. } if echoed == &receipt)
    );
    assert_eq!(service.get(&old.id).unwrap().bytes, 256);
    assert_eq!(service.list().unwrap().len(), 1);
    assert_eq!(http.calls().len(), 4);
    assert_eq!(media.calls().len(), 1);
    service.request(&old.id, DownloadAction::Pause).unwrap();
    assert!(matches!(
        client
            .search_group_download(&epoch, confirm(&receipt, Quality::HiRes))
            .await,
        Err(Error::StaleResponse)
    ));
    assert_eq!(
        http.calls().len(),
        4,
        "a new pause intent must not authorize old bytes"
    );

    let receipt = prepare_same(&client, &epoch, &rows).await;
    let R::Accepted { result } = client
        .search_group_download(&epoch, confirm(&receipt, Quality::HiRes))
        .await
        .unwrap()
    else {
        panic!("matching quality should admit the same task")
    };
    assert_eq!(result.accepted.len(), 2);
    assert_eq!(result.accepted[0], old.id);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let tasks = service.list().unwrap();
            assert!(
                tasks
                    .iter()
                    .all(|task| task.status != DownloadStatus::Error),
                "synthetic batch did not finish: {:?}",
                tasks
                    .iter()
                    .map(|task| (task.song_id.as_str(), &task.last_error))
                    .collect::<Vec<_>>()
            );
            let done = tasks.len() == 2
                && tasks
                    .iter()
                    .all(|task| task.status == DownloadStatus::Complete);
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("both verified synthetic downloads finish");
    let resumed = service.get(&old.id).unwrap();
    assert_eq!(resumed.target, old.target);
    assert_eq!(resumed.created_ms, old.created_ms);
    assert_eq!(resumed.authorization_count, 2);
    assert_eq!(service.list().unwrap().len(), 2);
    let mut offsets = media.calls().iter().map(|c| c.offset).collect::<Vec<_>>();
    offsets.sort_unstable();
    assert_eq!(offsets, [0, 0, 256]);
    for id in &result.accepted {
        let task = service.get(id).unwrap();
        assert_eq!(task.options.quality, Quality::HiRes);
        assert_eq!(std::fs::read(task.target.unwrap()).unwrap(), DATA);
    }
    let calls = http.calls();
    assert_eq!(calls.len(), 7);
    let mut authorization = calls
        .iter()
        .filter(|c| c.path == AUTH)
        .map(|c| {
            assert_eq!(c.payload["level"], "hires");
            assert_eq!(c.payload["immerseType"], "c51");
            c.payload["id"].as_str().unwrap()
        })
        .collect::<Vec<_>>();
    authorization.sort_unstable();
    assert_eq!(authorization, ["41", "41", "43"]);
}
