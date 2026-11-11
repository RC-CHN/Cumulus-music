use super::*;
use crate::protocol::api::ApiFuture;
use serde_json::json;

struct Wire;

impl Api for Wire {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        panic!("collection authority never performs a request")
    }

    fn lyric_group_collection_credential_revision(&self) -> Result<Option<String>> {
        Ok(Some("synthetic".into()))
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

fn hit(id: &str, subp: i64, lyrics: &str) -> Value {
    json!({"song":{"id":id,"name":"合成","privilege":{"id":id,"subp":subp}},"lyrics":lyrics})
}

fn publish(
    scope: &LyricCollection,
    token: &SessionToken,
    api: &Arc<dyn Api>,
    query: SearchResultQuery,
    rows: Vec<Value>,
) -> Value {
    let read = scope
        .begin(token, api, &query, Some("synthetic".into()))
        .unwrap()
        .unwrap();
    let mut view = super::super::results::project(
        json!({"code":200,"data":{"resources":rows}}),
        query,
        "9",
        false,
    )
    .unwrap()
    .0;
    scope.capture(token, api, read, &mut view).unwrap();
    serde_json::to_value(view).unwrap()
}

fn receipt(page: &Value, index: usize) -> Option<String> {
    page["sections"][0]["items"][index]["lyricGroupCollectionReceipt"]
        .as_str()
        .map(str::to_owned)
}

#[test]
fn coherent_duplicate_collection_hits_revoke_the_old_scope_on_conflicting_continuation() {
    let cache = LyricCollection::default();
    let token = SessionToken {
        generation: 1,
        uid: Some("9".into()),
    };
    let api: Arc<dyn Api> = Arc::new(Wire);
    let first = publish(
        &cache,
        &token,
        &api,
        query(0),
        vec![hit("1", 2, "甲"), hit("1", 2, "乙"), hit("2", 1, "丙")],
    );
    let a = receipt(&first, 0).unwrap();
    let b = receipt(&first, 1).unwrap();
    let other = receipt(&first, 2).unwrap();
    assert_ne!(a, b);
    let active = cache
        .resolve(&token, &api, "synthetic", &b, &[other.clone(), a.clone()])
        .unwrap();
    assert_eq!(active.songs, ["2", "1"]);
    let next = publish(
        &cache,
        &token,
        &api,
        query(3),
        vec![hit("1", -1, "冲突"), hit("3", 2, "新结果")],
    );
    assert!(receipt(&next, 0).is_none());
    assert!(!cache.current(&active));
    let anchor = receipt(&next, 1).unwrap();
    assert!(matches!(
        cache.resolve(&token, &api, "synthetic", &anchor, &[other.clone(), b]),
        Err(Error::StaleResponse)
    ));
    assert!(
        cache
            .resolve(&token, &api, "synthetic", &anchor, &[other, anchor.clone()])
            .is_ok()
    );
}
