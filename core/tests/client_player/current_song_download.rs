//! Sealed native PlayerBar download tests; no production account or media host.
use super::*;
use cumulus_core::{
    Result,
    account::resources::{AccountResources, ResourceKind},
    download::{directory::DownloadDirectory, service::DownloadOptions},
    playlist::PlaylistPhase,
    protocol::api::{GuardedPlaylistAdd, GuardedPlaylistCreate, PlaylistAddPermission},
};
use std::{sync::atomic::AtomicBool, time::Duration};

const COLLECT: &str = "/api/user/playlist/v2";
const LIST: &str = "/api/user/playlist";
const DETAIL: &str = "/api/v6/playlist/detail";
const ADD: &str = "/api/v1/playlist/manipulate/tracks";
const CREATE: &str = "/api/playlist/create";
const AUTH: &str = "/api/song/enhance/download/url/v1";
const NAME: &str = "合成私密下载列表";

#[derive(Default)]
pub(super) struct Server {
    enabled: AtomicBool,
    liked: AtomicBool,
    add_mode: AtomicU64,
    create_mode: AtomicU64,
    created: AtomicBool,
    present: AtomicBool,
    started: Notify,
    release: Notify,
    ledger: Mutex<Vec<(String, Value)>>,
}

impl Server {
    pub(super) fn revision(&self, revision: Option<String>) -> Option<String> {
        if self.enabled.load(Ordering::SeqCst) {
            revision
        } else {
            None
        }
    }

    fn capture(&self, path: &str, payload: &Value) {
        self.ledger
            .lock()
            .unwrap()
            .push((path.into(), payload.clone()));
    }

    fn calls(&self, path: &str) -> Vec<Value> {
        self.ledger
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == path)
            .map(|(_, body)| body.clone())
            .collect()
    }

    pub(super) fn read(&self, path: &str, payload: &Value, cache: bool) -> Option<Result<Value>> {
        if !self.enabled.load(Ordering::SeqCst) {
            return None;
        }
        let row = |id, name| json!({"id":id,"name":name,"userId":9,"creator":{"userId":9},"privacy":10,"specialType":0});
        let value = match path {
            "/api/song/like/get" => {
                assert_eq!(*payload, json!({"uid":"9"}));
                json!({"code":200,"ids":if self.liked.load(Ordering::SeqCst) { vec![1] } else { vec![] }})
            }
            COLLECT => {
                assert_eq!(
                    *payload,
                    json!({"uid":"9","offset":0,"limit":10,"sortType":"default"})
                );
                json!({"code":200,"more":false,"playlist":[row(81,"合成列表"),{"id":83,"name":"他人列表","userId":3,"privacy":0}]})
            }
            LIST => {
                let mut rows = vec![row(81, "合成列表")];
                if self.created.load(Ordering::SeqCst) {
                    rows.push(row(84, NAME));
                }
                json!({"code":200,"more":false,"playlist":rows})
            }
            DETAIL => {
                let id = payload["id"].as_str().unwrap().parse::<u64>().unwrap();
                assert!([81, 84].contains(&id));
                let mut value = row(id, if id == 81 { "合成列表" } else { NAME });
                value["tags"] = json!([]);
                value["trackIds"] = if self.present.load(Ordering::SeqCst) {
                    json!([{"id":1}])
                } else {
                    json!([])
                };
                json!({"code":200,"playlist":value})
            }
            AUTH => {
                // Record exact selected quality but never issue media bytes.
                json!({"code":200,"data":{"code":403}})
            }
            _ => return None,
        };
        assert!(!cache);
        self.capture(path, payload);
        Some(Ok(value))
    }

    pub(super) async fn add(
        &self,
        write: GuardedPlaylistAdd,
        permission: &PlaylistAddPermission<'_>,
        dispatched: &AtomicBool,
    ) -> Result<Value> {
        let mode = self.add_mode.load(Ordering::SeqCst);
        if mode == 2 {
            self.started.notify_one();
            self.release.notified().await;
        }
        permission()?;
        let (path, payload) = write.into_request()?;
        dispatched.store(true, Ordering::SeqCst);
        self.capture(path, &payload);
        if mode == 1 {
            return Err(Error::Transport(
                "synthetic lost add acknowledgement".into(),
            ));
        }
        self.present.store(true, Ordering::SeqCst);
        Ok(json!({"code":200}))
    }

    pub(super) async fn create(
        &self,
        write: GuardedPlaylistCreate,
        permission: &PlaylistAddPermission<'_>,
        dispatched: &AtomicBool,
    ) -> Result<Value> {
        permission()?;
        let (path, payload) = write.into_request();
        dispatched.store(true, Ordering::SeqCst);
        self.capture(path, &payload);
        self.created.store(true, Ordering::SeqCst);
        match self.create_mode.load(Ordering::SeqCst) {
            1 => {
                self.started.notify_one();
                self.release.notified().await;
            }
            2 => {
                return Err(Error::Transport(
                    "synthetic lost create acknowledgement".into(),
                ));
            }
            _ => {}
        }
        Ok(json!({"code":200,"id":84}))
    }
}

use CurrentSongDownloadCommand as C;
use CurrentSongDownloadDecision as D;
use CurrentSongDownloadResult as R;

fn target(view: PlayerView) -> CurrentSongDownloadTarget {
    CurrentSongDownloadTarget {
        play_id: view.play_id.unwrap(),
        entry_id: view.entry_id.unwrap(),
    }
}

async fn action(client: &MusicClient, target: &CurrentSongDownloadTarget, command: C) -> Result<R> {
    Ok(client
        .player_current_song_download("1", target, command)
        .await?
        .result)
}

fn setup(api: &Backend) -> &Server {
    api.player_song_purchase.revision.store(1, Ordering::SeqCst);
    api.current_song_download
        .enabled
        .store(true, Ordering::SeqCst);
    &api.current_song_download
}

async fn like_cache(session: &Arc<AccountSession>) {
    AccountResources::new(session.clone())
        .unwrap()
        .refresh_for(&session.token().unwrap(), ResourceKind::Liked)
        .await
        .unwrap();
}

async fn purchase(client: &MusicClient, target: &CurrentSongDownloadTarget) -> String {
    let R::Plan {
        decision: D::PurchasedQualityRequired,
        offer: Some(offer),
        purchased_qualities,
    } = action(client, target, C::Prepare).await.unwrap()
    else {
        panic!("expected purchase plan")
    };
    assert_eq!(
        purchased_qualities,
        [
            Quality::HiRes,
            Quality::Lossless,
            Quality::ExHigh,
            Quality::Standard
        ]
    );
    offer
}

async fn select(client: &MusicClient, target: &CurrentSongDownloadTarget, decision: D) -> String {
    let offer = purchase(client, target).await;
    let R::Selection {
        quality: Quality::HiRes,
        decision: found,
        selection: Some(selection),
    } = action(
        client,
        target,
        C::Select {
            offer,
            quality: Quality::HiRes,
        },
    )
    .await
    .unwrap()
    else {
        panic!("expected confirmed choice")
    };
    assert_eq!(found, decision);
    selection
}

async fn pick(client: &MusicClient, target: &CurrentSongDownloadTarget, selection: &str) -> String {
    let R::Destinations {
        rows,
        next_page: None,
        can_create: true,
    } = action(
        client,
        target,
        C::Destinations {
            selection: selection.into(),
            sort: PlaylistCollectSort::Default,
            page: 0,
        },
    )
    .await
    .unwrap()
    else {
        panic!("expected verified destination page")
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].playlist.id, "81");
    rows[0].receipt.clone()
}

fn start(selection: &str, continuation: Option<String>) -> C {
    C::Start {
        selection: selection.into(),
        continuation,
        options: DownloadOptions {
            quality: Quality::HiRes,
            ..Default::default()
        },
    }
}

async fn authorized(wire: &Server) {
    tokio::time::timeout(Duration::from_secs(4), async {
        while wire.calls(AUTH).is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        wire.calls(AUTH),
        [json!({"id":"1","level":"hires","immerseType":"c51"})]
    );
}

#[tokio::test]
async fn current_song_requires_verified_ordinary_row_and_can_refresh_exact_next_identity() {
    let (_dir, api, _, client, _) = fixture();
    let wire = setup(&api);
    api.player_song_purchase.mode.store(9, Ordering::SeqCst);
    let played = client
        .play_selection(
            "1",
            PlaySelection::Songs {
                ids: vec!["1".into(), "2".into()],
                index: 0,
                quality: Quality::Standard,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        action(&client, &target(played), C::Prepare).await.unwrap(),
        R::Plan {
            decision: D::Ordinary,
            offer: None,
            ..
        }
    ));
    let next = client.player_action("1", PlayerAction::Next).await.unwrap();
    let next = target(next);
    assert!(matches!(
        action(&client, &next, C::Prepare).await.unwrap(),
        R::Plan {
            decision: D::Ordinary,
            offer: None,
            ..
        }
    ));
    assert_eq!(
        *api.player_song_purchase.observed.lock().unwrap(),
        [vec!["1", "2"], vec!["2"]]
    );
    api.player_song_purchase.revision.store(0, Ordering::SeqCst);
    let unsigned = target(client.play_selection("1", songs("1")).await.unwrap());
    let reads = api
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|path| path.as_str() == "/api/v3/song/detail")
        .count();
    assert!(matches!(
        action(&client, &unsigned, C::Prepare).await.unwrap(),
        R::Plan {
            decision: D::Unavailable,
            offer: None,
            ..
        }
    ));
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.as_str() == "/api/v3/song/detail")
            .count(),
        reads
    );
    assert!(
        wire.calls(ADD).is_empty() && wire.calls(CREATE).is_empty() && wire.calls(AUTH).is_empty()
    );
}

#[tokio::test]
async fn pause_and_seek_reuse_song_evidence_but_expire_old_chooser() {
    let (_dir, api, _, client, _) = fixture();
    let wire = setup(&api);
    let current = target(client.play_selection("1", songs("1")).await.unwrap());
    let old_offer = purchase(&client, &current).await;
    let paused = target(
        client
            .player_action("1", PlayerAction::Pause { paused: true })
            .await
            .unwrap(),
    );
    assert_eq!(paused.play_id, current.play_id);
    assert_eq!(paused.entry_id, current.entry_id);
    assert!(matches!(
        action(
            &client,
            &paused,
            C::Select {
                offer: old_offer,
                quality: Quality::HiRes,
            },
        )
        .await,
        Err(Error::StaleResponse)
    ));
    let paused_offer = purchase(&client, &paused).await;
    let after_seek = target(
        client
            .player_action("1", PlayerAction::Seek { seconds: 7.0 })
            .await
            .unwrap(),
    );
    assert_eq!(after_seek.play_id, current.play_id);
    assert_eq!(after_seek.entry_id, current.entry_id);
    assert_ne!(purchase(&client, &after_seek).await, paused_offer);
    assert_eq!(
        *api.player_song_purchase.observed.lock().unwrap(),
        [vec!["1"]]
    );
    assert!(
        wire.calls(ADD).is_empty() && wire.calls(CREATE).is_empty() && wire.calls(AUTH).is_empty()
    );
}

#[tokio::test]
async fn current_purchased_liked_requires_same_choice_directory_and_single_admission() {
    let (_dir, api, session, client, _) = fixture();
    let wire = setup(&api);
    wire.liked.store(true, Ordering::SeqCst);
    like_cache(&session).await;
    let current = target(client.play_selection("1", songs("1")).await.unwrap());
    let offer = purchase(&client, &current).await;
    assert_eq!(
        client
            .current_song_purchased_quality_guide("1", &current, &offer)
            .unwrap(),
        "https://y.music.163.com/m/at/648fba3cbe7c73d87cbc2e81"
    );
    assert!(matches!(
        action(
            &client,
            &current,
            C::Cancel {
                receipt: offer.clone()
            }
        )
        .await
        .unwrap(),
        R::Cancelled
    ));
    assert!(
        client
            .current_song_purchased_quality_guide("1", &current, &offer)
            .is_err()
    );
    assert!(wire.calls(AUTH).is_empty());
    let selection = select(&client, &current, D::Ready).await;
    assert!(matches!(
        action(&client, &current, start(&selection, None))
            .await
            .unwrap(),
        R::DirectoryRequired
    ));
    assert!(wire.calls(AUTH).is_empty());
    let media = tempfile::tempdir().unwrap();
    DownloadDirectory::new(session.clone())
        .unwrap()
        .configure(media.path(), 2, 1)
        .unwrap();
    let wrong = C::Start {
        selection: selection.clone(),
        continuation: None,
        options: Default::default(),
    };
    assert!(matches!(
        action(&client, &current, wrong).await,
        Err(Error::InvalidInput(_))
    ));
    wire.liked.store(false, Ordering::SeqCst);
    like_cache(&session).await;
    assert!(matches!(
        action(&client, &current, start(&selection, None)).await,
        Err(Error::State("liked download admission changed"))
    ));
    assert!(wire.calls(AUTH).is_empty());
    wire.liked.store(true, Ordering::SeqCst);
    like_cache(&session).await;
    let R::Started { result } = action(&client, &current, start(&selection, None))
        .await
        .unwrap()
    else {
        panic!("expected accepted download")
    };
    assert_eq!(result.accepted.len(), 1);
    assert!(matches!(
        action(&client, &current, start(&selection, None)).await,
        Err(Error::StaleResponse)
    ));
    authorized(wire).await;
    assert!(wire.calls(COLLECT).is_empty() && wire.calls(ADD).is_empty());
}

#[tokio::test]
async fn existing_destination_lost_ack_needs_readback_and_uses_one_opaque_permit() {
    let (_dir, api, session, client, _) = fixture();
    let wire = setup(&api);
    like_cache(&session).await;
    let current = target(client.play_selection("1", songs("1")).await.unwrap());
    let selection = select(&client, &current, D::PlaylistRequired).await;
    assert!(matches!(
        action(
            &client,
            &current,
            C::ChoosePlaylist {
                selection: selection.clone(),
                destination: "81".into()
            }
        )
        .await,
        Err(Error::StaleResponse)
    ));
    let destination = pick(&client, &current, &selection).await;
    wire.add_mode.store(1, Ordering::SeqCst);
    let R::Picker { picker } = action(
        &client,
        &current,
        C::ChoosePlaylist {
            selection: selection.clone(),
            destination,
        },
    )
    .await
    .unwrap() else {
        panic!("expected picker receipt")
    };
    assert!(picker.phase.pending());
    assert!(picker.continuation.is_none());
    assert!(matches!(
        action(&client, &current, C::Prepare).await,
        Err(Error::MutationUncertain)
    ));
    assert!(matches!(
        action(&client, &current, start(&selection, Some("forged".into()))).await,
        Err(Error::StaleResponse)
    ));
    wire.present.store(true, Ordering::SeqCst);
    let R::Picker { picker } = action(
        &client,
        &current,
        C::PickerStatus {
            selection: selection.clone(),
            reconcile: true,
        },
    )
    .await
    .unwrap() else {
        panic!("expected read-only result")
    };
    assert_eq!(picker.phase, PlaylistPhase::Confirmed);
    assert!(picker.continuation.is_some());
    assert_eq!(
        wire.calls(ADD),
        [json!({"pid":"81","trackIds":"[\"1\"]","op":"add"})]
    );
    let media = tempfile::tempdir().unwrap();
    DownloadDirectory::new(session.clone())
        .unwrap()
        .configure(media.path(), 2, 1)
        .unwrap();
    assert!(matches!(
        action(&client, &current, start(&selection, picker.continuation))
            .await
            .unwrap(),
        R::Started { .. }
    ));
    authorized(wire).await;
    assert_eq!(wire.calls(ADD).len(), 1);
}

#[tokio::test]
async fn changed_credential_before_destination_dispatch_sends_no_add() {
    let (_dir, api, session, client, _) = fixture();
    let wire = setup(&api);
    like_cache(&session).await;
    let current = target(client.play_selection("1", songs("1")).await.unwrap());
    let selection = select(&client, &current, D::PlaylistRequired).await;
    let destination = pick(&client, &current, &selection).await;
    wire.add_mode.store(2, Ordering::SeqCst);
    let (result, ()) = tokio::join!(
        action(
            &client,
            &current,
            C::ChoosePlaylist {
                selection,
                destination
            }
        ),
        async {
            tokio::time::timeout(Duration::from_secs(4), wire.started.notified())
                .await
                .unwrap();
            api.player_song_purchase.revision.store(2, Ordering::SeqCst);
            wire.release.notify_one();
        }
    );
    assert!(result.is_err());
    assert!(wire.calls(ADD).is_empty() && wire.calls(AUTH).is_empty());
}

#[tokio::test]
async fn changed_current_song_after_create_ack_cannot_chain_the_add() {
    let (_dir, api, session, client, _) = fixture();
    let wire = setup(&api);
    like_cache(&session).await;
    let current = target(client.play_selection("1", songs("1")).await.unwrap());
    let selection = select(&client, &current, D::PlaylistRequired).await;
    wire.create_mode.store(1, Ordering::SeqCst);
    let (result, ()) = tokio::join!(
        action(
            &client,
            &current,
            C::CreatePlaylist {
                selection,
                name: NAME.into(),
                private: true
            }
        ),
        async {
            tokio::time::timeout(Duration::from_secs(4), wire.started.notified())
                .await
                .unwrap();
            let new_song = client.play_selection("1", songs("2")).await.unwrap();
            assert_eq!(new_song.track.unwrap().id, "2");
            wire.release.notify_one();
        }
    );
    assert!(matches!(result, Err(Error::StaleResponse)));
    assert_eq!(
        wire.calls(CREATE),
        [json!({"uid":"9","name":NAME,"privacy":10})]
    );
    assert!(wire.calls(ADD).is_empty() && wire.calls(AUTH).is_empty());
}

#[tokio::test]
async fn new_destination_only_chains_add_from_its_direct_created_id() {
    for lost in [false, true] {
        let (_dir, api, session, client, _) = fixture();
        let wire = setup(&api);
        like_cache(&session).await;
        let current = target(client.play_selection("1", songs("1")).await.unwrap());
        let selection = select(&client, &current, D::PlaylistRequired).await;
        if lost {
            wire.create_mode.store(2, Ordering::SeqCst);
        }
        let R::Creation { creation } = action(
            &client,
            &current,
            C::CreatePlaylist {
                selection: selection.clone(),
                name: NAME.into(),
                private: true,
            },
        )
        .await
        .unwrap() else {
            panic!("expected creation result");
        };
        assert_eq!(
            wire.calls(CREATE),
            [json!({"uid":"9","name":NAME,"privacy":10})]
        );
        if lost {
            assert!(creation.add.is_none() && creation.playlist_id.is_none());
            let R::Creation { creation } = action(
                &client,
                &current,
                C::CreationStatus {
                    selection: selection.clone(),
                    reconcile: true,
                },
            )
            .await
            .unwrap() else {
                panic!("expected creation status");
            };
            assert!(!creation.can_resume_add && creation.add.is_none());
            assert!(
                action(&client, &current, C::AddCreatedPlaylist { selection })
                    .await
                    .is_err()
            );
            assert!(wire.calls(ADD).is_empty());
        } else {
            assert_eq!(creation.create_phase, PlaylistPhase::Confirmed);
            assert_eq!(creation.playlist_id.as_deref(), Some("84"));
            let add = creation.add.unwrap();
            assert!(add.continuation.is_some() && !add.add_failed);
            assert_eq!(
                wire.calls(ADD),
                [json!({"pid":"84","trackIds":"[\"1\"]","op":"add"})]
            );
        }
        assert!(wire.calls(AUTH).is_empty());
    }
}

#[tokio::test]
async fn signed_nonpurchase_catalogue_fee_uses_ordinary_but_unentitled_rows_cannot() {
    let (_dir, api, _, client, _) = fixture();
    let wire = setup(&api);
    // M/7 fee bits and M/3085’s two independent native decisions. The
    // authenticated synthetic account has no membership assignment snapshot.
    for (row, expected) in [
        (json!({"fee":1,"payed":1}), D::Ordinary),
        (json!({"fee":2,"payed":1}), D::Ordinary),
        (json!({"fee":4,"payed":1}), D::Ordinary),
        (json!({"fee":8,"payed":1}), D::Ordinary),
        (json!({"fee":16,"payed":1}), D::Ordinary),
        (json!({"fee":32,"payed":1}), D::Ordinary),
        (json!({"fee":63,"payed":9}), D::Ordinary),
        (json!({"fee":0,"payed":1}), D::Ordinary),
        (json!({"fee":1,"payed":0}), D::Unavailable),
        (json!({"fee":8,"payed":null}), D::Unavailable),
        (json!({"fee":64,"payed":1}), D::Unavailable),
        (json!({"fee":1,"payed":"1"}), D::Unavailable),
        (json!({"fee":1,"payed":1,"st":-200}), D::Unavailable),
        (json!({"fee":1,"payed":1,"dl":0}), D::Unavailable),
        (json!({"fee":1,"payed":1,"bd":1}), D::Unavailable),
        (json!({"fee":1,"payed":1,"bd":""}), D::Unavailable),
        (json!({"fee":8,"payed":3}), D::PurchasedQualityRequired),
        (json!({"fee":1,"payed":4}), D::PurchasedQualityRequired),
    ] {
        *api.player_song_purchase.privilege_override.lock().unwrap() = Some(row.clone());
        let current = target(client.play_selection("1", songs("1")).await.unwrap());
        let R::Plan {
            decision,
            offer,
            purchased_qualities,
        } = action(&client, &current, C::Prepare).await.unwrap()
        else {
            panic!("expected native preparation for {row}");
        };
        assert_eq!(decision, expected, "{row}");
        let chooser = expected == D::PurchasedQualityRequired;
        assert_eq!(offer.is_some(), chooser, "{row}");
        assert_eq!(!purchased_qualities.is_empty(), chooser, "{row}");
    }
    assert!(
        wire.calls(ADD).is_empty() && wire.calls(CREATE).is_empty() && wire.calls(AUTH).is_empty()
    );
}
