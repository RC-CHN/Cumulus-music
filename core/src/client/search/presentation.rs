//! M3095/M2071 reactive display fields. Candidates never authorize media or writes.
use super::{
    SearchCategory, SearchItem, SearchPreviewLayout, SearchResultView, track_hint::TrackHint,
};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::search::results::reason::ReasonTag,
    client::{MusicClient, dto::AccountData},
    protocol::api::Api,
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, Weak},
};

const PAGE_LIMIT: usize = 16;
const PAGE_BYTES: usize = 2 * 1024 * 1024;
const TOTAL_BYTES: usize = 8 * 1024 * 1024;
const SECTION_SONGS: usize = 6;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPresentationItem {
    pub index: usize,
    pub track_id: String,
    pub hint: Option<TrackHint>,
    pub reasons: Vec<ReasonTag>,
}
#[derive(Clone, Serialize)]
pub struct SearchPresentationSection {
    pub id: String,
    pub items: Vec<SearchPresentationItem>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPresentationPatch {
    pub receipt: String,
    pub revision: String,
    pub preview_layout: Option<SearchPreviewLayout>,
    pub sections: Vec<SearchPresentationSection>,
}
struct Page {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: Option<String>,
    receipt: String,
    bytes: usize,
    sections: Vec<SearchPresentationSection>,
}
#[derive(Default)]
pub(super) struct SearchPresentationPages(Mutex<VecDeque<Page>>);

impl SearchPresentationPages {
    fn publish(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<&str>,
        view: &SearchResultView,
    ) -> Result<Option<String>> {
        let sections = candidates(view);
        let bytes = serde_json::to_vec(&sections)?.len();
        if bytes > PAGE_BYTES {
            return Ok(None);
        }
        let mut pages = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        if api.pc_presentation_credential_revision()?.as_deref() != credentials {
            return Err(Error::StaleResponse);
        }
        if pages
            .back()
            .is_some_and(|page| !owns(page, token, api, credentials))
        {
            pages.clear();
        }
        while pages.len() >= PAGE_LIMIT
            || pages.iter().map(|page| page.bytes).sum::<usize>() + bytes > TOTAL_BYTES
        {
            pages.pop_front();
        }
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        pages.push_back(Page {
            token: token.clone(),
            api: Arc::downgrade(api),
            credentials: credentials.map(str::to_owned),
            receipt: receipt.clone(),
            bytes,
            sections,
        });
        drop(pages);
        if api.pc_presentation_credential_revision()?.as_deref() != credentials {
            return Err(Error::StaleResponse);
        }
        Ok(Some(receipt))
    }
    fn resolve(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        receipt: &str,
    ) -> Result<Vec<SearchPresentationSection>> {
        if receipt.len() != 32 || !receipt.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::InvalidInput("invalid search presentation receipt"));
        }
        let credentials = api.pc_presentation_credential_revision()?;
        let mut pages = self.0.lock().map_err(|_| Error::LockPoisoned)?;
        let position = pages
            .iter()
            .position(|page| {
                page.receipt == receipt && owns(page, token, api, credentials.as_deref())
            })
            .ok_or(Error::StaleResponse)?;
        let page = pages.remove(position).ok_or(Error::StaleResponse)?;
        let rows = page.sections.clone();
        pages.push_back(page);
        drop(pages);
        if api.pc_presentation_credential_revision()? != credentials {
            return Err(Error::StaleResponse);
        }
        Ok(rows)
    }
}
fn owns(page: &Page, token: &SessionToken, api: &Arc<dyn Api>, credentials: Option<&str>) -> bool {
    page.token == *token
        && page.api.upgrade().is_some_and(|old| Arc::ptr_eq(&old, api))
        && page.credentials.as_deref() == credentials
}
fn candidates(view: &SearchResultView) -> Vec<SearchPresentationSection> {
    view.sections
        .iter()
        .map(|section| SearchPresentationSection {
            id: section.id.clone(),
            items: section
                .items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| match item {
                    SearchItem::Song {
                        track,
                        hint,
                        reasons,
                        ..
                    } => Some(SearchPresentationItem {
                        index,
                        track_id: track.id.clone(),
                        hint: hint.clone(),
                        reasons: reasons.clone(),
                    }),
                    _ => None,
                })
                .take(SECTION_SONGS)
                .collect(),
        })
        .filter(|section| !section.items.is_empty())
        .collect()
}
impl MusicClient {
    pub(super) fn capture_search_presentation(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: Option<&str>,
        view: &mut SearchResultView,
    ) -> Result<bool> {
        let Some(credentials) = credentials.filter(|_| view.query.category == SearchCategory::All)
        else {
            return Ok(false);
        };
        let still_bound = || {
            api.pc_presentation_credential_revision()
                .ok()
                .flatten()
                .as_deref()
                == Some(credentials)
        };
        // The response may itself have committed a new Cookie. A new live
        // cohort can render new searches, but must not inherit old candidates.
        if !still_bound() {
            return Ok(false);
        }
        // Hosts without the native presentation lifecycle retain their existing
        // one-read search policy; an arbitrary result cannot create a cohort.
        let snapshot = match self.pc_presentation_snapshot(token, api) {
            Ok(snapshot) => snapshot,
            Err(Error::LockPoisoned) => return Err(Error::LockPoisoned),
            Err(_) => return Ok(false),
        };
        // Capture only already-whitelisted display fields before applying c.
        // No raw response, media URL, privilege, or playback receipt is stored.
        let receipt = match self
            .search
            .presentation
            .publish(token, api, Some(credentials), view)
        {
            Ok(receipt) => receipt,
            Err(Error::StaleResponse | Error::CredentialChanged) => return Ok(false),
            Err(error) => return Err(error),
        };
        view.preview_layout = SearchPreviewLayout::from_experiments(
            snapshot.view.search_expanded,
            snapshot.view.search_dense,
        );
        if !snapshot.view.search_footer {
            for section in &mut view.sections {
                for item in &mut section.items {
                    if let SearchItem::Song { hint, reasons, .. } = item {
                        *hint = None;
                        reasons.clear();
                    }
                }
            }
        }
        match self.pc_presentation_is_current(token, api, &snapshot.view.revision) {
            Ok(true) if still_bound() => (),
            Ok(_) | Err(Error::StaleResponse | Error::CredentialChanged) => return Ok(false),
            Err(error) => return Err(error),
        }
        view.presentation_receipt = receipt;
        view.presentation_revision = Some(snapshot.view.revision);
        Ok(true)
    }

    /// M3095 changes display selectors in place. This performs no search,
    /// account refresh, credential mutation, media request, or player action.
    pub fn search_presentation(
        &self,
        epoch: &str,
        receipt: &str,
        revision: &str,
    ) -> Result<AccountData<SearchPresentationPatch>> {
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        let snapshot = self.pc_presentation_snapshot(&token, &api)?;
        if snapshot.view.revision != revision
            || !self.pc_presentation_is_current(&token, &api, revision)?
        {
            return Err(Error::StaleResponse);
        }
        let mut sections = self.search.presentation.resolve(&token, &api, receipt)?;
        if !snapshot.view.search_footer {
            for section in &mut sections {
                for item in &mut section.items {
                    item.hint = None;
                    item.reasons.clear();
                }
            }
        }
        let value = SearchPresentationPatch {
            receipt: receipt.into(),
            revision: revision.into(),
            preview_layout: SearchPreviewLayout::from_experiments(
                snapshot.view.search_expanded,
                snapshot.view.search_dense,
            ),
            sections,
        };
        if !self.pc_presentation_is_current(&token, &api, revision)? {
            return Err(Error::StaleResponse);
        }
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::mapping, protocol::api::ApiFuture};
    use serde_json::{Value, json};
    struct Offline;
    impl Api for Offline {
        fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async { panic!("presentation candidates cannot access the network") })
        }
    }
    fn token(generation: u64) -> SessionToken {
        SessionToken {
            generation,
            uid: Some("71".into()),
        }
    }
    fn page() -> SearchResultView {
        SearchResultView {
            query: super::super::SearchResultQuery {
                keyword: "合成".into(),
                category: SearchCategory::All,
                offset: 0,
                limit: 30,
                correct: true,
                channel: None,
            },
            sections: vec![super::super::SearchSection {
                id: "search_block_song:0".into(),
                title: "歌曲".into(),
                category: Some(SearchCategory::Songs),
                items: (1..=10)
                    .map(|id| SearchItem::Song {
                        track: mapping::track(&json!({"id":id,"name":"合成曲目","ar":[],"al":{}}))
                            .unwrap(),
                        queue_drop_receipt: None,
                        group_next_right: None,
                        group_download: None,
                        song_comment_menu: None,
                        lyric_group_playback: None,
                        song_share_receipt: None,
                        song_public_link_available: false,
                        mv_id: Some("9".into()),
                        replacement_mv: false,
                        vip_badge: None,
                        limited_free: false,
                        quality_badge: None,
                        hint: Some(TrackHint::Alias {
                            text: format!("合成别名{id}"),
                        }),
                        table_hint: None,
                        explicit: true,
                        original_recording: true,
                        reasons: vec![],
                        excerpt: vec![],
                        excerpt_truncated: false,
                    })
                    .collect(),
            }],
            privilege_receipt: Some("privilege-must-not-copy".into()),
            presentation_receipt: None,
            presentation_revision: None,
            total: None,
            more: false,
            next_offset: 0,
            correction: None,
            correction_parts: None,
            correction_receipt: None,
            unavailable_count: 0,
            notice: None,
            empty_title: None,
            tabs: None,
            hot_explanation: None,
            song_comment_rows: vec![],
            lyric_group_rows: vec![],
            song_share_rows: vec![],
            preview_layout: None,
        }
    }
    #[test]
    fn candidates_are_bounded_display_fields_without_media_or_privilege_state() {
        let pages = SearchPresentationPages::default();
        let api: Arc<dyn Api> = Arc::new(Offline);
        let owner = token(1);
        let original = page();
        let receipt = pages
            .publish(&owner, &api, None, &original)
            .unwrap()
            .unwrap();
        let rows = pages.resolve(&owner, &api, &receipt).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].items.len(), SECTION_SONGS);
        assert_eq!(rows[0].items[5].track_id, "6");
        assert_eq!(rows[0].items[5].index, 5);
        let value = serde_json::to_string(&rows).unwrap();
        assert!(value.contains("合成别名6"));
        assert!(!value.contains("合成别名7"));
        for forbidden in [
            "privilege",
            "mvId",
            "coverUrl",
            "explicit",
            "originalRecording",
            "queueDropReceipt",
        ] {
            assert!(!value.contains(forbidden));
        }
        assert!(matches!(
            &original.sections[0].items[0],
            SearchItem::Song {
                hint: Some(_),
                explicit: true,
                ..
            }
        ));
    }
    #[test]
    fn page_receipts_are_lru_bounded_and_account_and_api_scoped() {
        let pages = SearchPresentationPages::default();
        let api: Arc<dyn Api> = Arc::new(Offline);
        let other: Arc<dyn Api> = Arc::new(Offline);
        let owner = token(1);
        let mut receipts = Vec::new();
        for _ in 0..PAGE_LIMIT {
            receipts.push(pages.publish(&owner, &api, None, &page()).unwrap().unwrap());
        }
        pages.resolve(&owner, &api, &receipts[0]).unwrap();
        pages.publish(&owner, &api, None, &page()).unwrap();
        assert!(pages.resolve(&owner, &api, &receipts[1]).is_err());
        assert!(pages.resolve(&owner, &api, &receipts[0]).is_ok());
        assert!(pages.resolve(&owner, &other, &receipts[0]).is_err());
        assert!(pages.resolve(&token(2), &api, &receipts[0]).is_err());
        assert!(pages.resolve(&owner, &api, "invalid").is_err());
        pages.publish(&token(2), &other, None, &page()).unwrap();
        assert!(pages.resolve(&owner, &api, &receipts[0]).is_err());
        assert!(pages.0.lock().unwrap().len() <= PAGE_LIMIT);
    }
}
