use super::*;
use std::sync::Mutex;
#[derive(Default)]
struct ReportsApi(Mutex<Vec<(String, Value)>>);
impl Api for ReportsApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.0.lock().unwrap().push((path.into(), payload));
            Ok(match path {
                "/api/song/lyric/v1" => json!({
                    "code":200, "lrc":{"version":17,"lyric":"[00:01]线上合成歌词"}
                }),
                "/api/feedback/correct/lyric/type" => {
                    json!({"code":200,"data":{"10":"歌词缺失/错误"}})
                }
                "/api/feedback/correct/song" => json!({"code":200}),
                _ => panic!("local lyric report attempted unrelated API: {path}"),
            })
        })
    }
}
#[tokio::test]
async fn lyric_report_uses_current_local_mapping_and_displayed_song_version_without_leaking_paths()
{
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("media");
    std::fs::create_dir(&media).unwrap();
    std::fs::write(media.join("local.wav"), b"synthetic local audio").unwrap();
    std::fs::write(media.join("local.lrc"), "[00:01]本地合成歌词").unwrap();
    let (session, client, library) = client(&root.path().join("state"), Arc::new(Probe::default()));
    let api = Arc::new(ReportsApi::default());
    session
        .adopt(api.clone(), json!({"userId":9}), &session.token().unwrap())
        .unwrap();
    library.scan(&media).unwrap();
    let page = client.local_page(LocalQuery::default()).await.unwrap();
    let id = page.tracks[0].track.id.clone();
    let view = client
        .play_selection(
            "1",
            PlaySelection::Local {
                query: LocalQuery::default(),
                start_id: Some(id.clone()),
                revision: page.revision,
            },
        )
        .await
        .unwrap();
    let play_id = view.play_id.unwrap();
    assert!(matches!(
        client.lyric_report_open("1", &play_id).await,
        Err(Error::InvalidInput(
            "local lyric report requires an online match"
        ))
    ));
    assert!(api.0.lock().unwrap().is_empty());
    library.map_online(&session, &id, &json!(123)).unwrap();
    let opened = client.lyric_report_open("1", &play_id).await.unwrap();
    let loaded = client
        .lyric_report_types("1", &opened.id, &opened.revision)
        .await
        .unwrap();
    let result = client
        .lyric_report_submit(
            "1",
            &loaded.id,
            &loaded.revision,
            "10",
            "本地歌词内容需要核对",
        )
        .await
        .unwrap();
    assert_eq!(result.phase, cumulus_core::lyrics::reports::Phase::Sent);
    let calls = api.0.lock().unwrap().clone();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[0],
        (
            "/api/song/lyric/v1".into(),
            json!({"id":"123","lv":-1,"tv":-1,"rv":-1,"yv":-1})
        )
    );
    assert_eq!(
        calls[2].1,
        json!({"songId":"123","lyricVersion":17,"correctType":10,"content":"本地歌词内容需要核对"})
    );
    let public = serde_json::to_string(&result).unwrap();
    assert!(
        !public.contains("local_id")
            && !public.contains("local_revision")
            && !public.contains(root.path().to_str().unwrap())
    );
    session
        .adopt(api.clone(), json!({"userId":10}), &session.token().unwrap())
        .unwrap();
    assert!(client.lyric_report_read("2", &loaded.id).is_err());
    let page = client.local_page(LocalQuery::default()).await.unwrap();
    let selected = client
        .play_selection(
            "2",
            PlaySelection::Local {
                query: LocalQuery::default(),
                start_id: Some(id),
                revision: page.revision,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        client
            .lyric_report_open("2", selected.play_id.as_deref().unwrap())
            .await,
        Err(Error::InvalidInput(
            "local lyric report requires an online match"
        ))
    ));
    assert_eq!(api.0.lock().unwrap().len(), 3);
}
