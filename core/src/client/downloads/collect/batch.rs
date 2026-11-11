//! DownloadedTrackBatchOperation's explicit, ordered Favorite selection.
use super::*;
use crate::protocol::api::GuardedPlaylistAdd;
use std::collections::BTreeSet;

const UNVERIFIED: Error =
    Error::InvalidInput("selected download has no verified catalogue identity");

#[derive(Debug, PartialEq, Eq)]
struct Row {
    task: String,
    download_song: String,
    origin: crate::download::service::DownloadOrigin,
    song: String,
    selected: bool,
    fallback: bool,
}

impl Row {
    fn observe(task: DownloadTask) -> Result<Self> {
        let (song, privilege) = collection_resource(&task).ok_or(UNVERIFIED)?;
        // M/21 maps subp to subPriv. Keep an explicit null/zero distinct
        // from absence, and do not resolve conflicting recorded aliases.
        let raw = privilege.get("subp");
        let normalized = privilege.get("subPriv");
        if raw.zip(normalized).is_some_and(|(a, b)| a != b) {
            return Err(UNVERIFIED);
        }
        let (selected, fallback) = match raw.or(normalized) {
            None => (true, true),
            Some(value) if !js_truthy(value) => (false, true),
            Some(value) => {
                let positive = value
                    .as_f64()
                    .or_else(|| value.as_str()?.trim().parse().ok())
                    .filter(|n| n.is_finite())
                    .ok_or(UNVERIFIED)?
                    > 0.0;
                (positive, positive)
            }
        };
        Ok(Self {
            task: task.id,
            download_song: task.song_id,
            origin: task.origin,
            song,
            selected,
            fallback,
        })
    }
}

pub(super) struct Batch {
    tasks: Vec<String>,
    rows: Vec<Row>,
    pub(super) songs: Vec<String>,
}

impl Batch {
    fn capture(context: &Context, tasks: Vec<String>) -> Result<(Self, Vec<String>)> {
        let rows = context
            .service
            .get_ordered_for(&context.token, &tasks)?
            .into_iter()
            .map(Row::observe)
            .collect::<Result<Vec<_>>>()?;
        // M/3085 picks the undefined/positive rows, then uses only the
        // first original row if that filtered group is empty. M/3160
        // reverses the temporary order back before the ordinary add.
        let mut selected = rows.iter().filter(|row| row.selected).collect::<Vec<_>>();
        if selected.is_empty()
            && let Some(first) = rows.first().filter(|row| row.fallback)
        {
            selected.push(first);
        }
        if selected.is_empty() {
            return Err(Error::InvalidInput("no selected download can be collected"));
        }
        let mut unique = BTreeSet::new();
        if !selected.iter().all(|row| unique.insert(&row.song)) {
            return Err(Error::InvalidInput(
                "selected downloads have duplicate catalogue songs",
            ));
        }
        let accepted = selected.iter().map(|row| row.task.clone()).collect();
        let songs = selected.iter().map(|row| row.song.clone()).collect();
        Ok((Self { tasks, rows, songs }, accepted))
    }

    pub(super) fn require(&self, context: &Context) -> Result<()> {
        let current = context
            .service
            .get_ordered_for(&context.token, &self.tasks)?;
        if current
            .into_iter()
            .zip(&self.rows)
            .any(|(task, row)| Row::observe(task).as_ref().ok() != Some(row))
        {
            return Err(Error::StaleResponse);
        }
        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBatchCollectPlan {
    pub receipt: String,
    pub accepted_task_ids: Vec<String>,
}

impl MusicClient {
    pub async fn download_batch_collect_prepare(
        &self,
        epoch: &str,
        task_ids: Vec<String>,
    ) -> Result<AccountData<DownloadBatchCollectPlan>> {
        if !(1..=GuardedPlaylistAdd::MAX_SONGS).contains(&task_ids.len())
            || task_ids.iter().collect::<BTreeSet<_>>().len() != task_ids.len()
        {
            return Err(Error::InvalidInput("invalid download collection selection"));
        }
        let context = self.downloads.inner.context(epoch)?;
        let ticket = self.begin_download_collection()?;
        let api = context.session.native_api(&context.token)?;
        let credentials = api
            .download_collection_scope_revision()?
            .ok_or(Error::State("guarded download collection is unavailable"))?;
        let (source, accepted_task_ids) = tokio::task::spawn_blocking(move || {
            let (batch, accepted) = Batch::capture(&context, task_ids)?;
            let source = Arc::new(Source {
                session: context.session.clone(),
                token: context.token.clone(),
                api,
                credentials,
                receipt: uuid::Uuid::new_v4().simple().to_string(),
                selection: Selection::Batch {
                    batch,
                    context: Box::new(context),
                },
                created: Instant::now(),
            });
            Ok::<_, Error>((source, accepted))
        })
        .await
        .map_err(|_| Error::State("download collection preparation worker stopped"))??;
        let receipt = source.receipt.clone();
        self.install_download_collection(source, ticket)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value: DownloadBatchCollectPlan {
                receipt,
                accepted_task_ids,
            },
        })
    }
}
