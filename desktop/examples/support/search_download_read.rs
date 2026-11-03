//! One bounded plain download and local playback in a private research profile.
use cumulus_core::{
    account::SessionStatus,
    client::{
        MusicClient,
        dto::{DownloadCommand, PlayerAction, PlayerResourceKind, PlayerView},
    },
    download::{
        offline::OfflineRecord,
        service::{DownloadOptions, DownloadStatus, DownloadTask, OutputContainer},
    },
    media::integrity::inspect_file_until,
    membership::Quality,
    playback::controller::PlaybackStatus,
    storage::StateStore,
};
use serde::Serialize;
use std::{
    fs,
    future::Future,
    path::Path,
    time::{Duration, Instant},
};
use tokio::time::{sleep, timeout};

const TASKS: &str = "download_tasks_v1";
const LEDGER: &str = "offline_music_v1";
const MAX_BYTES: u64 = 64 * 1024 * 1024;
const CONTROL: Duration = Duration::from_secs(5);
const SAMPLE: Duration = Duration::from_millis(80);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadFacts {
    pub published_bytes: u64,
    pub plain: bool,
    pub receipt_integrity_verified: bool,
    pub published_ledger_verified: bool,
    pub transport_cutoff_confirmed: bool,
    pub offline_native_progress: bool,
    pub stopped: bool,
}

type Evidence<T> = Result<T, &'static str>;

/// `cutoff` must close the host's admitted network transport before playback.
/// The evidence does not imply an OS network namespace or sound-card output.
pub async fn verify_download<F, Fut>(
    client: &MusicClient,
    epoch: &str,
    song_id: &str,
    root: &Path,
    store: &StateStore,
    owner: &str,
    cutoff: F,
) -> Evidence<DownloadFacts>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Evidence<()>>,
{
    let account = client.account().map_err(|_| "download_account")?;
    if account.epoch != epoch || account.status != SessionStatus::Authenticated {
        return Err("download_account");
    }
    if !fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_dir())
        || fs::read_dir(root)
            .map_err(|_| "download_root")?
            .next()
            .is_some()
    {
        return Err("download_root");
    }
    let root = root.canonicalize().map_err(|_| "download_root")?;
    let root_text = root.to_str().ok_or("download_root")?;
    let empty = store
        .transaction(|tx| {
            Ok(tx.values(owner, TASKS)?.is_empty() && tx.values(owner, LEDGER)?.is_empty())
        })
        .map_err(|_| "download_baseline")?;
    if !empty {
        return Err("download_baseline");
    }
    let configuration = bounded(
        CONTROL,
        client.download_command(
            epoch,
            DownloadCommand::Configure {
                root: root_text.into(),
                path_type: 0,
                file_type: 1,
            },
        ),
        "download_configure",
    )
    .await?;
    if configuration.epoch != epoch || !configuration.failed.is_empty() {
        return Err("download_configure");
    }
    let result = bounded(
        CONTROL,
        client.download_command(
            epoch,
            DownloadCommand::Start {
                song_ids: vec![song_id.into()],
                options: DownloadOptions {
                    quality: Quality::Standard,
                    tags: false,
                    cover: false,
                    lyrics: false,
                    container: OutputContainer::Official,
                },
            },
        ),
        "download_start",
    )
    .await?;
    let [id] = result.accepted.as_slice() else {
        return Err("download_start");
    };
    if result.epoch != epoch || !result.failed.is_empty() {
        return Err("download_start");
    }
    let task = timeout(Duration::from_secs(30), async {
        loop {
            let tasks = store.values(owner, TASKS).map_err(|_| "download_task")?;
            if tasks.len() != 1 {
                return Err("download_task");
            }
            let task: DownloadTask =
                serde_json::from_value(tasks.get(id).ok_or("download_task")?.clone())
                    .map_err(|_| "download_task")?;
            if task.id != *id || task.song_id != song_id {
                return Err("download_task");
            }
            match task.status {
                DownloadStatus::Complete if !task.cleanup_pending => return Ok(task),
                DownloadStatus::Error | DownloadStatus::Cancelled | DownloadStatus::Paused => {
                    return Err("download_worker");
                }
                _ => sleep(SAMPLE).await,
            }
        }
    })
    .await
    .map_err(|_| "download_publish_timeout")??;
    let published_bytes = audit(store, owner, &root, &task)?;
    timeout(Duration::from_secs(10), cutoff())
        .await
        .map_err(|_| "download_transport_cutoff")?
        .map_err(|_| "download_transport_cutoff")?;

    let playback = offline_progress(client, epoch, song_id, id).await;
    // Always stop the native player after an attempted local selection, even
    // when the clock, projection, or load did not meet the proof contract.
    let stopped = bounded(
        CONTROL,
        client.player_action(epoch, PlayerAction::Stop),
        "download_local_stop",
    )
    .await;
    playback?;
    if stopped?.state != PlaybackStatus::Idle {
        return Err("download_local_stop");
    }
    Ok(DownloadFacts {
        published_bytes,
        plain: true,
        receipt_integrity_verified: true,
        published_ledger_verified: true,
        transport_cutoff_confirmed: true,
        offline_native_progress: true,
        stopped: true,
    })
}

async fn bounded<T, E>(
    duration: Duration,
    op: impl Future<Output = Result<T, E>>,
    stage: &'static str,
) -> Evidence<T> {
    timeout(duration, op)
        .await
        .map_err(|_| stage)?
        .map_err(|_| stage)
}

fn audit(store: &StateStore, owner: &str, root: &Path, task: &DownloadTask) -> Evidence<u64> {
    let target = task.target.as_deref().ok_or("download_file")?;
    if target.parent() != Some(root)
        || task.encrypted
        || task.official_encryption
        || task.file_operation.is_some()
        || task.options.quality != Quality::Standard
        || task.options.container != OutputContainer::Official
        || task.options.tags
        || task.options.cover
        || task.options.lyrics
        || task.authorization_count != 1
    {
        return Err("download_plain_contract");
    }
    let raw = task.raw_integrity.as_ref().ok_or("download_receipt")?;
    let receipt = task.resource.as_ref().ok_or("download_receipt")?;
    let md5 = receipt["md5"].as_str().ok_or("download_receipt")?;
    if receipt["size"].as_u64() != Some(raw.bytes)
        || md5.len() != 32
        || !md5.bytes().all(|b| b.is_ascii_hexdigit())
        || !md5.eq_ignore_ascii_case(&raw.md5)
        || !receipt["freeTrialInfo"].is_null()
        || !(1..=MAX_BYTES).contains(&raw.bytes)
    {
        return Err("download_receipt");
    }
    let metadata = fs::symlink_metadata(target).map_err(|_| "download_file")?;
    if !metadata.file_type().is_file() || metadata.len() != raw.bytes {
        return Err("download_file");
    }
    let deadline = Instant::now() + CONTROL;
    let actual = inspect_file_until(target, || Instant::now() >= deadline)
        .map_err(|_| "download_file_integrity")?;
    if actual.bytes != raw.bytes
        || actual.sha256 != raw.sha256
        || task.output_integrity.as_ref() != Some(&actual)
    {
        return Err("download_file_integrity");
    }

    let records = store.values(owner, LEDGER).map_err(|_| "download_ledger")?;
    let (_, record) = records.first_key_value().ok_or("download_ledger")?;
    let record: OfflineRecord =
        serde_json::from_value(record.clone()).map_err(|_| "download_ledger")?;
    if records.len() != 1
        || record.task_id != task.id
        || record.song_id != task.song_id
        || record.path != target
        || record.quality != Quality::Standard
        || record.encrypted
        || record.integrity != actual
    {
        return Err("download_ledger");
    }
    Ok(actual.bytes)
}

async fn offline_progress(
    client: &MusicClient,
    epoch: &str,
    song_id: &str,
    id: &str,
) -> Evidence<()> {
    timeout(Duration::from_secs(10), async {
        let initial = client
            .play_downloads(epoch, vec![id.into()], 0)
            .await
            .map_err(|_| "download_local_select")?;
        let mut prior: Option<(String, f64)> = None;
        let mut view = initial;
        loop {
            if view.epoch != epoch || view.state == PlaybackStatus::Error {
                return Err("download_local_playback");
            }
            if let Some((play_id, position)) = playing(&view, song_id) {
                match &prior {
                    Some((old_id, old_position)) if old_id == play_id => {
                        if position >= old_position + 0.1 {
                            return Ok(());
                        }
                    }
                    Some(_) => return Err("download_local_selection_changed"),
                    None => prior = Some((play_id.into(), position)),
                }
            } else if prior.is_some() {
                return Err("download_local_selection_changed");
            }
            sleep(SAMPLE).await;
            view = client
                .player_snapshot(epoch)
                .await
                .map_err(|_| "download_local_snapshot")?;
        }
    })
    .await
    .map_err(|_| "download_local_timeout")?
}

fn playing<'a>(view: &'a PlayerView, song_id: &str) -> Option<(&'a str, f64)> {
    if view.state != PlaybackStatus::Playing
        || view.resource_kind != Some(PlayerResourceKind::Downloaded)
        || view.track.as_ref()?.id != song_id
    {
        return None;
    }
    let position = view.position.filter(|n| n.is_finite() && *n >= 0.0)?;
    Some((view.play_id.as_deref()?, position))
}
