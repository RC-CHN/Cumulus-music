use cumulus_core::{
    Error,
    account::{AccountSession, profiles::ProfileRegistry},
    client::{MusicClient, dto::*},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Backend(Mutex<Vec<(String, Value)>>);
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.0.lock().unwrap().push((path.into(), args.clone()));
            let leaf = |id| json!({"id":id,"name":"合成资源","picUrl":"https://p1.music.126.net/synthetic?private=SECRET"});
            Ok(match path {
                "/api/search/song/list/page" => {
                    let rows: Vec<_> = (1..=8)
                        .map(|id| {
                            let mut item = leaf(id);
                            if id == 2 {
                                item["privilege"] = json!({"st":-200});
                            }
                            json!({"baseInfo":{"simpleSongData":item},"raw":"SECRET"})
                        })
                        .collect();
                    json!({"code":200,"data":{"resources":rows}})
                }
                "/api/v1/search/artist/get" => json!({"code":200,"result":{"artists":[leaf(91)]}}),
                "/api/v1/search/album/get" => json!({"code":200,"result":{"albums":[leaf(92)]}}),
                "/api/v1/search/playlist/get" => {
                    json!({"code":200,"result":{"playlists":[leaf(93)]}})
                }
                "/api/search/voicelist/get" => {
                    json!({"code":200,"data":{"resources":[{"baseInfo":leaf(94)}]}})
                }
                _ => panic!("unexpected request {path}"),
            })
        })
    }
}

fn new_client(
    path: &std::path::Path,
    api: Arc<Backend>,
    user: u64,
) -> (Arc<AccountSession>, MusicClient) {
    let registry = ProfileRegistry::open(path, ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session
        .adopt(api, json!({"userId":user}), &session.token().unwrap())
        .unwrap();
    (
        session.clone(),
        MusicClient::with_session(session, registry),
    )
}

#[tokio::test]
async fn composer_uses_five_official_search_envelopes_and_filters_without_losing_paging() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (_, client) = new_client(root.path(), api.clone(), 41);
    for (kind, path, args, expected, next) in [
        (
            ComposerKind::Song,
            "/api/search/song/list/page",
            json!({"keyword":"词","scene":"normal","limit":8,"offset":0,"needCorrect":true}),
            "1",
            json!(8),
        ),
        (
            ComposerKind::Artist,
            "/api/v1/search/artist/get",
            json!({"s":"词","limit":8,"offset":0,"queryCorrect":true}),
            "91",
            Value::Null,
        ),
        (
            ComposerKind::Album,
            "/api/v1/search/album/get",
            json!({"s":"词","limit":8,"offset":0,"queryCorrect":true}),
            "92",
            Value::Null,
        ),
        (
            ComposerKind::Playlist,
            "/api/v1/search/playlist/get",
            json!({"s":"词","limit":8,"offset":0,"queryCorrect":true}),
            "93",
            Value::Null,
        ),
        (
            ComposerKind::Podcast,
            "/api/search/voicelist/get",
            json!({"keyword":"词","scene":"normal","limit":8,"offset":0}),
            "94",
            Value::Null,
        ),
    ] {
        let page = serde_json::to_value(
            client
                .community_composer_search("1", kind, " 词 ", 0)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(api.0.lock().unwrap().last(), Some(&(path.into(), args)));
        assert_eq!(page["value"]["items"][0]["id"], expected);
        assert_eq!(page["value"]["nextOffset"], next);
        assert!(!page.to_string().contains("SECRET"));
        if kind == ComposerKind::Song {
            assert_eq!(page["value"]["items"].as_array().unwrap().len(), 7);
        }
    }
    assert!(
        client
            .community_composer_search("1", ComposerKind::Song, "词", 1)
            .await
            .is_err()
    );
    assert_eq!(api.0.lock().unwrap().len(), 5);
}

#[tokio::test]
async fn composer_drafts_survive_reopen_and_reject_stale_revisions_and_other_accounts() {
    let root = tempfile::tempdir().unwrap();
    let api = Arc::new(Backend::default());
    let (session, client) = new_client(root.path(), api.clone(), 41);
    let search = serde_json::to_value(
        client
            .community_composer_search("1", ComposerKind::Album, "词", 0)
            .await
            .unwrap(),
    )
    .unwrap();
    let receipt = search["value"]["items"][0]["receipt"]
        .as_str()
        .unwrap()
        .to_owned();
    let save = |revision, text: &str, attachment| ComposerDraftAction::Save {
        revision,
        text: text.into(),
        attachment,
    };
    let first = serde_json::to_value(
        client
            .community_composer_draft(
                "1",
                save(
                    None,
                    "草稿",
                    DraftAttachmentEdit::Select {
                        receipt: receipt.clone(),
                    },
                ),
            )
            .unwrap(),
    )
    .unwrap();
    let revision = first["value"]["revision"].as_str().unwrap().to_owned();
    assert_eq!(first["value"]["attachment"]["id"], "92");
    assert!(!first.to_string().contains("SECRET"));
    assert!(matches!(
        client.community_composer_draft("1", save(None, "覆盖", DraftAttachmentEdit::Keep)),
        Err(Error::StaleResponse)
    ));
    session
        .adopt(api.clone(), json!({"userId":42}), &session.token().unwrap())
        .unwrap();
    assert!(
        client
            .community_composer_draft("1", ComposerDraftAction::Read)
            .is_err()
    );
    let other = serde_json::to_value(
        client
            .community_composer_draft("2", ComposerDraftAction::Read)
            .unwrap(),
    )
    .unwrap();
    assert!(other["value"].is_null());
    assert!(matches!(
        client.community_composer_draft(
            "2",
            save(None, "别人的附件", DraftAttachmentEdit::Select { receipt })
        ),
        Err(Error::StaleResponse)
    ));

    let (_, reopened) = new_client(root.path(), api.clone(), 41);
    let restored = serde_json::to_value(
        reopened
            .community_composer_draft("1", ComposerDraftAction::Read)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(restored, first);
    assert!(
        reopened
            .community_composer_draft(
                "1",
                save(
                    Some(revision.clone()),
                    &"🎵".repeat(501),
                    DraftAttachmentEdit::Keep
                )
            )
            .is_err()
    );
    let discarded = serde_json::to_value(
        reopened
            .community_composer_draft("1", ComposerDraftAction::Discard { revision })
            .unwrap(),
    )
    .unwrap();
    assert!(discarded["value"].is_null());
    assert_eq!(
        api.0.lock().unwrap().len(),
        1,
        "draft commands never access the API"
    );
}
