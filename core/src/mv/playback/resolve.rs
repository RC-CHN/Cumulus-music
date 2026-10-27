use super::*;
use crate::{
    download::{
        offline::OfflineLibrary,
        service::{DownloadService, DownloadStatus},
    },
    mv::{MvApi, MvPurpose, MvService},
    playback::engine::{LoadRequest, MediaInput},
    protocol::api::identifier,
};
use serde_json::json;
pub(super) struct Load {
    pub id: String,
    pub resolution: u32,
    pub task: Option<String>,
    pub position: Option<f64>,
    pub remote: bool,
}
pub(super) struct Prepared {
    pub engine: Engine,
    pub detail: crate::mv::MvDetail,
    pub actual_resolution: u32,
    pub local: bool,
    pub duration: Option<f64>,
    pub entry: i64,
    pub position: Option<f64>,
}
pub(super) async fn prepare(
    session: Arc<AccountSession>,
    token: SessionToken,
    factory: Factory,
    load: Load,
    cancel: Cancellation,
    loading: impl FnOnce() + Send,
    selected: impl FnOnce(local::LocalSource) + Send,
) -> Result<Prepared> {
    let mv = MvService::new(session.clone())?;
    let detail_result = mv.detail_for(&token, &json!(load.id)).await;
    cancel.check()?;
    let downloads = DownloadService::new(session.clone())?;
    let mut local = downloads
        .list()?
        .into_iter()
        .filter(|t| {
            t.origin.is_mv()
                && t.song_id == load.id
                && t.status == DownloadStatus::Complete
                && t.file_operation.is_none()
        })
        .filter(|t| load.task.as_ref().is_none_or(|id| id == &t.id))
        .collect::<Vec<_>>();
    local.sort_by_key(|t| {
        std::cmp::Reverse(
            t.resource
                .as_ref()
                .and_then(|v| v["r"].as_u64())
                .unwrap_or(0),
        )
    });
    if load.remote {
        local.retain(|t| {
            t.resource.as_ref().and_then(|v| v["r"].as_u64()) == Some(u64::from(load.resolution))
        });
    }
    if load.task.is_some() && local.is_empty() {
        return Err(Error::State("completed MV download is unavailable"));
    }
    let mut source = None;
    let mut lease = None;
    let mut provenance = None;
    let mut actual = load.resolution;
    for task in local {
        let Some(path) = task.target.clone() else {
            continue;
        };
        let held = crate::download::playback::acquire(&session, &token, &task)?;
        let check = OfflineLibrary::new(session.clone())?
            .check(&token, &path, true)
            .await;
        if check.is_ok_and(|v| v.access.can_play && v.integrity_verified) {
            actual = task
                .resource
                .as_ref()
                .and_then(|v| v["r"].as_u64())
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0 && *n <= 8640)
                .ok_or(Error::State("completed MV resolution missing"))?;
            provenance = Some(local::LocalSource {
                task: task.id,
                path: path.clone(),
            });
            lease = Some(held);
            source = Some(MediaInput::File(path));
            break;
        }
        if load.task.is_some() {
            return Err(Error::Media("completed MV failed offline integrity check"));
        }
    }
    let is_local = source.is_some();
    let detail = match detail_result {
        Ok(value) => value,
        Err(error) if is_local => mv.cached_for(&token, &json!(load.id))?.ok_or(error)?,
        Err(error) => return Err(error),
    };
    if identifier(&json!(detail.id))? != load.id {
        return Err(Error::Response("MV playback identity mismatch"));
    }
    let source = match source {
        Some(source) => source,
        None => {
            let value = session
                .run_for(&token, |api, _| async {
                    MvApi::new(api)
                        .authorize(&json!(load.id), load.resolution, MvPurpose::Play)
                        .await
                })
                .await?;
            actual = value["r"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(Error::Response("MV actual resolution missing"))?;
            MediaInput::Url(crate::download::transfer::MediaUrl::parse(
                value["url"]
                    .as_str()
                    .ok_or(Error::Response("MV URL missing"))?,
            )?)
        }
    };
    cancel.check()?;
    let position = load
        .position
        .or(mv
            .position_for(&token, &json!(load.id))?
            .map(|v| v.position_ms as f64 / 1000.0))
        .unwrap_or(0.0);
    let position = if detail.duration_ms > 0 && position >= detail.duration_ms as f64 / 1000.0 {
        0.0
    } else {
        position
    };
    loading();
    if let Some(provenance) = provenance {
        selected(provenance);
    }
    tokio::task::spawn_blocking(move || {
        cancel.check()?;
        let mut engine = factory()?;
        let mut request = LoadRequest::new(source);
        request.start = position;
        let deadline = std::time::Instant::now() + request.timeout;
        let loaded = engine.load(request, &cancel)?;
        // FILE_LOADED may precede video decoding. Stay in Loading until the
        // render worker has a real frame, rather than publishing its blank
        // initialization surface as successful playback.
        while engine.frame_after(0)?.is_none() {
            cancel.check()?;
            if std::time::Instant::now() >= deadline {
                return Err(Error::Media("video produced no decoded frame"));
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        cancel.check()?;
        let position = engine.poll()?.position;
        Ok(Prepared {
            engine: Arc::new(Mutex::new(HeldEngine {
                device: engine,
                _lease: lease,
            })),
            detail,
            actual_resolution: actual,
            local: is_local,
            duration: loaded.duration,
            entry: loaded.entry_id,
            position,
        })
    })
    .await
    .map_err(|_| Error::State("video decoder worker failed"))?
}
