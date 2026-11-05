use cumulus_core::{
    Error, Result,
    podcast::{AudiobookSkip, PodcastApi, is_audiobook},
    protocol::api::{Api, ApiFuture},
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

struct Sealed {
    calls: Mutex<Vec<Value>>,
    response: Value,
}

impl Api for Sealed {
    fn request<'a>(&'a self, path: &'a str, payload: Value, plain: bool) -> ApiFuture<'a, Value> {
        assert_eq!(path, "/api/voice/alg/skip/info/get");
        assert!(!plain);
        self.calls.lock().unwrap().push(payload);
        Box::pin(async move { Ok(self.response.clone()) })
    }
}

fn wire(data: Value) -> (Arc<Sealed>, PodcastApi) {
    let sealed = Arc::new(Sealed {
        calls: Mutex::new(Vec::new()),
        response: json!({"code":200,"data":data}),
    });
    let api = PodcastApi::new(sealed.clone());
    (sealed, api)
}

#[tokio::test]
async fn only_smart_uses_program_array_and_manual_boundaries_follow_official_duration_rules()
-> Result<()> {
    let (sealed, api) = wire(json!([{
        "programId": "9223372036854775901", "skipBeginDuration": 22.5, "skipEndDuration": 30
    }]));
    let episode = "9223372036854775901";
    let manual = AudiobookSkip::Custom { start: 32, end: 16 };
    let short = manual.resolve(&api, episode, 199.9).await?;
    assert_eq!(short.start_position(4.5, 199.9), 4.5);
    assert!(!short.reaches_outro(193.0, 199.9));
    let long = manual.resolve(&api, episode, 200.2).await?;
    assert_eq!(long.start_position(4.5, 200.2), 32.0);
    assert_eq!(long.start_position(47.0, 200.2), 47.0);
    assert!(!long.reaches_outro(184.9, 200.2));
    assert!(long.reaches_outro(185.0, 200.2));
    assert!(sealed.calls.lock().unwrap().is_empty());

    let smart = AudiobookSkip::Smart.resolve(&api, episode, 130.0).await?;
    assert_eq!(smart.start_position(4.0, 130.0), 22.5);
    assert!(!smart.reaches_outro(120.0, 130.0));
    assert_eq!(
        &*sealed.calls.lock().unwrap(),
        &[json!({"programIdsStr":["9223372036854775901"]})]
    );
    Ok(())
}

#[tokio::test]
async fn empty_algorithm_response_is_distinct_from_mismatched_or_malformed_rows() -> Result<()> {
    let (_, api) = wire(json!([]));
    let empty = api.audiobook_skip_intervals("7").await?;
    assert_eq!(empty.start_position(8.0, 400.0), 8.0);
    assert!(!empty.reaches_outro(399.0, 400.0));
    for data in [
        json!([{"programId":8,"skipBeginDuration":4}]),
        json!([{"programId":7,"skipBeginDuration":-4}]),
        json!([{"programId":7,"skipEndDuration":"23"}]),
        json!({"programId":7,"skipEndDuration":23}),
    ] {
        let (_, api) = wire(data);
        assert!(matches!(
            api.audiobook_skip_intervals("7").await,
            Err(Error::Response(_))
        ));
    }
    Ok(())
}

#[test]
fn strict_book_category_and_local_preference_are_scoped_to_account_and_radio() -> Result<()> {
    assert!(is_audiobook(&json!({"category":"广播剧"})));
    assert!(is_audiobook(&json!({"categoryId":3148096})));
    assert!(!is_audiobook(&json!({"categoryId":"3148096"})));
    assert!(!is_audiobook(
        &json!({"category":"音乐故事","radioFeeType":2})
    ));
    let root = tempfile::tempdir().unwrap();
    let store = StateStore::open(root.path().join("skip.db"))?;
    let mode = AudiobookSkip::Custom { start: 0, end: 90 };
    mode.save(&store, "51", "7")?;
    assert_eq!(AudiobookSkip::read(&store, "51", "7")?, mode);
    assert_eq!(
        AudiobookSkip::read(&store, "52", "7")?,
        AudiobookSkip::Close
    );
    assert_eq!(
        AudiobookSkip::read(&store, "51", "8")?,
        AudiobookSkip::Close
    );
    assert!(matches!(
        (AudiobookSkip::Custom { start: 91, end: 0 }).save(&store, "51", "7"),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(AudiobookSkip::read(&store, "51", "7")?, mode);
    AudiobookSkip::Close.save(&store, "51", "7")?;
    assert_eq!(
        AudiobookSkip::read(&store, "51", "7")?,
        AudiobookSkip::Close
    );
    Ok(())
}
