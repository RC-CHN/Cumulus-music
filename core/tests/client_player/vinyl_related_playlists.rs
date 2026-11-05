use super::*;
use cumulus_core::{client::VinylSimilarAction, protocol::api::VinylRelatedPlaylistsRead};
use std::sync::atomic::AtomicBool;

#[derive(Default)]
pub(super) struct Server {
    calls: Mutex<Vec<Value>>,
    response: Mutex<Option<Value>>,
    hold: AtomicBool,
    started: Notify,
    release: Notify,
}

impl Server {
    pub(super) async fn response(
        &self,
        read: VinylRelatedPlaylistsRead,
    ) -> cumulus_core::Result<Value> {
        let (path, args) = read.into_request();
        assert_eq!(path, "/api/discovery/simiPlaylist");
        self.calls.lock().unwrap().push(args);
        if self.hold.swap(false, Ordering::SeqCst) {
            self.started.notify_one();
            self.release.notified().await;
        }
        let default = || {
            json!({"code":200,"playlists":[
                {"id":"90","name":"第一张","title":"应忽略","playCount":192,"coverImgUrl":"http://p1.music.126.net/card.jpg?private=NOT_PUBLIC#ignored","picUrl":"https://p2.music.126.net/fallback.jpg","authorization":"NOT_PUBLIC"},
                {"id":12,"name":"","title":"备用标题","playCount":"50","picUrl":"https://p2.music.126.net/second.jpg","coverUrl":"https://p3.music.126.net/third.jpg"},
                {"id":37,"name":"第三张","coverUrl":"https://p3.music.126.net/third.jpg"},
                {"id":28,"name":"第四张","coverImgUrl":"https://outside.invalid/private","picUrl":"https://p1.music.126.net/ignored.jpg"},
                {"id":63,"name":"第五张","playCount":-7},
                {"id":53,"name":"第六张"},
                {"id":81,"name":"第七张","playCount":9007199254740992_u64},
                {"id":2,"name":"第八张"}
            ]})
        };
        Ok(self
            .response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(default))
    }
}

async fn current_play(client: &MusicClient, id: &str) -> String {
    client
        .play_selection("1", songs(id))
        .await
        .unwrap()
        .play_id
        .unwrap()
}

#[tokio::test]
async fn vinyl_related_playlists_keep_service_order_exact_seed_and_only_safe_card_fields() {
    let (_dir, api, _, client, _) = fixture();
    let play_id = current_play(&client, "2").await;
    let server = &api.vinyl_related_playlists;
    api.vinyl_comment_group.store(2, Ordering::SeqCst);
    assert!(
        client
            .player_vinyl_related_playlists("1", &play_id)
            .await
            .is_err()
    );
    assert!(server.calls.lock().unwrap().is_empty());

    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    assert!(matches!(
        client.player_vinyl_related_playlists("1", "old").await,
        Err(Error::StaleResponse)
    ));
    let voice = client
        .play_selection("1", PlaySelection::Voice { id: "81".into() })
        .await
        .unwrap();
    assert!(
        client
            .player_vinyl_related_playlists("1", voice.play_id.as_deref().unwrap())
            .await
            .is_err()
    );
    assert!(server.calls.lock().unwrap().is_empty());

    let play_id = current_play(&client, "2").await;
    for _ in 0..2 {
        let value = client
            .player_vinyl_related_playlists("1", &play_id)
            .await
            .unwrap();
        assert_eq!(value.play_id, play_id);
        assert_eq!(value.song_id, "2");
        let cards = &value.playlists;
        assert_eq!(
            cards.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            ["90", "12", "37", "28", "63", "53", "81", "2"]
        );
        assert_eq!(cards[0].name, "第一张");
        assert_eq!(
            cards[0].cover_url.as_deref(),
            Some("https://p1.music.126.net/card.jpg?param=320y320")
        );
        assert_eq!(cards[1].name, "备用标题");
        assert_eq!(
            cards[1].cover_url.as_deref(),
            Some("https://p2.music.126.net/second.jpg?param=320y320")
        );
        assert_eq!(
            cards[2].cover_url.as_deref(),
            Some("https://p3.music.126.net/third.jpg?param=320y320")
        );
        assert!(cards[3].cover_url.is_none());
        assert_eq!(
            cards.iter().map(|row| row.play_count).collect::<Vec<_>>(),
            [192, 50, 0, 0, 0, 0, 0, 0]
        );
        let encoded = serde_json::to_value(value).unwrap();
        assert_eq!(
            encoded["playlists"][0],
            json!({"id":"90","name":"第一张","coverUrl":"https://p1.music.126.net/card.jpg?param=320y320","playCount":192})
        );
        assert!(!encoded.to_string().contains("NOT_PUBLIC"));
    }
    assert_eq!(
        *server.calls.lock().unwrap(),
        [json!({"songid":"2"}), json!({"songid":"2"})]
    );
}

#[tokio::test]
async fn vinyl_related_playlist_empty_or_failed_reads_preserve_the_sibling_song_receipt() {
    let (_dir, api, _, client, _) = fixture();
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    let play_id = current_play(&client, "2").await;
    let songs = client
        .player_vinyl_similar_songs("1", &play_id)
        .await
        .unwrap();
    let server = &api.vinyl_related_playlists;
    *server.response.lock().unwrap() = Some(json!({"code":200,"playlists":[]}));
    assert!(
        client
            .player_vinyl_related_playlists("1", &play_id)
            .await
            .unwrap()
            .playlists
            .is_empty()
    );
    for invalid in [
        json!({"code":503,"message":"synthetic failure"}),
        json!({"code":200,"data":{"playlists":[]}}),
        json!({"code":200,"playlists":[{"id":"../external"}]}),
    ] {
        *server.response.lock().unwrap() = Some(invalid);
        assert!(
            client
                .player_vinyl_related_playlists("1", &play_id)
                .await
                .is_err()
        );
    }
    let action = client
        .player_vinyl_similar_play(
            "1",
            &play_id,
            &songs.receipt.unwrap(),
            VinylSimilarAction::Header,
            Quality::Standard,
        )
        .await
        .unwrap();
    assert_eq!(action.track.unwrap().id, "4");
    assert_eq!(client.player_queue("1").await.unwrap().tracks.len(), 3);
}

#[tokio::test]
async fn vinyl_related_playlist_delayed_reads_are_retired_on_track_credential_or_account_change() {
    let (_dir, api, session, client, _) = fixture();
    api.vinyl_comment_group.store(1, Ordering::SeqCst);
    let server = &api.vinyl_related_playlists;
    for reason in ["track", "credentials", "account"] {
        let play_id = current_play(&client, "2").await;
        server.hold.store(true, Ordering::SeqCst);
        let read = tokio::spawn({
            let client = client.clone();
            async move { client.player_vinyl_related_playlists("1", &play_id).await }
        });
        server.started.notified().await;
        match reason {
            "track" => {
                current_play(&client, "3").await;
            }
            "credentials" => api.vinyl_comment_group.store(3, Ordering::SeqCst),
            "account" => {
                session
                    .adopt(api.clone(), json!({"userId":19}), &session.token().unwrap())
                    .unwrap();
            }
            _ => unreachable!(),
        }
        server.release.notify_one();
        assert!(matches!(read.await.unwrap(), Err(Error::StaleResponse)));
    }
}
