//! The chapter link on official Recent Playback radio and audiobook rows.
use super::{
    dto::HistoryRadioChapterView,
    listened_books::{Chapter, local_chapter, unavailable},
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::MusicClient,
    history::{HistoryKind, HistoryRecord, HistoryService},
    playback::engine::Cancellation,
    podcast::{Radio, progress::latest},
    protocol::api::identifier,
    storage::StateTransaction,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

fn kind_slot(kind: HistoryKind) -> Option<usize> {
    match kind {
        HistoryKind::Podcast => Some(0),
        HistoryKind::Audiobook => Some(1),
        _ => None,
    }
}

pub(super) fn supported(kind: HistoryKind) -> bool {
    kind_slot(kind).is_some()
}

struct Published {
    token: SessionToken,
    revision: String,
    receipt: String,
    chapters: HashMap<String, Chapter>,
    source: Cancellation,
}

#[derive(Default)]
pub(in crate::client) struct RadioChaptersHost(Mutex<[Option<Published>; 2]>);

impl RadioChaptersHost {
    pub(super) fn publish(
        &self,
        token: &SessionToken,
        kind: HistoryKind,
        revision: &str,
        chapters: HashMap<String, Chapter>,
    ) -> Result<String> {
        let index = kind_slot(kind).ok_or(Error::InvalidInput("not a radio history kind"))?;
        let mut slots = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let slot = &mut slots[index];
        if let Some(state) = slot.as_ref().filter(|state| {
            &state.token == token && state.revision == revision && state.chapters == chapters
        }) {
            return Ok(state.receipt.clone());
        }
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        if let Some(old) = slot.replace(Published {
            token: token.clone(),
            revision: revision.to_owned(),
            receipt: receipt.clone(),
            chapters,
            source: Cancellation::default(),
        }) {
            old.source.cancel();
        }
        Ok(receipt)
    }

    fn selection(
        &self,
        token: &SessionToken,
        kind: HistoryKind,
        receipt: &str,
        id: &str,
    ) -> Result<(String, Chapter, Cancellation)> {
        let index = kind_slot(kind).ok_or(Error::InvalidInput("not a radio history kind"))?;
        let slots = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let state = slots[index]
            .as_ref()
            .filter(|state| &state.token == token && state.receipt == receipt)
            .ok_or(Error::QueueChanged)?;
        let chapter = state.chapters.get(id).ok_or(Error::QueueChanged)?;
        Ok((
            state.revision.clone(),
            chapter.clone(),
            state.source.clone(),
        ))
    }
}

fn label(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    (!text.trim().is_empty() && text.len() <= 2048 && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}

struct RadioChapter {
    value: Chapter,
    tag: Option<String>,
}

impl RadioChapter {
    fn from_record(
        tx: &StateTransaction<'_>,
        owner: &str,
        row: &HistoryRecord,
    ) -> Result<Option<Self>> {
        let raw = &row.resource;
        // M/21.j retains voiceName, voiceId and tag; unlike the separate
        // 听过的书 endpoint, the timeline format does not populate extPlayRecord.
        let id = identifier(&raw["voiceId"])
            .ok()
            .filter(|id| id.bytes().any(|digit| digit != b'0'));
        let mut value = Chapter {
            name: label(&raw["voiceName"]).unwrap_or_default(),
            playable: id.is_some() && !unavailable(raw),
            id: id.unwrap_or_default(),
            media_id: None,
            recent: false,
            location: 0,
            duration: 0,
            completed: false,
        };
        if !value.id.is_empty()
            && let Some(local) = latest::read(tx, owner, &row.id)?
        {
            local_chapter(&mut value, local);
        }
        if value.name.is_empty() {
            return Ok(None);
        }
        let tag = if value.recent {
            None
        } else {
            label(&raw["tag"])
        };
        Ok(Some(Self { value, tag }))
    }

    fn view(&self, online: bool, hide_progress: bool) -> HistoryRadioChapterView {
        let chapter = &self.value;
        let percent =
            if !hide_progress && !chapter.completed && chapter.location > 0 && chapter.duration > 0
            {
                Some(
                    (u128::from(chapter.location) * 100 / u128::from(chapter.duration)).clamp(1, 99)
                        as u8,
                )
            } else {
                None
            };
        HistoryRadioChapterView {
            name: chapter.name.clone(),
            tag: self.tag.clone(),
            recent: chapter.recent,
            percent,
            completed: !hide_progress && chapter.completed,
            playable: online && chapter.playable,
        }
    }
}

pub(super) fn view(
    tx: &StateTransaction<'_>,
    owner: &str,
    row: &HistoryRecord,
    online: bool,
    hidden: &HashSet<String>,
) -> Result<(Option<HistoryRadioChapterView>, Option<Chapter>)> {
    let Some(chapter) = RadioChapter::from_record(tx, owner, row)? else {
        return Ok((None, None));
    };
    let view = chapter.view(online, hidden.contains(&row.id));
    let selected = view.playable.then_some(chapter.value);
    Ok((Some(view), selected))
}

impl MusicClient {
    pub(in crate::client) async fn history_radio_chapter_queue(
        &self,
        token: &SessionToken,
        kind: HistoryKind,
        receipt: &str,
        id: &str,
    ) -> Result<(Vec<Value>, usize, Value, Cancellation)> {
        let (revision, chapter, source) = self
            .history_radio_chapters
            .selection(token, kind, receipt, id)?;
        let history = HistoryService::new(self.session.clone())?;
        let snapshot = history.snapshot_for(token, kind)?;
        if snapshot.revision != revision {
            return Err(Error::QueueChanged);
        }
        let row = snapshot
            .entries
            .iter()
            .find(|row| row.id == id)
            .ok_or(Error::QueueChanged)?;
        let radio = Radio::from_official(&row.resource)?;
        let (resources, index) = self
            .resolve_radio_chapter_queue(token, &radio, &chapter, &source)
            .await?;
        if source.is_cancelled() || history.snapshot_for(token, kind)?.revision != revision {
            return Err(Error::QueueChanged);
        }
        Ok((
            resources,
            index,
            json!({"kind":"history_radio_chapter","scene":"unknown","id":radio.id,"text":radio.name}),
            source,
        ))
    }
}
