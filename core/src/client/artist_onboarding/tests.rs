use super::*;
use crate::{
    account::{AccountSession, artist_onboarding::ArtistLogin, profiles::ProfileRegistry},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::Value;
use std::sync::Arc;

#[derive(Default)]
struct Backend {
    routes: Mutex<Vec<String>>,
    lost_save: bool,
}
impl Api for Backend {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            self.routes.lock().unwrap().push(path.into());
            match path {
                "/api/w/nuser/account/get" => {
                    assert_eq!(args, json!({}));
                    Ok(
                        json!({"code":200,"profile":{"userId":11,"nickname":"Synthetic"},
                        "account":{"id":11,"type":10,"userName":"private-qr-account"},
                        "loginExtData":{"actionParam":{"targetUrl":"orpheus://appModal?comName=RecallUserFavorModal"}}}),
                    )
                }
                "/api/recall/user/favor/rcmd-artists" => {
                    assert_eq!(args, json!({}));
                    Ok(
                        json!({"code":200,"data":{"usedListenedArtists":[{"artistId":701,"artistName":"First"}],
                        "recentHotArtists":[{"artistId":701,"artistName":"Duplicate"},{"id":702,"name":"Second"}]}}),
                    )
                }
                "/api/newuser/favor/rcmd-artists/save" => {
                    assert_eq!(args, json!({"artistIds":[701,702]}));
                    if self.lost_save {
                        Err(Error::Transport("lost synthetic receipt".into()))
                    } else {
                        Ok(json!({"code":200}))
                    }
                }
                "/api/recall/user/favor/artist/song" => {
                    assert_eq!(args, json!({"artistIds":[701,702]}));
                    Ok(json!({"code":200,"data":{"sortedSongIds":[2,1,2]}}))
                }
                "/api/v3/song/detail" => {
                    assert_eq!(
                        serde_json::from_str::<Value>(args["c"].as_str().unwrap()).unwrap(),
                        json!([{"id":"2","v":0},{"id":"1","v":0}])
                    );
                    Ok(
                        json!({"code":200,"songs":[{"id":1,"name":"Second"},{"id":2,"name":"First"}]}),
                    )
                }
                _ => panic!("unexpected artist onboarding request: {path}"),
            }
        })
    }
}

fn verified(
    session: &AccountSession,
    registry: &ProfileRegistry,
    api: Arc<dyn Api>,
    uid: u64,
    scene: Option<Scene>,
) -> String {
    session
        .adopt_registered(
            api,
            crate::account::identity::VerifiedAccount::profile_only(
                json!({"userId":uid,"nickname":"Synthetic"}),
            ),
            &session.token().unwrap(),
            &registry.store(),
            Some(ArtistLogin { scene }),
            |_| Ok(()),
        )
        .unwrap();
    session.token().unwrap().generation.to_string()
}

#[tokio::test]
async fn exact_qr_extension_does_not_escape_or_trigger_from_ordinary_profile_refresh() {
    let api = Arc::new(Backend::default());
    let (identity, evidence) = crate::account::artist_onboarding::qr_identity(&*api)
        .await
        .unwrap();
    assert_eq!(identity.profile["userId"], 11);
    assert_eq!(identity.registration_type, Some(10));
    assert!(identity.profile.get("loginExtData").is_none());
    assert!(identity.profile.get("account").is_none());
    assert_eq!(evidence.scene, Some(Scene::RecallUser));
    for target in [
        "https://example.invalid/?comName=NewUserFavorModal",
        "orpheus://appModal?comName=RecallUserFavorModal&extra=1",
        "orpheus://appModal?comName=NEWUSERFAVORMODAL",
        "orpheus://appModal?comName=NewUserFavorModal/",
    ] {
        assert!(
            ArtistLogin::from_reply(&json!({"loginExtData":{"actionParam":{"targetUrl":target}}}))
                .scene
                .is_none()
        );
    }
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    session.refresh().await.unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let client = MusicClient::with_session(session, registry);
    assert!(
        client
            .artist_onboarding_offer(&epoch)
            .unwrap()
            .value
            .is_none()
    );
    assert_eq!(
        &*api.routes.lock().unwrap(),
        &["/api/w/nuser/account/get"; 2]
    );
}

#[tokio::test]
async fn same_uid_skipped_offer_recovers_once_and_only_inside_week() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Backend::default());
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session.clone(), registry.clone());
    let epoch = verified(
        &session,
        &registry,
        api.clone(),
        11,
        Some(Scene::RecallUser),
    );
    let first = client
        .artist_onboarding_offer(&epoch)
        .unwrap()
        .value
        .unwrap();
    assert!(!first.resumed);
    client.artist_onboarding_close(&epoch, &first.id).unwrap();
    assert!(
        client
            .artist_onboarding_offer(&epoch)
            .unwrap()
            .value
            .is_none()
    );
    let other = verified(&session, &registry, api.clone(), 12, None);
    assert!(
        client
            .artist_onboarding_offer(&other)
            .unwrap()
            .value
            .is_none()
    );
    let next = verified(&session, &registry, api.clone(), 11, None);
    let second = client
        .artist_onboarding_offer(&next)
        .unwrap()
        .value
        .unwrap();
    assert!(second.resumed);
    assert_eq!(second.scene, Scene::RecallUser);
    assert_ne!(first.id, second.id);
    client.artist_onboarding_close(&next, &second.id).unwrap();
    let next = verified(&session, &registry, api.clone(), 11, None);
    assert!(
        client
            .artist_onboarding_offer(&next)
            .unwrap()
            .value
            .is_none()
    );
    registry
        .store()
        .put(
            "11",
            KIND,
            "resume",
            &json!({"scene":"recall_user","aborted":time().unwrap()-WEEK-1}),
        )
        .unwrap();
    let next = verified(&session, &registry, api.clone(), 11, None);
    assert!(
        client
            .artist_onboarding_offer(&next)
            .unwrap()
            .value
            .is_none()
    );
    assert!(api.routes.lock().unwrap().is_empty());
    // A process restoring this same authenticated UID can consume only the
    // durable local second-entry record, not a fresh target in account/get.
    registry
        .store()
        .put(
            "11",
            KIND,
            "resume",
            &json!({"scene":"new_user","aborted":time().unwrap()}),
        )
        .unwrap();
    let restored_session = Arc::new(AccountSession::new(api, Some(registry.store())));
    restored_session.refresh().await.unwrap();
    let epoch = restored_session.token().unwrap().generation.to_string();
    let restored = MusicClient::with_session(restored_session, registry);
    let local = restored
        .artist_onboarding_offer(&epoch)
        .unwrap()
        .value
        .unwrap();
    assert!(local.resumed);
    assert_eq!(local.scene, Scene::NewUser);
}

#[tokio::test]
async fn unknown_save_is_durable_and_preview_only_with_exact_selection() {
    let root = tempfile::tempdir().unwrap();
    let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
    let api = Arc::new(Backend {
        lost_save: true,
        ..Default::default()
    });
    let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
    let client = MusicClient::with_session(session.clone(), registry.clone());
    let epoch = verified(
        &session,
        &registry,
        api.clone(),
        11,
        Some(Scene::RecallUser),
    );
    let offer = client
        .artist_onboarding_offer(&epoch)
        .unwrap()
        .value
        .unwrap();
    let artists = vec!["701".to_owned(), "702".to_owned()];
    assert!(
        client
            .artist_onboarding_save(&epoch, &offer.id, artists.clone())
            .await
            .is_err()
    );
    let rows = client
        .artist_onboarding_candidates(&epoch, &offer.id, None)
        .await
        .unwrap()
        .value;
    assert_eq!(
        rows.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        ["701", "702"]
    );
    assert!(
        client
            .artist_onboarding_preview(&epoch, &offer.id)
            .await
            .is_err()
    );
    assert!(
        client
            .artist_onboarding_save(&epoch, &offer.id, vec!["701".into(), "701".into()])
            .await
            .is_err()
    );
    assert!(
        client
            .artist_onboarding_save(&epoch, &offer.id, vec!["701".into(), "801".into()])
            .await
            .is_err()
    );
    assert_eq!(
        client
            .artist_onboarding_save(&epoch, &offer.id, artists.clone())
            .await
            .unwrap()
            .value,
        ArtistSave::Uncertain
    );
    assert!(
        client
            .artist_onboarding_save(&epoch, &offer.id, artists.clone())
            .await
            .is_err()
    );
    let preview = client
        .artist_onboarding_preview(&epoch, &offer.id)
        .await
        .unwrap()
        .value;
    assert_eq!(
        preview
            .iter()
            .map(|track| track.id.as_str())
            .collect::<Vec<_>>(),
        ["2", "1"]
    );
    client.artist_onboarding_close(&epoch, &offer.id).unwrap();
    let second = MusicClient::with_session(session.clone(), registry.clone());
    let next = verified(
        &session,
        &registry,
        api.clone(),
        11,
        Some(Scene::RecallUser),
    );
    let offered = second
        .artist_onboarding_offer(&next)
        .unwrap()
        .value
        .unwrap();
    assert!(offered.save_blocked);
    assert!(
        second
            .artist_onboarding_save(&next, &offered.id, artists)
            .await
            .is_err()
    );
    assert_eq!(
        &*api.routes.lock().unwrap(),
        &[
            "/api/recall/user/favor/rcmd-artists",
            "/api/newuser/favor/rcmd-artists/save",
            "/api/recall/user/favor/artist/song",
            "/api/v3/song/detail",
        ]
    );
}
