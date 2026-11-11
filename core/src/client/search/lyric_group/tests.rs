use super::*;
use crate::protocol::api::ApiFuture;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
struct Wire(AtomicU64);

impl Api for Wire {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        panic!("the native authority does not request a resource by itself")
    }

    fn lyric_group_playback_credential_revision(&self) -> Result<Option<String>> {
        Ok(Some(self.0.load(Ordering::Acquire).to_string()))
    }
}

fn query(offset: usize) -> SearchResultQuery {
    SearchResultQuery {
        keyword: "合成".into(),
        category: SearchCategory::Lyrics,
        offset,
        limit: 3,
        correct: true,
        channel: None,
    }
}

fn hit(id: &str, line: &str, status: i64, bitrate: u64) -> Value {
    json!({"song":{"id":id,"name":"合成标题","dt":120000,
        "privilege":{"id":id,"st":status,"pl":bitrate}},"lyrics":line})
}

fn result(query: SearchResultQuery, rows: Vec<Value>) -> SearchResultView {
    super::super::results::project(
        json!({"code":200,"data":{"resources":rows}}),
        query,
        "9",
        false,
    )
    .unwrap()
    .0
}

fn receipt(view: &SearchResultView, index: usize) -> Option<String> {
    serde_json::to_value(view).unwrap()["sections"][0]["items"][index]["lyricGroupPlayback"]
        ["receipt"]
        .as_str()
        .map(str::to_owned)
}

#[test]
fn coherent_duplicate_lyric_hits_have_exact_receipts_and_conflicting_pages_revoke_them() {
    let cache = LyricGroupPlayback::default();
    let api: Arc<dyn Api> = Arc::new(Wire::default());
    let token = SessionToken {
        uid: Some("9".into()),
        generation: 1,
    };
    let first = query(0);
    let read = cache
        .begin(&token, &api, &first, Some("0".into()))
        .unwrap()
        .unwrap();
    let mut page = result(
        first,
        vec![
            hit("1", "第一段", 0, 320000),
            hit("1", "另一段", 0, 320000),
            hit("2", "次序", 0, 320000),
        ],
    );
    cache.capture(&token, &api, read, &mut page).unwrap();
    let a = receipt(&page, 0).unwrap();
    let duplicate = receipt(&page, 1).unwrap();
    let b = receipt(&page, 2).unwrap();
    assert_ne!(a, duplicate);
    let chosen = cache
        .resolve(
            &token,
            &api,
            "0",
            &duplicate,
            &[b.clone(), duplicate.clone()],
        )
        .unwrap();
    assert_eq!(chosen.anchor_id, "1");
    assert_eq!(
        chosen
            .tracks
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["2", "1"]
    );
    assert!(matches!(
        cache.resolve(&token, &api, "0", &a, &[a.clone(), duplicate.clone()]),
        Err(Error::InvalidInput(_))
    ));

    let second = query(3);
    let read = cache
        .begin(&token, &api, &second, Some("0".into()))
        .unwrap()
        .unwrap();
    let mut page = result(
        second,
        vec![
            hit("2", "续页又命中", 0, 320000),
            hit("3", "版权受限", -200, 0),
            hit("4", "第四首", 0, 320000),
        ],
    );
    cache.capture(&token, &api, read, &mut page).unwrap();
    let cross_page = receipt(&page, 0).unwrap();
    assert_ne!(b, cross_page);
    let blocked = receipt(&page, 1).unwrap();
    let fourth = receipt(&page, 2).unwrap();
    let cross = cache
        .resolve(
            &token,
            &api,
            "0",
            &a,
            &[cross_page.clone(), blocked.clone(), a.clone()],
        )
        .unwrap();
    assert_eq!(
        cross
            .tracks
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["2", "1"]
    );
    assert!(matches!(
        cache.resolve(
            &token,
            &api,
            "0",
            &blocked,
            &[blocked.clone(), fourth.clone()]
        ),
        Err(Error::State(_))
    ));

    let third = query(6);
    let read = cache
        .begin(&token, &api, &third, Some("0".into()))
        .unwrap()
        .unwrap();
    let mut page = result(third, vec![hit("1", "互斥权限", -200, 0)]);
    cache.capture(&token, &api, read, &mut page).unwrap();
    assert!(receipt(&page, 0).is_none());
    assert!(!cache.current(&chosen));
    assert!(!cache.current(&cross));
    assert!(matches!(
        cache.resolve(
            &token,
            &api,
            "0",
            &duplicate,
            &[fourth.clone(), duplicate.clone()]
        ),
        Err(Error::StaleResponse)
    ));
    assert!(
        cache
            .resolve(
                &token,
                &api,
                "0",
                &cross_page,
                &[fourth, cross_page.clone()]
            )
            .is_ok()
    );
}

#[test]
fn lyric_authority_only_publishes_current_contiguous_authentic_results() {
    let cache = LyricGroupPlayback::default();
    let wire = Arc::new(Wire::default());
    let api: Arc<dyn Api> = wire.clone();
    let token = SessionToken {
        uid: Some("9".into()),
        generation: 1,
    };
    let first = query(0);
    let late = cache
        .begin(&token, &api, &first, Some("0".into()))
        .unwrap()
        .unwrap();
    let current = cache
        .begin(&token, &api, &first, Some("0".into()))
        .unwrap()
        .unwrap();
    let mut old = result(first.clone(), vec![hit("91", "旧回包", 0, 320000)]);
    cache.capture(&token, &api, late, &mut old).unwrap();
    assert!(receipt(&old, 0).is_none());
    assert!(
        cache
            .begin(&token, &api, &query(3), Some("0".into()))
            .unwrap()
            .is_none()
    );
    let mut disagree = hit("92", "外层特权冲突", 0, 320000);
    disagree["privilege"] = json!({"id":"92","st":-200,"pl":0});
    let mut private = hit("93", "标识冲突", 0, 320000);
    private["id"] = json!("900");
    let mut page = result(first, vec![disagree, private, hit("94", "原生", 0, 320000)]);
    cache.capture(&token, &api, current, &mut page).unwrap();
    assert!(receipt(&page, 0).is_none() && receipt(&page, 1).is_none());
    let first_receipt = receipt(&page, 2).unwrap();

    let mut other = query(3);
    other.keyword = "别的搜索".into();
    assert!(
        cache
            .begin(&token, &api, &other, Some("0".into()))
            .unwrap()
            .is_none()
    );
    let second = query(3);
    let read = cache
        .begin(&token, &api, &second, Some("0".into()))
        .unwrap()
        .unwrap();
    let mut after_rotation = result(second, vec![hit("95", "旧凭据", 0, 320000)]);
    wire.0.store(1, Ordering::Release);
    cache
        .capture(&token, &api, read, &mut after_rotation)
        .unwrap();
    assert!(receipt(&after_rotation, 0).is_none());
    assert!(matches!(
        cache.resolve(
            &token,
            &api,
            "1",
            &first_receipt,
            &[first_receipt.clone(), Uuid::new_v4().simple().to_string()]
        ),
        Err(Error::StaleResponse)
    ));
}
