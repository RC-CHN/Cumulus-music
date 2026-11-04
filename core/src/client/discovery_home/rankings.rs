use super::{MusicClient, project::label};
use crate::client::{dto::AccountData, rankings::RankingPageView};
use crate::{
    Result,
    account::login_policy::AB_PATH,
    catalog::rankings::{RankingGroup, RankingQuery},
};
use serde_json::{Value, json};
use std::time::Duration;

const RANK_EXPERIMENT: &str = "PH-PC-Home-Ranklist";
const TAG_EXPERIMENT: &str = "PH-PC-tagT";

fn enabled(value: &Value) -> bool {
    let Some(rows) = value["data"]
        .as_array()
        .filter(|rows| rows.len() <= 4096 && rows.iter().all(Value::is_object))
    else {
        return false;
    };
    let lookup = |name: &str, fallback: &'static str| {
        let mut matching = rows.iter().filter(|row| row["expName"] == name);
        let group = match matching.next() {
            Some(row) => row["expGroupName"].as_str()?,
            // Official M/147 refreshWebAbtest substitutes its built-in
            // config when this known experiment has no returned row.
            None => fallback,
        };
        matching.next().is_none().then_some(group)
    };
    lookup(RANK_EXPERIMENT, "t1") == Some("t1")
        && matches!(lookup(TAG_EXPERIMENT, "c"), Some("c" | "t1"))
}

fn safe_label(value: &mut String) -> bool {
    let Some(allowed) = label(&Value::String(std::mem::take(value))) else {
        return false;
    };
    *value = allowed;
    true
}

impl MusicClient {
    pub async fn discovery_home_rankings(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Option<RankingGroup>>> {
        let token = self.playlist_token(epoch)?;
        let enabled = self
            .session
            .run_for(&token, |api, _| async move {
                let result = tokio::time::timeout(
                    Duration::from_secs(3),
                    api.call(
                        AB_PATH,
                        json!({"expNames":format!("{TAG_EXPERIMENT},{RANK_EXPERIMENT}")}),
                        false,
                        &[200],
                    ),
                )
                .await;
                match result {
                    Ok(result) => Ok(super::request::optional(result)?
                        .as_ref()
                        .is_some_and(enabled)),
                    Err(_) => Ok(false),
                }
            })
            .await?;
        let group = if enabled {
            let query = RankingQuery::Home;
            // Use M/400's independent source and the existing account cache.
            let saved = self.ranking_page(epoch, query.clone(), false).await;
            let view = match saved {
                Ok(saved) if saved.value.fresh => saved.value,
                Err(error)
                    if error.is_service_code(301)
                        || matches!(
                            error,
                            crate::Error::StaleResponse | crate::Error::CredentialChanged
                        ) =>
                {
                    return Err(error);
                }
                _ => self.ranking_page(epoch, query, true).await?.value,
            };
            let RankingPageView::Home { groups } = view.page else {
                return Err(crate::Error::Response("invalid homepage ranking view"));
            };
            groups
                .into_iter()
                .find(|group| group.name == "官方榜")
                .and_then(|mut group| {
                    group.cards.retain_mut(|card| {
                        safe_label(&mut card.update_frequency);
                        card.tracks.truncate(3);
                        card.tracks.retain_mut(|track| {
                            safe_label(&mut track.artist);
                            safe_label(&mut track.name)
                        });
                        safe_label(&mut card.name)
                    });
                    group.cards.truncate(18);
                    // The official homepage always uses RankListTop3Card.
                    group.display_type = "TOP_3".into();
                    (!group.cards.is_empty()).then_some(group)
                })
        } else {
            None
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: group,
            })
        })
    }
}
