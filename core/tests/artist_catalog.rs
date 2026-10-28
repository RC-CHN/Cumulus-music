use cumulus_core::{
    catalog::{ArtistApi, ArtistPageRequest},
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Backend {
    requests: Mutex<Vec<(String, Value, bool)>>,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap()
                .push((path.into(), payload, cache));
            Ok(json!({"code":200,"artist":{"id":12},"hotSongs":[]}))
        })
    }
}

#[tokio::test]
async fn official_artist_subpages_use_their_distinct_ids_and_pagination_contracts() {
    let backend = Arc::new(Backend::default());
    let api = ArtistApi::new(backend.clone());
    let id = json!("12");
    let page = ArtistPageRequest {
        offset: 40,
        limit: 20,
    };
    api.detail(&id).await.unwrap();
    api.dynamic(&id).await.unwrap();
    api.albums(&id, page).await.unwrap();
    api.songs(&id, page).await.unwrap();
    api.introduction(&id).await.unwrap();
    api.similar(&id, page).await.unwrap();
    api.videos(&id, "900719925474099312345", 20).await.unwrap();
    api.concerts(&id, page).await.unwrap();
    let requests = backend.requests.lock().unwrap();
    assert_eq!(
        *requests,
        vec![
            (
                "/api/artist/v3/detail".into(),
                json!({"id":"12","top":50,"e_r":false}),
                true
            ),
            (
                "/api/artist/detail/dynamic".into(),
                json!({"id":"12"}),
                false
            ),
            (
                "/api/artist/albums/12".into(),
                json!({"limit":20,"offset":40}),
                false
            ),
            (
                "/api/v2/artist/songs".into(),
                json!({"id":"12","limit":20,"offset":40}),
                false
            ),
            ("/api/artist/introduction".into(), json!({"id":"12"}), false),
            (
                "/api/discovery/simiArtist".into(),
                json!({"artistid":"12","limit":20,"offset":40,"total":true}),
                false
            ),
            (
                "/api/mlog/artist/video".into(),
                json!({"artistId":"12","tab":1,"order":0,"page":r#"{"size":20,"cursor":900719925474099312345}"#}),
                false
            ),
            (
                "/api/concert/list/byartist".into(),
                json!({"artistId":"12","limit":20,"offset":40}),
                false
            ),
        ]
    );
}

#[tokio::test]
async fn invalid_navigation_input_never_reaches_api_and_zero_cursor_stays_json_number() {
    let backend = Arc::new(Backend::default());
    let api = ArtistApi::new(backend.clone());
    let id = json!(12);
    assert!(
        api.albums(&json!("../private"), ArtistPageRequest::default())
            .await
            .is_err()
    );
    for page in [
        ArtistPageRequest {
            offset: 0,
            limit: 0,
        },
        ArtistPageRequest {
            offset: 0,
            limit: 101,
        },
        ArtistPageRequest {
            offset: 1_000_001,
            limit: 20,
        },
    ] {
        assert!(api.albums(&id, page).await.is_err());
        assert!(api.songs(&id, page).await.is_err());
        assert!(api.similar(&id, page).await.is_err());
        assert!(api.concerts(&id, page).await.is_err());
    }
    for cursor in ["", "-1", "1.5", "0,\"extra\":true", "null", "１２"] {
        assert!(api.videos(&id, cursor, 20).await.is_err());
    }
    assert!(api.videos(&id, &"1".repeat(129), 20).await.is_err());
    assert!(backend.requests.lock().unwrap().is_empty());
    api.videos(&id, "000", 20).await.unwrap();
    assert_eq!(
        backend.requests.lock().unwrap()[0].1["page"],
        r#"{"size":20,"cursor":0}"#
    );
}
