use super::{LocalRootView, LocalTrackView};
use crate::{
    client::dto::{ArtistReference, TrackView},
    library::{AudioMetadata, LibraryItem, LocalRevision, monitor::WatchRoot},
};
use sha2::{Digest, Sha256};
pub(super) fn revision(revision: &LocalRevision) -> String {
    hex::encode(Sha256::digest(
        serde_json::to_vec(revision).expect("local revisions serialize"),
    ))
}
pub(crate) fn track(id: String, metadata: &AudioMetadata, missing: bool) -> TrackView {
    TrackView {
        id,
        voice_id: None,
        name: metadata.title.clone(),
        artists: metadata.artists.clone(),
        artist_refs: metadata
            .artists
            .iter()
            .map(|name| ArtistReference {
                id: None,
                name: name.clone(),
            })
            .collect(),
        album: metadata.album.clone(),
        album_id: None,
        cover_url: None,
        duration_ms: (metadata.duration * 1000.0).max(0.0) as u64,
        fee: 0,
        missing,
        local: true,
    }
}
pub(super) fn row(item: &LibraryItem) -> LocalTrackView {
    LocalTrackView {
        track: track(item.id.clone(), &item.metadata, !item.available()),
        revision: revision(&item.revision),
        file_name: item
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        folder: item
            .path
            .parent()
            .unwrap_or(&item.path)
            .to_string_lossy()
            .into_owned(),
        format: item.metadata.file_format().into(),
        cue: matches!(item.revision, LocalRevision::Cue { .. }),
        encrypted: item.metadata.container == "ncm",
        unreadable: item.unreadable,
    }
}
pub(super) fn root(root: &WatchRoot) -> LocalRootView {
    let report = root.result.as_ref();
    LocalRootView {
        id: root.id.clone(),
        path: root.path.to_string_lossy().into_owned(),
        enabled: root.enabled,
        status: root.status,
        progress: root.progress.clone(),
        last_scan_ms: root.last_scan_ms,
        updated: report.map_or(0, |r| r.updated + r.cue_updated),
        unchanged: report.map_or(0, |r| r.unchanged + r.cue_unchanged),
        missing: report.map_or(0, |r| r.missing + r.cue_missing),
        failures: report.map_or(0, |r| r.errors.len()),
        root_missing: report.is_some_and(|r| r.root_missing),
        error: root
            .error
            .as_ref()
            .map(|_| "目录扫描失败，请检查路径、访问权限和媒体探测组件后重试"),
    }
}
