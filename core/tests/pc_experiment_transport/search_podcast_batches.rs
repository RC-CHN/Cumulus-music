//! M/3095 AUDIO scroll requests and M/15.Jn / M/3001 result semantics.
use super::{Error, Fixture, SearchCategory, SearchResultQuery, Value, json};

const PATH: &str = "/api/search/voicelist/get";
const KEYWORD: &str = "合成已纠正播客";

fn radio(id: u64) -> Value {
    json!({"baseInfo":{"id":id,"name":format!("合成播客 {id}")}})
}

fn page(data: Value) -> Value {
    json!({"code":200,"data":data,"queryRewriteInfo":{"rewriteQuery":"不属于播客分类的纠正"}})
}

#[tokio::test]
async fn audio_uses_mapped_batch_size_and_distinguishes_null_from_empty() {
    let mut first = (1..=9).map(radio).collect::<Vec<_>>();
    first.push(json!({"baseInfo":{"name":"没有可信标识"}}));
    let fixture = Fixture::new(
        [
            page(json!({"resources":first,"totalCount":9,"hasMore":false})),
            json!({"code":500,"msg":"合成续页失败"}),
            page(
                json!({"resources":[radio(10),radio(11),radio(12)],"totalCount":99,"hasMore":true}),
            ),
            page(json!({"resources":null,"totalCount":99,"hasMore":true})),
            page(json!({"resources":[],"totalCount":99,"hasMore":true})),
            page(json!({"resources":[],"totalCount":99,"hasMore":false})),
            page(json!({"totalCount":99,"hasMore":true})),
            page(json!({"resources":null})),
            page(json!({"resources":{}})),
            page(Value::Null),
        ]
        .map(Into::into),
        true,
    )
    .await;
    let read = |offset| {
        fixture.client.search_results(
            &fixture.epoch,
            SearchResultQuery {
                keyword: KEYWORD.into(),
                category: SearchCategory::Podcasts,
                offset,
                limit: 10,
                correct: true,
                channel: None,
            },
        )
    };
    let first = read(0).await.unwrap().value;
    assert_eq!(first.total, Some(9));
    assert_eq!((first.next_offset, first.more), (10, true));
    assert_eq!(
        (first.sections[0].items.len(), first.unavailable_count),
        (9, 1)
    );
    assert!(first.correction.is_none());
    assert!(matches!(read(10).await, Err(error) if error.is_service_code(500)));
    let short = read(10).await.unwrap().value;
    assert_eq!(short.total, Some(99));
    assert_eq!((short.next_offset, short.more), (13, false));
    assert_eq!(short.sections[0].items.len(), 3);
    assert!(short.correction.is_none());
    for total in [99, 0, 99, 0, 0] {
        let empty = read(0).await.unwrap().value;
        assert_eq!(empty.total, Some(total));
        assert_eq!((empty.next_offset, empty.more), (0, false));
        assert!(empty.sections[0].items.is_empty());
        assert!(empty.notice.is_none() && empty.correction.is_none());
    }
    for _ in 0..2 {
        assert!(matches!(
            read(0).await,
            Err(Error::Response("missing search result list"))
        ));
    }
    let calls = fixture.http.calls();
    assert_eq!(calls.len(), 11);
    assert_eq!(calls[0].path, "/api/w/nuser/account/get");
    for (call, offset) in calls[1..].iter().zip([0, 10, 10, 0, 0, 0, 0, 0, 0, 0]) {
        assert_eq!(call.path, PATH);
        assert!(
            call.cookie
                .as_deref()
                .unwrap()
                .contains("MUSIC_U=synthetic-pc-experiment")
        );
        let mut args = call.payload.clone();
        let header = args.as_object_mut().unwrap().remove("header").unwrap();
        let header: Value = serde_json::from_str(header.as_str().unwrap()).unwrap();
        assert_eq!(header["clientSign"], "synthetic-signature");
        assert_eq!(
            args,
            json!({"keyword":KEYWORD,"scene":"normal","limit":"10","offset":offset.to_string()})
        );
    }
}
