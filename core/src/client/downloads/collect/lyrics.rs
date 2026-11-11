//! The shared guarded picker carries a separate, revalidated Lyrics source.
use super::{AccountData, MusicClient, Selection, Source};
use crate::{Error, Result};
use serde::Serialize;
use std::{sync::Arc, time::Instant};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricGroupCollectPlan {
    pub receipt: Option<String>,
    pub can_create: bool,
    pub selected_count: usize,
    pub song_ids: Vec<String>,
}

impl MusicClient {
    pub fn lyric_group_collect_prepare(
        &self,
        epoch: &str,
        anchor: &str,
        rows: &[String],
    ) -> Result<AccountData<LyricGroupCollectPlan>> {
        let login = self.auth.pending_guard();
        login()?;
        let token = self.player_token(epoch)?;
        let api = self.session.native_api(&token)?;
        self.session
            .with_verified_profile(&token, &api, |_, _| Ok(()))?;
        let credentials = api
            .lyric_group_collection_credential_revision()?
            .ok_or(Error::State("guarded lyric collection is unavailable"))?;
        let authority = self.search.lyric_collection.clone();
        let selection = authority.resolve(&token, &api, &credentials, anchor, rows)?;
        let mut value = LyricGroupCollectPlan {
            receipt: None,
            can_create: false,
            selected_count: rows.len(),
            song_ids: selection.songs.clone(),
        };
        login()?;
        if !authority.current(&selection) {
            return Err(Error::StaleResponse);
        }
        self.session.require_api(&token, &api)?;
        if !value.song_ids.is_empty() {
            let ticket = self.begin_download_collection()?;
            let receipt = uuid::Uuid::new_v4().simple().to_string();
            value.can_create = api
                .download_collection_create_scope_revision()
                .ok()
                .flatten()
                .as_deref()
                == Some(&credentials);
            login()?;
            let source = Arc::new(Source {
                session: self.session.clone(),
                token,
                api,
                credentials,
                receipt: receipt.clone(),
                selection: Selection::Lyrics {
                    selection: Box::new(selection),
                    authority,
                    login: Box::new(login),
                },
                created: Instant::now(),
            });
            self.install_download_collection(source, ticket)?;
            value.receipt = Some(receipt);
        }
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}
