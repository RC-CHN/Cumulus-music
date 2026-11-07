//! Main-window transport for a prepared SongCard download group.
use super::{voice_related::require_main, voice_song_download::purchased::PurchasedQuality};
use cumulus_core::{
    Error,
    client::{
        ClientError, MusicClient,
        dto::{
            DownloadResult, VoiceSongGroupDownloadConfirmation, VoiceSongGroupDownloadDecision,
            VoiceSongGroupDownloadPlan,
        },
    },
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tauri::ipc::{InvokeBody, Request};

#[derive(Deserialize)]
#[serde(try_from = "String")]
struct Receipt(String);

impl TryFrom<String> for Receipt {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            Ok(Self(value))
        } else {
            Err("invalid native SongCard receipt")
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepare {
    epoch: String,
    play_id: String,
    card_receipt: Receipt,
    anchor_index: usize,
    indices: Vec<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepared {
    epoch: String,
    play_id: String,
    card_receipt: Receipt,
    selection_receipt: Receipt,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Confirm {
    epoch: String,
    play_id: String,
    card_receipt: Receipt,
    selection_receipt: Receipt,
    quality: PurchasedQuality,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Plan {
    epoch: String,
    play_id: String,
    anchor_index: usize,
    eligible_indices: Vec<usize>,
    omitted_indices: Vec<usize>,
    decision: VoiceSongGroupDownloadDecision,
    selection_receipt: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    purchased_qualities: Vec<PurchasedQuality>,
}

impl TryFrom<VoiceSongGroupDownloadPlan> for Plan {
    type Error = Error;

    fn try_from(plan: VoiceSongGroupDownloadPlan) -> Result<Self, Self::Error> {
        use VoiceSongGroupDownloadDecision as D;
        let invalid = || Error::State("invalid native SongCard group download plan");
        let purchased = plan.decision == D::PurchasedQualityRequired;
        let prepared = purchased || plan.decision == D::Ready;
        let has_qualities = !plan.purchased_qualities.is_empty();
        if purchased != has_qualities
            || prepared != plan.selection_receipt.is_some()
            || (prepared && plan.eligible_indices.is_empty())
            || plan
                .purchased_qualities
                .windows(2)
                .any(|pair| pair[0].bitrate() <= pair[1].bitrate())
        {
            return Err(invalid());
        }
        let purchased_qualities = plan
            .purchased_qualities
            .into_iter()
            .map(|quality| PurchasedQuality::try_from(quality).map_err(|_| invalid()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            epoch: plan.epoch,
            play_id: plan.play_id,
            anchor_index: plan.anchor_index,
            eligible_indices: plan.eligible_indices,
            omitted_indices: plan.omitted_indices,
            decision: plan.decision,
            selection_receipt: plan.selection_receipt,
            purchased_qualities,
        })
    }
}

fn decode<T: DeserializeOwned>(body: &InvokeBody) -> Result<T, ClientError> {
    let invalid = || Error::InvalidInput("invalid SongCard group download request").into();
    // Ordinary Tauri argument extraction ignores unknown top-level keys.
    // These commands accept exactly one flat, typed request as a whole.
    match body {
        InvokeBody::Json(value) if value.is_object() => {
            T::deserialize(value).map_err(|_| invalid())
        }
        _ => Err(invalid()),
    }
}

#[tauri::command]
pub(crate) async fn player_voice_songs_download_prepare<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<Plan, ClientError> {
    require_main(&window)?;
    let input: Prepare = decode(request.body())?;
    let plan = client
        .player_voice_songs_download_prepare(
            &input.epoch,
            &input.play_id,
            &input.card_receipt.0,
            input.anchor_index,
            &input.indices,
        )
        .await?;
    Plan::try_from(plan).map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn player_voice_songs_download_cancel<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<(), ClientError> {
    require_main(&window)?;
    let input: Prepared = decode(request.body())?;
    client
        .player_voice_songs_download_cancel(
            &input.epoch,
            &input.play_id,
            &input.card_receipt.0,
            &input.selection_receipt.0,
        )
        .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn player_voice_songs_download_start<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<DownloadResult, ClientError> {
    require_main(&window)?;
    let input: Prepared = decode(request.body())?;
    client
        .player_voice_songs_download_start(
            &input.epoch,
            &input.play_id,
            &input.card_receipt.0,
            &input.selection_receipt.0,
        )
        .await
        .map_err(Into::into)
}

#[tauri::command]
pub(crate) async fn player_voice_songs_download_confirm_quality<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    client: tauri::State<'_, MusicClient>,
    request: Request<'_>,
) -> Result<VoiceSongGroupDownloadConfirmation, ClientError> {
    require_main(&window)?;
    let input: Confirm = decode(request.body())?;
    client
        .player_voice_songs_download_confirm_quality(
            &input.epoch,
            &input.play_id,
            &input.card_receipt.0,
            &input.selection_receipt.0,
            input.quality.into(),
        )
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cumulus_core::membership::Quality;
    use serde_json::json;

    fn native(
        decision: VoiceSongGroupDownloadDecision,
        receipt: Option<&str>,
        purchased_qualities: Vec<Quality>,
    ) -> VoiceSongGroupDownloadPlan {
        VoiceSongGroupDownloadPlan {
            epoch: "1".into(),
            play_id: "play".into(),
            anchor_index: 0,
            eligible_indices: vec![2, 0],
            omitted_indices: vec![1],
            decision,
            selection_receipt: receipt.map(str::to_owned),
            purchased_qualities,
        }
    }

    #[test]
    fn purchased_group_plan_only_projects_explicit_tiers_in_native_order() {
        use Quality::*;
        use VoiceSongGroupDownloadDecision as D;
        const RECEIPT: &str = "abcdef0123456789abcdef0123456789";
        let ordinary =
            serde_json::to_value(Plan::try_from(native(D::Ready, Some(RECEIPT), vec![])).unwrap())
                .unwrap();
        assert_eq!(
            ordinary,
            json!({
                "epoch":"1", "playId":"play", "anchorIndex":0,
                "eligibleIndices":[2,0], "omittedIndices":[1],
                "decision":"ready", "selectionReceipt":RECEIPT
            })
        );
        let mut expected = ordinary;
        expected["decision"] = json!("purchased_quality_required");
        expected["purchasedQualities"] = json!(["sky", "hires", "standard"]);
        assert_eq!(
            serde_json::to_value(
                Plan::try_from(native(
                    D::PurchasedQualityRequired,
                    Some(RECEIPT),
                    vec![Sky, HiRes, Standard],
                ))
                .unwrap()
            )
            .unwrap(),
            expected,
        );
        for (decision, receipt, tiers) in [
            (D::PurchasedQualityRequired, None, vec![Sky]),
            (D::PurchasedQualityRequired, Some(RECEIPT), vec![]),
            (D::PurchasedQualityRequired, Some(RECEIPT), vec![Dolby]),
            (D::PurchasedQualityRequired, Some(RECEIPT), vec![Sky, Sky]),
            (D::PurchasedQualityRequired, Some(RECEIPT), vec![HiRes, Sky]),
            (D::Ready, Some(RECEIPT), vec![Sky]),
            (D::PlaylistRequired, Some(RECEIPT), vec![]),
        ] {
            assert!(Plan::try_from(native(decision, receipt, tiers)).is_err());
        }
    }
}
