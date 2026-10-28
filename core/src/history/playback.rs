//! Native-only projection of a successfully started queue entry (M/308).
//! No network request or raw playback URL is needed for local history.
use super::{HistoryKind as Kind, HistoryRecord, service};
use crate::{Result, playback::QueueEntry, storage::StateTransaction};
use serde_json::{Value, json};

#[derive(Clone, Default)]
pub(crate) struct Checkpoint {
    pub played_at: Option<u64>,
    pub committed: bool,
}

fn song(resource: &Value) -> Value {
    if resource["local_id"].is_string() {
        return json!({"local_id":resource["local_id"],"local_revision":resource["local_revision"],
            "metadata":resource["metadata"]});
    }
    if resource["song"].is_object() {
        let mut metadata = resource["song"].clone();
        if resource["download_task_id"].is_string() {
            metadata["download_task_id"] = resource["download_task_id"].clone();
        }
        return metadata;
    }
    json!({"id":resource["id"],"name":resource["name"],"dt":resource["dt"]})
}

/// Official M/144.c uses strict category names/numeric IDs, not fee status.
fn audiobook(radio: &Value) -> bool {
    matches!(
        radio["category"].as_str(),
        Some("有声书" | "广播剧" | "文学出版")
    ) || matches!(
        radio["categoryId"].as_u64(),
        Some(10001 | 3088097 | 3148096)
    )
}

fn records(item: &QueueEntry, at: u64) -> Result<Vec<(Kind, HistoryRecord)>> {
    let resource = &item.resource;
    let voice = resource
        .get("voice")
        .filter(|v| v.is_object())
        .or_else(|| resource["song"].get("voice"));
    if let Some(voice) = voice.filter(|v| v.is_object()) {
        let radio = voice
            .get("audio")
            .filter(|v| v.is_object())
            .or_else(|| voice.get("radio"));
        let mut rows = Vec::new();
        if let Some(radio) = radio.filter(|r| r.is_object()) {
            let mut radio = radio.clone();
            for field in ["creator", "dj"] {
                if !voice[field].is_null() {
                    radio[field] = voice[field].clone();
                }
            }
            let kind = if audiobook(&radio) {
                Kind::Audiobook
            } else {
                Kind::Podcast
            };
            rows.push((kind, HistoryRecord::local(kind, radio, at)?));
            if kind == Kind::Audiobook {
                return Ok(rows);
            }
        }
        let mut record = HistoryRecord::local(Kind::Voice, voice.clone(), at)?;
        record.play_progress = Some(0.0);
        record.listened = Some(false);
        rows.push((Kind::Voice, record));
        return Ok(rows);
    }
    // Missing voice metadata must not turn its media-song ID into song history.
    if !resource["voice_id"].is_null() || !resource["song"]["voice_id"].is_null() {
        return Err(crate::Error::State("voice history metadata missing"));
    }
    let track = song(resource);
    let mut rows = vec![(
        Kind::Song,
        HistoryRecord::local(Kind::Song, track.clone(), at)?,
    )];
    match item.source["kind"].as_str() {
        Some("album") => {
            let mut album = track
                .get("al")
                .or_else(|| track.get("album"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            if album["id"].is_null() {
                album["id"] = item.source["id"].clone();
            }
            album["artists"] = track
                .get("ar")
                .or_else(|| track.get("artists"))
                .cloned()
                .unwrap_or_else(|| json!([]));
            rows.push((Kind::Album, HistoryRecord::local(Kind::Album, album, at)?));
        }
        Some("playlist") => {
            let playlist = item
                .source
                .get("history_resource")
                .cloned()
                .unwrap_or_else(|| json!({"id":item.source["id"]}));
            rows.push((
                Kind::Playlist,
                HistoryRecord::local(Kind::Playlist, playlist, at)?,
            ));
        }
        _ => {}
    }
    Ok(rows)
}

pub(crate) fn save(
    tx: &mut StateTransaction<'_>,
    owner: &str,
    item: &QueueEntry,
    checkpoint: &Checkpoint,
    seconds: f64,
    ended: bool,
    full: bool,
) -> Result<()> {
    let Some(at) = checkpoint.played_at else {
        return Ok(());
    };
    if !checkpoint.committed {
        for (kind, mut record) in records(item, at)? {
            // An earlier failed DB write must not re-add something the user has
            // since removed. A deletion's local effect also applies on rejection.
            if super::mutation::removed_since(tx, owner, kind, &record.id, at)? {
                continue;
            }
            if kind == Kind::Voice {
                let previous = service::local(tx, owner, kind)?;
                if let Some(previous) = previous.iter().find(|r| r.id == record.id) {
                    record.play_progress = previous.play_progress;
                    record.listened = previous.listened;
                }
            }
            service::upsert(tx, owner, kind, record)?;
        }
    }
    // Seconds are a local full-program clock. A preview is heard but does not
    // overwrite a full program's bookmark or mark the whole program finished.
    if full {
        let voice = item
            .resource
            .get("voice_id")
            .filter(|v| !v.is_null())
            .or_else(|| item.resource["song"].get("voice_id"));
        if let Some(voice) = voice.filter(|v| !v.is_null()) {
            let id = crate::protocol::api::identifier(voice)?;
            let mut rows = service::local(tx, owner, Kind::Voice)?;
            if let Some(row) = rows.iter_mut().find(|r| r.id == id && r.played_at == at) {
                row.play_progress = Some(seconds);
                row.listened = Some(ended);
                tx.put(
                    owner,
                    service::NAMESPACE,
                    &service::local_key(Kind::Voice),
                    &serde_json::to_value(rows)?,
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StateStore;

    #[test]
    fn audiobook_classification_matches_executed_official_m144_vectors() {
        let vectors: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/fixtures/history-audiobook.json"))
                .unwrap();
        for vector in vectors {
            assert_eq!(
                audiobook(&vector["input"]),
                vector["expected"].as_bool().unwrap(),
                "{vector}"
            );
        }
    }

    #[test]
    fn downloaded_voice_keeps_program_identity_and_full_bookmark_across_preview_and_deletion() {
        let root = tempfile::tempdir().unwrap();
        let store = StateStore::open(root.path()).unwrap();
        let voice = json!({"id":81,"mainTrackId":91,"name":"Episode","duration":120000,
            "audio":{"id":71,"name":"Radio"}});
        // Download queues expose the main song at the root and program ID under song.
        let item = QueueEntry {
            entry_id: "entry".into(),
            source: json!({"kind":"downloads"}),
            resource: json!({"id":91,"voice":voice,"song":{"id":91,"voice_id":81},
                "path":"/private/fixture.mp3","url":"https://private.invalid/?secret=hidden"}),
        };
        let checkpoint = Checkpoint {
            played_at: Some(1),
            committed: false,
        };
        store
            .transaction(|tx| save(tx, "9", &item, &checkpoint, 42.0, false, true))
            .unwrap();
        let checkpoint = Checkpoint {
            played_at: Some(2),
            committed: false,
        };
        store
            .transaction(|tx| save(tx, "9", &item, &checkpoint, 60.0, true, false))
            .unwrap();
        store
            .transaction(|tx| {
                let rows = service::local(tx, "9", Kind::Voice)?;
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].id, "81");
                assert_eq!(rows[0].play_progress, Some(42.0));
                assert_eq!(rows[0].listened, Some(false));
                assert_eq!(rows[0].played_at, 2);
                assert_eq!(service::local(tx, "9", Kind::Podcast)?[0].id, "71");
                assert!(service::local(tx, "9", Kind::Song)?.is_empty());
                assert!(!serde_json::to_string(&rows)?.contains("private"));
                tx.put(
                    "9",
                    service::NAMESPACE,
                    &service::local_key(Kind::Voice),
                    &json!([]),
                )
            })
            .unwrap();
        let checkpoint = Checkpoint {
            committed: true,
            ..checkpoint
        };
        store
            .transaction(|tx| {
                save(tx, "9", &item, &checkpoint, 120.0, true, true)?;
                assert!(service::local(tx, "9", Kind::Voice)?.is_empty());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn audiobook_playback_records_the_book_without_mixing_it_into_podcasts_or_songs() {
        let item = QueueEntry {
            entry_id: "entry".into(),
            source: json!({"kind":"podcast"}),
            resource: json!({"id":91,"voice_id":81,"voice":{"id":81,"mainTrackId":91,
                "audio":{"id":71,"categoryId":10001,"name":"Book"}}}),
        };
        let recorded = records(&item, 123).unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, Kind::Audiobook);
        assert_eq!(recorded[0].1.id, "71");
    }
}
