use super::*;
use cumulus_core::playback::controller::PlaybackStatus;

pub(super) fn response(payload: &Value) -> Value {
    let ids = if payload["offset"] == 0 {
        [4, 5, 6]
    } else {
        [2, 7, 8]
    };
    let resources: Vec<_> = ids
        .into_iter()
        .map(|id| {
            json!({"baseInfo":{"simpleSongData":{"id":id,
                "name":format!("合成搜索歌曲 {id}"),"dt":120000,
                "ar":[{"id":101,"name":"合成歌手"}],"al":{"id":123,"name":"合成专辑"},
                "privilege":{"cs":id==6}}}})
        })
        .collect();
    json!({"code":200,"data":{"resources":resources,"totalCount":6,"hasMore":payload["offset"]==0}})
}

fn query(offset: usize) -> SearchResultQuery {
    SearchResultQuery {
        keyword: "合成".into(),
        category: SearchCategory::Songs,
        offset,
        limit: 3,
        correct: true,
        channel: None,
    }
}

fn drop(view: &PlayerView, before: Option<String>, rows: Vec<Value>) -> PlayerAction {
    serde_json::from_value(json!({"kind":"drop_search_songs_into_queue","rows":rows,
        "quality":"standard","before_entry_id":before,"expected_play_id":view.play_id,
        "expected_queue_revision":view.queue_revision}))
    .unwrap()
}

fn search_row(section: &Value, index: usize) -> Value {
    let row = &section["sections"][0]["items"][index];
    json!({"receipt":row["queueDropReceipt"],"track_id":row["track"]["id"]})
}

#[tokio::test]
async fn search_drag_inserts_only_native_observed_rows_from_both_pages_without_restarting() {
    let (_dir, api, session, client, _) = fixture();
    let _ = client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: ["1", "2", "3"].map(str::to_owned).to_vec(),
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    let playing = client
        .player_action("1", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    let before = client.player_queue("1").await.unwrap();
    let p1 =
        serde_json::to_value(client.search_results("1", query(0)).await.unwrap().value).unwrap();
    let p2 =
        serde_json::to_value(client.search_results("1", query(3)).await.unwrap().value).unwrap();
    assert!(search_row(&p1, 0)["receipt"].is_string());
    assert!(search_row(&p1, 2)["receipt"].is_null());
    assert_ne!(search_row(&p1, 0)["receipt"], search_row(&p2, 0)["receipt"]);
    let next = before.tracks[2].entry_id.clone();
    let rows = vec![search_row(&p2, 1), search_row(&p1, 0), search_row(&p2, 0)];
    let mut forged = rows[0].clone();
    forged["track_id"] = json!("4");
    assert!(
        client
            .player_action("1", drop(&playing, Some(next.clone()), vec![forged]))
            .await
            .is_err()
    );
    assert_eq!(
        client.player_queue("1").await.unwrap().revision,
        before.revision
    );
    api.calls.lock().unwrap().clear();
    let result = client
        .player_action("1", drop(&playing, Some(next.clone()), rows.clone()))
        .await
        .unwrap();
    assert_eq!(result.play_id, playing.play_id);
    assert_eq!(result.state, PlaybackStatus::Paused);
    assert_eq!(result.position, playing.position);
    let queue = client.player_queue("1").await.unwrap();
    assert_eq!(
        queue
            .tracks
            .iter()
            .map(|r| r.track.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "7", "4", "2", "3"]
    );
    assert_eq!(queue.current, before.current);
    assert_eq!(queue.tracks[3].entry_id, before.tracks[1].entry_id);
    assert!(queue.tracks[1].track.name.contains("合成搜索"));
    assert!(api.calls.lock().unwrap().is_empty());
    assert!(matches!(
        client
            .player_action("1", drop(&playing, Some(next), rows.clone()))
            .await,
        Err(Error::QueueChanged)
    ));

    let token = session.token().unwrap();
    session
        .adopt(api.clone(), json!({"userId":11}), &token)
        .unwrap();
    let other = client.play_selection("2", songs("1")).await.unwrap();
    assert!(matches!(
        client.player_action("2", drop(&other, None, rows)).await,
        Err(Error::State("search drop results expired"))
    ));
    assert_eq!(client.player_queue("2").await.unwrap().tracks.len(), 1);
}
