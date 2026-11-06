use cumulus_core::{
    Error,
    client::{
        ClientError, MusicClient,
        dto::{
            AccountData, LikeResult, PlayerView, PlayerVoiceSongsView, PodcastMutationView,
            VinylVoiceDetailTarget, VinylVoiceDetailView, VinylVoiceRecommendationsView,
            VoiceSongAction, VoiceSongSelectionAction,
        },
    },
    membership::Quality,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceDetailRouteTarget {
    epoch: String,
    play_id: String,
    receipt: String,
    kind: VinylVoiceDetailTarget,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceDetailActionScope {
    epoch: String,
    play_id: String,
    receipt: String,
}

fn voice_detail_action_error(error: Error) -> ClientError {
    // A bare HTTP failure can leave a dispatched write uncertain. Only an
    // explicit service refusal or a native permission check proves denial.
    if matches!(
        error.root(),
        Error::InvalidInput(
            "cannot subscribe to an owned podcast" | "podcast search subscription unavailable"
        ) | Error::State("no authenticated account" | "no verified account profile")
    ) || error.is_service_code(403)
    {
        return ClientError {
            code: "voice_detail_forbidden",
            message: "当前账号无法执行这项节目操作",
            retryable: false,
        };
    }
    error.into()
}

pub(super) fn require_main<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
) -> Result<(), ClientError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(Error::InvalidInput("main window required").into())
    }
}

#[tauri::command]
pub async fn player_vinyl_voice_detail<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<VinylVoiceDetailView, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_voice_detail(&epoch, &play_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn player_vinyl_voice_detail_target<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    target: VoiceDetailRouteTarget,
) -> Result<String, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_voice_detail_target(
            &target.epoch,
            &target.play_id,
            &target.receipt,
            target.kind,
        )
        .map_err(voice_detail_action_error)
}

#[tauri::command]
pub async fn change_fresh_vinyl_podcast_subscription<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    scope: VoiceDetailActionScope,
    expected: bool,
) -> Result<AccountData<PodcastMutationView>, ClientError> {
    require_main(&window)?;
    client
        .change_fresh_vinyl_podcast_subscription(
            &scope.epoch,
            &scope.play_id,
            &scope.receipt,
            expected,
        )
        .await
        .map_err(voice_detail_action_error)
}

#[tauri::command]
pub async fn reconcile_fresh_vinyl_podcast_subscription<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    scope: VoiceDetailActionScope,
) -> Result<AccountData<PodcastMutationView>, ClientError> {
    require_main(&window)?;
    client
        .reconcile_fresh_vinyl_podcast_subscription(&scope.epoch, &scope.play_id, &scope.receipt)
        .await
        .map_err(voice_detail_action_error)
}

#[tauri::command]
pub async fn player_vinyl_voice_recommendations<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<VinylVoiceRecommendationsView, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_voice_recommendations(&epoch, &play_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub fn player_vinyl_voice_recommendations_next<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
) -> Result<VinylVoiceRecommendationsView, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_voice_recommendations_next(&epoch, &play_id, &receipt)
        .map_err(Into::into)
}

#[tauri::command]
pub fn player_vinyl_voice_recommendation_target<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
    page: usize,
    radio_id: String,
) -> Result<String, ClientError> {
    require_main(&window)?;
    client
        .player_vinyl_voice_recommendation_target(&epoch, &play_id, &receipt, page, &radio_id)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_songs<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
) -> Result<PlayerVoiceSongsView, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs(&epoch, &play_id)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_play<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
    action: VoiceSongAction,
    quality: Quality,
) -> Result<PlayerView, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_play(&epoch, &play_id, &receipt, action, quality)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_songs_selection<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
    action: VoiceSongSelectionAction,
    quality: Quality,
) -> Result<PlayerView, ClientError> {
    require_main(&window)?;
    client
        .player_voice_songs_selection(&epoch, &play_id, &receipt, action, quality)
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub async fn player_voice_song_like<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    epoch: String,
    play_id: String,
    receipt: String,
    index: usize,
    liked: bool,
) -> Result<AccountData<LikeResult>, ClientError> {
    require_main(&window)?;
    client
        .player_voice_song_like(&epoch, &play_id, &receipt, index, liked)
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fresh_voice_ipc_rejects_an_external_id_or_unknown_route_kind() {
        let scope = json!({"epoch":"1","playId":"live","receipt":"opaque"});
        let mut target = scope.clone();
        target["kind"] = json!("radio");
        assert!(serde_json::from_value::<VoiceDetailRouteTarget>(target.clone()).is_ok());
        target["radioId"] = json!("999");
        assert!(serde_json::from_value::<VoiceDetailRouteTarget>(target).is_err());
        let mut target = scope.clone();
        target["kind"] = json!("arbitrary_url");
        assert!(serde_json::from_value::<VoiceDetailRouteTarget>(target).is_err());
        let mut scope = scope;
        scope["radioId"] = json!("999");
        assert!(serde_json::from_value::<VoiceDetailActionScope>(scope).is_err());
    }

    #[test]
    fn fresh_voice_ipc_preserves_permission_session_and_journal_error_classes() {
        for (error, code, retryable) in [
            (Error::StaleResponse, "stale", false),
            (
                Error::HttpStatus {
                    status: 403,
                    endpoint: None,
                    body: Some(Box::new(json!({"code":301,"private":"DO NOT PROJECT"}))),
                },
                "session_expired",
                false,
            ),
            (
                Error::InvalidInput("cannot subscribe to an owned podcast"),
                "voice_detail_forbidden",
                false,
            ),
            (
                Error::service("synthetic", json!(403), json!({"private":"DO NOT PROJECT"})),
                "voice_detail_forbidden",
                false,
            ),
            (
                Error::HttpStatus {
                    status: 403,
                    endpoint: None,
                    body: None,
                },
                "network",
                true,
            ),
            (Error::MutationUncertain, "write_uncertain", false),
            (
                Error::RequestNotSent(Box::new(Error::Transport("DO NOT PROJECT".into()))),
                "request_not_sent",
                true,
            ),
        ] {
            let public = voice_detail_action_error(error);
            assert_eq!((public.code, public.retryable), (code, retryable));
            assert!(
                !serde_json::to_string(&public)
                    .unwrap()
                    .contains("DO NOT PROJECT")
            );
        }
    }
}
