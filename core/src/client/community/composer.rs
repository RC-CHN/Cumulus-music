//! Source-backed attachment search and durable local drafts.
use super::{AccountData, MusicClient, handle, project};
use crate::{Error, Result, client::mapping, podcast::Radio};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) const DRAFT_KIND: &str = "community-composer-draft-v1";
pub(super) const DRAFT_KEY: &str = "current";
const SEARCH_SIZE: usize = 8;
const SEARCH_LIMIT: usize = SEARCH_SIZE * 64;
const MAX_RECEIPTS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComposerKind {
    Song,
    Artist,
    Album,
    Playlist,
    Podcast,
}

impl ComposerKind {
    fn request(self, keyword: &str, offset: usize) -> (&'static str, Value) {
        match self {
            Self::Song => (
                "/api/search/song/list/page",
                json!({"keyword":keyword,"scene":"normal","limit":SEARCH_SIZE,
                    "offset":offset,"needCorrect":true}),
            ),
            Self::Podcast => (
                "/api/search/voicelist/get",
                json!({"keyword":keyword,"scene":"normal","limit":SEARCH_SIZE,"offset":offset}),
            ),
            Self::Artist | Self::Album | Self::Playlist => (
                match self {
                    Self::Artist => "/api/v1/search/artist/get",
                    Self::Album => "/api/v1/search/album/get",
                    _ => "/api/v1/search/playlist/get",
                },
                json!({"s":keyword,"limit":SEARCH_SIZE,"offset":offset,"queryCorrect":true}),
            ),
        }
    }

    fn rows(self, value: &Value) -> Result<&[Value]> {
        let rows = match self {
            Self::Song | Self::Podcast => value["data"].get("resources"),
            Self::Artist => value["result"].get("artists"),
            Self::Album => value["result"].get("albums"),
            Self::Playlist => value["result"].get("playlists"),
        };
        match rows {
            Some(Value::Array(rows)) if rows.len() <= SEARCH_SIZE => Ok(rows),
            Some(Value::Null) => Ok(&[]),
            _ => Err(Error::Response("invalid community attachment search rows")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Attachment {
    pub(super) kind: ComposerKind,
    pub(super) id: String,
    title: String,
    subtitle: Option<String>,
    cover_url: Option<String>,
}

#[derive(Serialize)]
pub struct ComposerSearchItem {
    receipt: String,
    #[serde(flatten)]
    attachment: Attachment,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerSearchPage {
    items: Vec<ComposerSearchItem>,
    next_offset: Option<usize>,
}

pub(super) struct AttachmentReceipt {
    key: Uuid,
    attachment: Attachment,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DraftAttachmentEdit {
    Keep,
    Remove,
    Select { receipt: String },
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ComposerDraftAction {
    Read,
    Save {
        revision: Option<String>,
        text: String,
        attachment: DraftAttachmentEdit,
    },
    Discard {
        revision: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposerDraft {
    pub(super) revision: String,
    pub(super) text: String,
    pub(super) attachment: Option<Attachment>,
}

fn sub(value: &Value) -> Option<String> {
    [
        &value["artist"]["transName"],
        &value["artist"]["name"],
        &value["creator"]["nickname"],
        &value["dj"]["nickname"],
    ]
    .into_iter()
    .find_map(|entry| project::text(entry, 120))
    .or_else(|| {
        let names = value["ar"]
            .as_array()
            .or_else(|| value["artists"].as_array())?;
        let credits = names
            .iter()
            .take(4)
            .filter_map(|entry| project::text(&entry["name"], 40))
            .collect::<Vec<_>>();
        (!credits.is_empty()).then(|| credits.join(" / "))
    })
}

fn project_attachment(kind: ComposerKind, raw: &Value) -> Option<Attachment> {
    let item = match kind {
        ComposerKind::Song => &raw["baseInfo"]["simpleSongData"],
        ComposerKind::Podcast => &raw["baseInfo"],
        _ => raw,
    };
    if !item.is_object() || item["grayDisplay"] == true || item["underShelf"] == true {
        return None;
    }
    if kind == ComposerKind::Song
        && [item, raw].into_iter().any(|item| {
            ["st", "status"].into_iter().any(|key| {
                item["privilege"][key]
                    .as_i64()
                    .is_some_and(|status| status < 0)
            })
        })
    {
        return None;
    }
    let (id, title) = if kind == ComposerKind::Podcast {
        let radio = Radio::from_official(item).ok()?;
        (radio.id, project::text(&json!(radio.name), 200)?)
    } else {
        (
            project::id(&item["id"])?,
            project::text(&item["name"], 200)?,
        )
    };
    if id == "0" {
        return None;
    }
    Some(Attachment {
        kind,
        id,
        title,
        subtitle: sub(item),
        cover_url: [
            &item["picUrl"],
            &item["coverImgUrl"],
            &item["al"]["picUrl"],
            &item["album"]["picUrl"],
            &item["img1v1Url"],
            &item["coverUrl"],
        ]
        .into_iter()
        .find_map(mapping::artwork),
    })
}

pub(super) fn validate_text(text: &str) -> Result<()> {
    if text.encode_utf16().count() > 1000
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r'))
    {
        return Err(Error::InvalidInput(
            "invalid or oversized community draft text",
        ));
    }
    Ok(())
}

impl MusicClient {
    pub async fn community_composer_search(
        &self,
        epoch: &str,
        kind: ComposerKind,
        keyword: &str,
        offset: usize,
    ) -> Result<AccountData<ComposerSearchPage>> {
        let token = self.playlist_token(epoch)?;
        let keyword = keyword.trim();
        if keyword.is_empty()
            || keyword.encode_utf16().count() > 200
            || keyword.chars().any(char::is_control)
            || offset >= SEARCH_LIMIT
            || !offset.is_multiple_of(SEARCH_SIZE)
        {
            return Err(Error::InvalidInput("invalid community attachment search"));
        }
        let (path, args) = kind.request(keyword, offset);
        let response = self
            .session
            .run_for(&token, |api, _| async move {
                api.call(path, args, false, &[200]).await
            })
            .await?;
        self.session.with_token(&token, |_| {
            let rows = kind.rows(&response)?;
            let mut state = self.community.state(&token)?;
            let mut items = Vec::with_capacity(rows.len());
            for attachment in rows.iter().filter_map(|row| project_attachment(kind, row)) {
                let receipt = Uuid::new_v4();
                state.attachments.push_back(AttachmentReceipt {
                    key: receipt,
                    attachment: attachment.clone(),
                });
                if state.attachments.len() > MAX_RECEIPTS {
                    state.attachments.pop_front();
                }
                items.push(ComposerSearchItem {
                    receipt: receipt.to_string(),
                    attachment,
                });
            }
            let next = offset + rows.len();
            Ok(AccountData {
                epoch: epoch.into(),
                value: ComposerSearchPage {
                    items,
                    next_offset: (rows.len() == SEARCH_SIZE && next < SEARCH_LIMIT).then_some(next),
                },
            })
        })
    }

    pub fn community_composer_draft(
        &self,
        epoch: &str,
        action: ComposerDraftAction,
    ) -> Result<AccountData<Option<ComposerDraft>>> {
        let token = self.playlist_token(epoch)?;
        let store = self.session.persistent_store()?;
        self.session.with_token(&token, |owner| {
            let value = if matches!(action, ComposerDraftAction::Read) {
                store
                    .get(owner, DRAFT_KIND, DRAFT_KEY)?
                    .map(serde_json::from_value)
                    .transpose()?
            } else {
                let selected = match &action {
                    ComposerDraftAction::Save {
                        attachment: DraftAttachmentEdit::Select { receipt },
                        ..
                    } => {
                        let key = handle(receipt)?;
                        let state = self.community.state(&token)?;
                        Some(
                            state
                                .attachments
                                .iter()
                                .find(|record| record.key == key)
                                .map(|record| record.attachment.clone())
                                .ok_or(Error::StaleResponse)?,
                        )
                    }
                    _ => None,
                };
                store.transaction(|tx| {
                    if super::publish::submission_unresolved(tx, owner)? {
                        return Err(Error::MutationBusy);
                    }
                    let old: Option<ComposerDraft> = tx
                        .get(owner, DRAFT_KIND, DRAFT_KEY)?
                        .map(serde_json::from_value)
                        .transpose()?;
                    let current = old.as_ref().map(|draft| draft.revision.as_str());
                    match action {
                        ComposerDraftAction::Read => unreachable!(),
                        ComposerDraftAction::Discard { revision } => {
                            if current != Some(revision.as_str()) {
                                return Err(Error::StaleResponse);
                            }
                            tx.delete(owner, DRAFT_KIND, DRAFT_KEY)?;
                            Ok(None)
                        }
                        ComposerDraftAction::Save {
                            revision,
                            text,
                            attachment,
                        } => {
                            if current != revision.as_deref() {
                                return Err(Error::StaleResponse);
                            }
                            validate_text(&text)?;
                            let attachment = match attachment {
                                DraftAttachmentEdit::Keep => old.and_then(|draft| draft.attachment),
                                DraftAttachmentEdit::Remove => None,
                                DraftAttachmentEdit::Select { .. } => selected,
                            };
                            if text.trim().is_empty() && attachment.is_none() {
                                return Err(Error::InvalidInput("community draft is empty"));
                            }
                            let draft = ComposerDraft {
                                revision: Uuid::new_v4().to_string(),
                                text,
                                attachment,
                            };
                            tx.put(owner, DRAFT_KIND, DRAFT_KEY, &serde_json::to_value(&draft)?)?;
                            Ok(Some(draft))
                        }
                    }
                })?
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
