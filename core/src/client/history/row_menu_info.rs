//! Read-only M556/M622 menu adornments bound to the current native history.
use super::{
    AccountData, HistoryRowComment, HistoryRowCommentResource, HistoryRowMenuInfo,
    HistoryRowMenuTarget, HistorySongDownloadMenu, MusicClient, row_menu, song_download,
    voice_file_menu,
};
use crate::{
    Error, Result,
    comments::{CommentApi, ResourceKind},
    history::{HistoryKind, HistoryRecord, HistoryService, HistorySnapshot},
    podcast::Episode,
    protocol::{api::identifier, wire::serialization::js_truthy},
};
use std::time::Duration;

const COUNT_TIMEOUT: Duration = Duration::from_millis(1_500);

struct MenuProjection {
    info: HistoryRowMenuInfo,
    count_target: Option<(ResourceKind, String)>,
    clicked_program: Option<Episode>,
    clicked_song: Option<String>,
}

fn public_id(kind: HistoryKind, row: &HistoryRecord) -> Option<String> {
    use song_download::HistorySongDownloadSource::{Catalog, DownloadTask};
    let raw = &row.resource;
    match kind {
        HistoryKind::Song => {
            // M556 excludes the uploaded song type from comments. A private
            // cloud/file identity must not be made into a public song link.
            if !matches!(song_download::source(row), Catalog | DownloadTask(_))
                || js_truthy(&raw["fileId"])
                || raw["songType"].as_i64().or(raw["t"].as_i64()) == Some(1)
            {
                return None;
            }
        }
        HistoryKind::Voice => {
            // This is the normal history program ID, not a main-song ID
            // or M622's distinct downloadedVoice/offline endpoint variant.
            if ["offlineId", "local_id", "fileId", "cloud_song_id"]
                .iter()
                .any(|field| js_truthy(&raw[*field]))
            {
                return None;
            }
        }
        _ => return None,
    }
    let id = identifier(&raw["id"]).ok()?;
    (id == row.id && id.len() <= 128).then_some(id)
}

fn project(snapshot: &HistorySnapshot, target: &HistoryRowMenuTarget) -> Result<MenuProjection> {
    if snapshot.kind != target.history_kind || snapshot.revision != target.revision {
        return Err(Error::QueueChanged);
    }
    let selection = row_menu::resolve(snapshot, &target.clicked_id, &target.selected_ids)?;
    let clicked = selection.clicked;
    let has_group = !selection.selected.is_empty();
    let (song_download, clicked_song) = if snapshot.kind == HistoryKind::Song {
        use song_download::{HistorySongDownloadProjection, HistorySongDownloadSource};
        // Comment and Copy still operate on larger selections; the download
        // branch alone has the bounded song-job limit.
        match HistorySongDownloadProjection::project_resolved_row(selection, snapshot) {
            Ok(rows)
                if matches!(
                    rows.row_anchor_source(),
                    HistorySongDownloadSource::Catalog | HistorySongDownloadSource::DownloadTask(_)
                ) =>
            {
                let menu = if rows.row_copyright_blocked() {
                    HistorySongDownloadMenu::CopyrightBlocked
                } else {
                    HistorySongDownloadMenu::Download
                };
                (Some(menu), Some(rows.anchor.id.clone()))
            }
            Ok(_) | Err(Error::InvalidInput(_)) => {
                (Some(HistorySongDownloadMenu::Unavailable), None)
            }
            Err(error) => return Err(error),
        }
    } else {
        (None, None)
    };
    let mut projection = MenuProjection {
        info: HistoryRowMenuInfo {
            comment: None,
            copy_link: None,
            song_download,
            voice_file: None,
        },
        count_target: None,
        clicked_program: (snapshot.kind == HistoryKind::Voice)
            .then(|| voice_file_menu::episode(clicked).ok())
            .flatten(),
        clicked_song,
    };
    let Some(id) = public_id(snapshot.kind, clicked) else {
        return Ok(projection);
    };
    let (kind, path) = match snapshot.kind {
        HistoryKind::Song => (ResourceKind::Track, "song"),
        HistoryKind::Voice => (ResourceKind::Voice, "dj"),
        _ => return Err(Error::InvalidInput("unsupported history row menu kind")),
    };
    // M3085 evaluates the async M556 count check before the group check
    // hides both entries. M622 tests the row's own from.selectedData;
    // history places its group in parentData, so a Voice shows clicked actions.
    projection.count_target = Some((kind, id.clone()));
    if kind == ResourceKind::Track && has_group {
        return Ok(projection);
    }
    if kind == ResourceKind::Voice || row_menu::song_copyright_blocked(clicked) != Some(true) {
        // M198 supplies this path. Do not put the account's userid/uct2
        // attribution in the renderer or in the copied public link.
        projection.info.copy_link = Some(format!("https://music.163.com/{path}?id={id}"));
    }
    projection.info.comment = Some(HistoryRowComment {
        resource: HistoryRowCommentResource { kind, id },
        count: None,
    });
    Ok(projection)
}

impl MusicClient {
    pub async fn history_row_menu_info(
        &self,
        epoch: &str,
        target: HistoryRowMenuTarget,
    ) -> Result<AccountData<HistoryRowMenuInfo>> {
        if !matches!(target.history_kind, HistoryKind::Song | HistoryKind::Voice) {
            return Err(Error::InvalidInput("unsupported history row menu kind"));
        }
        let token = self.player_token(epoch)?;
        let history = HistoryService::new(self.session.clone())?;
        let snapshot = history.snapshot_for(&token, target.history_kind)?;
        let mut menu = project(&snapshot, &target)?;
        let sequence = self.history_voice_menu.begin()?;
        if let Some((kind, id)) = menu.count_target
            && token.uid.is_some()
        {
            let api = self.session.native_api(&token)?;
            let count =
                tokio::time::timeout(COUNT_TIMEOUT, CommentApi::new(api.clone()).count(kind, id))
                    .await
                    .ok()
                    .and_then(Result::ok);
            self.session.require_api(&token, &api)?;
            if let Some(comment) = &mut menu.info.comment {
                comment.count = count;
            }
        }
        if let Some(song) = menu.clicked_song
            && token.uid.is_some()
        {
            let local = self.local.context().await?;
            let owner = token.clone();
            let session = self.session.clone();
            let unavailable = tokio::task::spawn_blocking(move || {
                song_download::row_hidden(&local.library, &session, &owner, &song)
            })
            .await
            .map_err(|_| Error::State("history song menu inspection stopped"))??;
            if unavailable {
                menu.info.song_download = Some(HistorySongDownloadMenu::Unavailable);
            }
        }
        let observation = if target.history_kind == HistoryKind::Voice {
            Some(
                self.observe_history_voice_file(&token, menu.clicked_program)
                    .await?,
            )
        } else {
            None
        };
        // Scope failures are not optional adornment failures. An expired
        // account epoch or a changed displayed history must not publish.
        let revision = target.revision.clone();
        history.with_matching_revision(&token, target.history_kind, &revision, |_| {
            menu.info.voice_file =
                self.history_voice_menu
                    .publish(sequence, token.clone(), target, observation)?;
            Ok(AccountData {
                epoch: epoch.into(),
                value: menu.info,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn row(kind: HistoryKind, raw: Value) -> HistoryRecord {
        HistoryRecord::local(kind, raw, 10).unwrap()
    }

    fn snapshot(kind: HistoryKind, entries: Vec<HistoryRecord>) -> HistorySnapshot {
        HistorySnapshot {
            kind,
            revision: "revision".into(),
            sync_enabled: false,
            local: vec![],
            remote: None,
            entries,
        }
    }

    fn target(kind: HistoryKind, clicked: &str, selected: &[&str]) -> HistoryRowMenuTarget {
        HistoryRowMenuTarget {
            history_kind: kind,
            revision: "revision".into(),
            clicked_id: clicked.into(),
            selected_ids: selected.iter().map(|id| (*id).into()).collect(),
        }
    }

    #[test]
    fn row_menu_song_copyright_and_group_visibility_are_independent_of_count_reads() {
        let kind = HistoryKind::Song;
        let snapshot = snapshot(
            kind,
            vec![
                row(kind, json!({"id":"8","privilege":{"st":-100}})),
                row(kind, json!({"id":"7"})),
                row(kind, json!({"id":"6"})),
            ],
        );
        let single = project(&snapshot, &target(kind, "8", &[])).unwrap();
        assert_eq!(single.count_target, Some((ResourceKind::Track, "8".into())));
        assert_eq!(single.info.comment.unwrap().resource.id, "8");
        assert_eq!(single.info.copy_link, None);
        let group = project(&snapshot, &target(kind, "8", &["7", "6"])).unwrap();
        assert_eq!(group.count_target, single.count_target);
        assert_eq!(group.info.comment, None);
        assert_eq!(group.info.copy_link, None);
    }

    #[test]
    fn row_menu_voice_uses_clicked_program_id_even_when_group_is_retained() {
        let kind = HistoryKind::Voice;
        let snapshot = snapshot(
            kind,
            ["8", "7", "6"]
                .into_iter()
                .map(|id| row(kind, json!({"id":id,"name":"Program","mainTrackId":"38"})))
                .collect(),
        );
        let group = project(&snapshot, &target(kind, "8", &["7", "6"])).unwrap();
        assert_eq!(group.count_target, Some((ResourceKind::Voice, "8".into())));
        assert_eq!(group.info.comment.unwrap().resource.id, "8");
        assert_eq!(
            group.info.copy_link.as_deref(),
            Some("https://music.163.com/dj?id=8")
        );
    }

    #[test]
    fn row_menu_private_and_local_song_ids_never_become_public_resources() {
        let kind = HistoryKind::Song;
        let snapshot = snapshot(
            kind,
            vec![
                row(kind, json!({"id":"8","songType":1})),
                row(kind, json!({"id":"7","privilege":{"cs":1}})),
                row(kind, json!({"local_id":"file"})),
                row(kind, json!({"id":"5","download_task_id":"0".repeat(32)})),
            ],
        );
        for id in ["8", "7", "local:file"] {
            let menu = project(&snapshot, &target(kind, id, &[])).unwrap();
            assert_eq!(menu.count_target, None);
            assert_eq!(menu.info.comment, None);
            assert_eq!(menu.info.copy_link, None);
        }
        let online = project(&snapshot, &target(kind, "5", &[])).unwrap();
        assert_eq!(
            online.info.copy_link.as_deref(),
            Some("https://music.163.com/song?id=5")
        );
    }
}
