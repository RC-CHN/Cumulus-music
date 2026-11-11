use super::*;
use cumulus_core::client::dto::{SearchCategory, SearchResultQuery};
use std::time::Duration;

pub(super) fn response(payload: &Value) -> Value {
    assert_eq!(
        payload,
        &json!({"keyword":"合成","scene":"normal","offset":0,"limit":10})
    );
    json!({"code":200,"data":{"resources":[
        {"song":{"id":"31","name":"合成","privilege":{"id":"31","subp":2}},"lyrics":"第一段"},
        {"song":{"id":"31","name":"合成","privilege":{"id":"31","subp":2}},"lyrics":"第二段"},
        {"song":{"id":"32","name":"合成","privilege":{"subp":null}},"lyrics":"空值"},
        {"song":{"id":"33","name":"合成","privilege":{}},"lyrics":"缺省"},
        {"song":{"id":"34","name":"合成","privilege":{"subp":-1}},"lyrics":"拒绝"},
        {"song":{"id":"35","name":"合成","privilege":{"subp":1}},"lyrics":"组外锚点"},
        {"song":{"id":"36","name":"合成","privilege":{"id":"999","subp":1}},"lyrics":"未知"},
        {"song":{"id":"37","name":"合成","privilege":{"subp":2}},"fileId":"private"}
    ]}})
}

async fn results(f: &Fixture, epoch: &str) -> Value {
    serde_json::to_value(
        f.client
            .search_results(
                epoch,
                SearchResultQuery {
                    keyword: "合成".into(),
                    category: SearchCategory::Lyrics,
                    offset: 0,
                    limit: 10,
                    correct: true,
                    channel: None,
                },
            )
            .await
            .unwrap()
            .value,
    )
    .unwrap()
}

fn item(view: &Value, index: usize) -> &Value {
    &view["sections"][0]["items"][index]
}

fn receipt(view: &Value, index: usize) -> String {
    item(view, index)["lyricGroupCollectionReceipt"]
        .as_str()
        .unwrap()
        .to_owned()
}

pub(super) async fn prepare_ordered(f: &Fixture, epoch: &str) -> (String, bool) {
    let view = results(f, epoch).await;
    let selected = [3, 2, 1, 4].map(|index| receipt(&view, index));
    let plan = f
        .client
        .lyric_group_collect_prepare(epoch, &receipt(&view, 5), &selected)
        .unwrap()
        .value;
    assert_eq!(plan.selected_count, 4);
    assert_eq!(plan.song_ids, ["33", "31"]);
    (plan.receipt.unwrap(), plan.can_create)
}

async fn destinations(f: &Fixture, epoch: &str, plan: &str) {
    let destinations = f
        .client
        .download_collect_destinations(epoch, plan, PlaylistCollectSort::Default, 0)
        .await
        .unwrap()
        .value;
    assert_eq!(
        destinations
            .playlists
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["81"]
    );
}

#[tokio::test]
async fn lyric_collection_keeps_exact_gesture_order_and_recovers_a_lost_add_without_replay() {
    let f = Fixture::new();
    let (api, epoch) = connect(&f);
    let rows = results(&f, &epoch).await;
    assert!(item(&rows, 7).get("lyricGroupCollectionReceipt").is_none());
    assert_ne!(receipt(&rows, 0), receipt(&rows, 1));
    let outsider = receipt(&rows, 5);
    let selected = [
        receipt(&rows, 3),
        receipt(&rows, 2),
        receipt(&rows, 1),
        receipt(&rows, 4),
    ];
    let plan = f
        .client
        .lyric_group_collect_prepare(&epoch, &outsider, &selected)
        .unwrap()
        .value;
    assert_eq!(plan.selected_count, 4);
    assert_eq!(plan.song_ids, ["33", "31"]);
    let receipt = plan.receipt.unwrap();
    assert!(api.writes.lock().unwrap().is_empty());
    assert_eq!(*api.reads.lock().unwrap(), ["/api/search/resource/lyric"]);
    destinations(&f, &epoch, &receipt).await;
    for invalid in ["82", "83", "84"] {
        assert!(
            f.client
                .download_collect_existing(&epoch, &receipt, invalid)
                .await
                .is_err()
        );
    }
    api.lost.store(true, Ordering::Release);
    let pending = f
        .client
        .download_collect_existing(&epoch, &receipt, "81")
        .await
        .unwrap()
        .value;
    assert_eq!(pending.phase, PlaylistPhase::Uncertain);
    let status = f
        .client
        .download_collect_existing(&epoch, &receipt, "81")
        .await
        .unwrap()
        .value;
    assert_eq!(status.operation_id, pending.operation_id);
    f.client.download_collect_cancel(&epoch, &receipt).unwrap();
    api.saved
        .lock()
        .unwrap()
        .extend(api.pending.lock().unwrap().drain(..));
    let confirmed = f
        .client
        .reconcile_playlist(&epoch, &pending.operation_id)
        .await
        .unwrap()
        .value;
    assert_eq!(confirmed.phase, PlaylistPhase::Confirmed);
    assert_eq!(confirmed.observed.unwrap().track_ids, ["33", "31"]);
    assert_eq!(
        *api.writes.lock().unwrap(),
        [json!({"pid":"81","trackIds":serde_json::to_string(&["33","31"]).unwrap(),"op":"add"})]
    );
    assert!(
        !api.reads
            .lock()
            .unwrap()
            .iter()
            .any(|path| path.contains("download"))
    );
}

#[tokio::test]
async fn lyric_collection_fallback_and_queued_write_are_bound_to_the_current_result() {
    let f = Fixture::new();
    let (api, epoch) = connect(&f);
    let view = results(&f, &epoch).await;
    let anchor = receipt(&view, 5);
    let falsey = receipt(&view, 2);
    let denied = receipt(&view, 4);
    let unknown = receipt(&view, 6);
    for rows in [[denied.clone(), falsey.clone()], [falsey.clone(), unknown]] {
        let empty = f
            .client
            .lyric_group_collect_prepare(&epoch, &anchor, &rows)
            .unwrap()
            .value;
        assert!(empty.song_ids.is_empty() && empty.receipt.is_none());
    }
    assert!(matches!(
        f.client.lyric_group_collect_prepare(
            &epoch,
            &anchor,
            &[receipt(&view, 0), receipt(&view, 1)]
        ),
        Err(Error::InvalidInput(_))
    ));
    let plan = f
        .client
        .lyric_group_collect_prepare(&epoch, &anchor, &[falsey, denied])
        .unwrap()
        .value;
    assert_eq!(plan.song_ids, ["32"]);
    let plan = plan.receipt.unwrap();
    destinations(&f, &epoch, &plan).await;
    api.hold.store(true, Ordering::Release);
    let (attempted, ()) = tokio::join!(
        f.client.download_collect_existing(&epoch, &plan, "81"),
        async {
            tokio::time::timeout(Duration::from_secs(2), api.entered.notified())
                .await
                .expect("the scoped write reached its before-dispatch guard");
            let _new = results(&f, &epoch).await;
            api.release.notify_one();
        }
    );
    assert!(attempted.is_err());
    assert!(api.writes.lock().unwrap().is_empty());
    assert!(matches!(
        f.client
            .download_collect_destinations(&epoch, &plan, PlaylistCollectSort::Default, 0)
            .await,
        Err(Error::StaleResponse)
    ));
}
