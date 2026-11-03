//! Journal for explicit, single-tag style preference saves.
use super::{AccountData, MusicClient, StyleCategory, applied, configuration};
use crate::{Error, Result, account::SessionToken, catalog::CatalogApi, storage::OperationLock};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const JOURNAL: &str = "style-tag-intents-v1";

#[derive(Clone, Copy)]
enum ReadStylePreference {
    Default,
    AfterSave,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StylePreference {
    pub category_id: String,
    pub tag_ids: Vec<String>,
    pub revision: String,
}

impl StylePreference {
    pub(in crate::client) fn from_category(category: &StyleCategory) -> Self {
        let tag_ids = category
            .tags
            .iter()
            .map(|tag| tag.id.clone())
            .collect::<Vec<_>>();
        // IDs cannot contain control characters; use NUL separators.
        let mut digest = Sha256::new();
        for id in std::iter::once(&category.id).chain(&tag_ids) {
            digest.update(id);
            digest.update([0]);
        }
        Self {
            category_id: category.id.clone(),
            tag_ids,
            revision: hex::encode(digest.finalize()),
        }
    }
    fn matches(&self, category: &str, tag: &str) -> bool {
        self.category_id == category && self.tag_ids == [tag]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleTagPhase {
    Pending,
    Unchanged,
    Confirmed,
    Rejected,
    Uncertain,
    AcceptedCurrentState,
}

impl StyleTagPhase {
    fn unresolved(self) -> bool {
        matches!(self, Self::Pending | Self::Uncertain)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleTagOperation {
    pub id: String,
    pub expected_revision: String,
    pub category_id: String,
    pub tag_id: String,
    pub phase: StyleTagPhase,
    pub observed: Option<StylePreference>,
}

fn validate_id(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id) {
        Ok(())
    } else {
        Err(Error::InvalidInput(
            "style tag intent requires a canonical UUID",
        ))
    }
}

fn valid_key(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && !id.chars().any(char::is_control)
}

fn unsent(error: Error) -> Error {
    if error.is_service_code(301)
        || matches!(
            error.root(),
            Error::StaleResponse | Error::RequestNotSent(_)
        )
    {
        error
    } else {
        Error::RequestNotSent(Box::new(error))
    }
}

impl MusicClient {
    fn style_tag_lock(&self, token: &SessionToken) -> Result<(String, OperationLock)> {
        let owner = self.session.with_token(token, |uid| Ok(uid.to_owned()))?;
        let lock = OperationLock::acquire(self.registry.store().directory(), JOURNAL, &owner)?;
        Ok((owner, lock))
    }

    fn decode_style_tag(key: &str, value: Value) -> Result<StyleTagOperation> {
        validate_id(key)?;
        let op: StyleTagOperation = serde_json::from_value(value)?;
        if op.id != key
            || !valid_key(&op.expected_revision)
            || !valid_key(&op.category_id)
            || !valid_key(&op.tag_id)
        {
            return Err(Error::State("invalid style tag journal record"));
        }
        Ok(op)
    }

    pub fn style_tag_operations(&self, epoch: &str) -> Result<AccountData<Vec<StyleTagOperation>>> {
        self.style_tag_receipts(epoch, None)
    }

    /// The existing list now contains only the unresolved operation and the
    /// exact requested receipt, even if that receipt is arbitrarily old.
    pub fn style_tag_receipts(
        &self,
        epoch: &str,
        id: Option<&str>,
    ) -> Result<AccountData<Vec<StyleTagOperation>>> {
        if let Some(id) = id {
            validate_id(id)?;
        }
        let token = self.player_token(epoch)?;
        self.session.with_token(&token, |owner| {
            let value = self.registry.store().read_snapshot(|tx| {
                let mut items = Vec::with_capacity(2);
                if let Some((key, value)) = tx.unresolved_style_tag(owner)? {
                    items.push(Self::decode_style_tag(&key, value)?);
                }
                if let Some(id) = id.filter(|id| items.first().is_none_or(|op| op.id != *id))
                    && let Some(value) = tx.get(owner, JOURNAL, id)?
                {
                    items.push(Self::decode_style_tag(id, value)?);
                }
                Ok(items)
            })?;
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    fn style_tag_by_id(&self, token: &SessionToken, id: &str) -> Result<Option<StyleTagOperation>> {
        self.session.with_token(token, |owner| {
            self.registry
                .store()
                .get(owner, JOURNAL, id)?
                .map(|value| Self::decode_style_tag(id, value))
                .transpose()
        })
    }

    async fn read_style_preference(
        &self,
        token: &SessionToken,
        read: ReadStylePreference,
    ) -> Result<StylePreference> {
        self.session
            .run_for(token, |api, _| async move {
                let catalog = CatalogApi::new(api);
                let response = match read {
                    ReadStylePreference::Default => catalog.default_style_recommendations().await?,
                    ReadStylePreference::AfterSave => {
                        catalog.style_recommendations_after_save().await?
                    }
                };
                applied(response["data"].get("tags"))
                    .as_ref()
                    .map(StylePreference::from_category)
                    .ok_or(Error::Response("applied style preference unavailable"))
            })
            .await
    }

    async fn style_tag_payload(
        &self,
        token: &SessionToken,
        category_id: &str,
        tag_id: &str,
    ) -> Result<Value> {
        self.session
            .run_for(token, |api, _| async move {
                let (raw, groups) = configuration(&*api).await?;
                let group = groups
                    .iter()
                    .position(|group| group.id == category_id)
                    .ok_or(Error::InvalidInput("unknown style category"))?;
                let tag = groups[group]
                    .tags
                    .iter()
                    .position(|tag| tag.id == tag_id)
                    .ok_or(Error::InvalidInput("unknown style tag in category"))?;
                Ok(json!({"tags": {
                    "categoryId": raw[group]["categoryId"],
                    "tagIds": [raw[group]["tagVOList"][tag]["tagId"]],
                }}))
            })
            .await
    }

    pub async fn save_style_tag(
        &self,
        epoch: &str,
        id: &str,
        expected_revision: &str,
        category_id: &str,
        tag_id: &str,
    ) -> Result<AccountData<StyleTagOperation>> {
        validate_id(id)?;
        if !valid_key(expected_revision) || !valid_key(category_id) || !valid_key(tag_id) {
            return Err(Error::InvalidInput("invalid style tag selection"));
        }
        let token = self.player_token(epoch)?;
        let (owner, _lock) = self.style_tag_lock(&token)?;
        if let Some(op) = self.style_tag_by_id(&token, id)? {
            if op.expected_revision != expected_revision
                || op.category_id != category_id
                || op.tag_id != tag_id
            {
                return Err(Error::InvalidInput("style tag intent ID reused"));
            }
            return self.session.with_token(&token, |_| {
                Ok(AccountData {
                    epoch: epoch.into(),
                    value: op,
                })
            });
        }
        let has_unresolved = self.session.with_token(&token, |owner| {
            self.registry.store().read_snapshot(|tx| {
                tx.unresolved_style_tag(owner)?
                    .map(|(key, value)| Self::decode_style_tag(&key, value))
                    .transpose()
                    .map(|op| op.is_some())
            })
        })?;
        if has_unresolved {
            return Err(Error::MutationUncertain);
        }
        let displayed = self.session.with_token(&token, |_| {
            self.style_preference_at(&token, expected_revision)
        })?;
        let wire = self
            .style_tag_payload(&token, category_id, tag_id)
            .await
            .map_err(unsent)?;
        let before = self
            .read_style_preference(&token, ReadStylePreference::Default)
            .await
            .map_err(unsent)?;
        if before != displayed {
            return Err(Error::QueueChanged);
        }
        let unchanged = before.matches(category_id, tag_id);
        let mut op = StyleTagOperation {
            id: id.into(),
            expected_revision: expected_revision.into(),
            category_id: category_id.into(),
            tag_id: tag_id.into(),
            phase: if unchanged {
                StyleTagPhase::Unchanged
            } else {
                StyleTagPhase::Pending
            },
            observed: unchanged.then_some(before),
        };
        self.session.with_token(&token, |uid| {
            self.style_preference_at(&token, expected_revision)?;
            self.store_style_tag(uid, &op)
        })?;
        if !unchanged {
            let result = self
                .session
                .run_for(&token, |api, _| async move {
                    api.call("/api/homepage/daily/song/tag/save", wire, false, &[200])
                        .await
                })
                .await;
            match result {
                Ok(_) => {
                    if let Ok(value) = self
                        .observe_style_tag(
                            &token,
                            epoch,
                            op.clone(),
                            None,
                            ReadStylePreference::AfterSave,
                        )
                        .await
                    {
                        return Ok(value);
                    }
                    op.phase = StyleTagPhase::Uncertain;
                }
                Err(error) => {
                    op.phase = if matches!(error.root(), Error::RequestNotSent(_)) {
                        StyleTagPhase::Rejected
                    } else {
                        StyleTagPhase::Uncertain
                    };
                }
            }
            // Preserve the original account's responsibility after account replacement.
            self.store_style_tag(&owner, &op)?;
        }
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: op,
            })
        })
    }

    fn store_style_tag(&self, uid: &str, op: &StyleTagOperation) -> Result<()> {
        self.registry
            .store()
            .put(uid, JOURNAL, &op.id, &serde_json::to_value(op)?)
    }

    pub async fn reconcile_style_tag(
        &self,
        epoch: &str,
        id: &str,
    ) -> Result<AccountData<StyleTagOperation>> {
        self.finish_style_tag(epoch, id, None).await
    }

    pub async fn accept_current_style_tag(
        &self,
        epoch: &str,
        id: &str,
        observed_revision: &str,
    ) -> Result<AccountData<StyleTagOperation>> {
        self.finish_style_tag(epoch, id, Some(observed_revision))
            .await
    }

    async fn finish_style_tag(
        &self,
        epoch: &str,
        id: &str,
        accept: Option<&str>,
    ) -> Result<AccountData<StyleTagOperation>> {
        validate_id(id)?;
        let token = self.player_token(epoch)?;
        let (_owner, _lock) = self.style_tag_lock(&token)?;
        let op = self
            .style_tag_by_id(&token, id)?
            .ok_or(Error::State("no style tag intent to reconcile"))?;
        if !op.phase.unresolved() {
            return self.session.with_token(&token, |_| {
                Ok(AccountData {
                    epoch: epoch.into(),
                    value: op,
                })
            });
        }
        if accept.is_some() && op.observed.as_ref().map(|value| value.revision.as_str()) != accept {
            return Err(Error::State("reconcile style tag before acceptance"));
        }
        self.observe_style_tag(&token, epoch, op, accept, ReadStylePreference::Default)
            .await
    }

    async fn observe_style_tag(
        &self,
        token: &SessionToken,
        epoch: &str,
        mut op: StyleTagOperation,
        accept: Option<&str>,
        read: ReadStylePreference,
    ) -> Result<AccountData<StyleTagOperation>> {
        let observed = self.read_style_preference(token, read).await?;
        op.phase = if observed.matches(&op.category_id, &op.tag_id) {
            StyleTagPhase::Confirmed
        } else if accept == Some(observed.revision.as_str()) {
            StyleTagPhase::AcceptedCurrentState
        } else {
            StyleTagPhase::Uncertain
        };
        op.observed = Some(observed);
        self.session.with_token(token, |uid| {
            self.store_style_tag(uid, &op)?;
            Ok(AccountData {
                epoch: epoch.into(),
                value: op,
            })
        })
    }
}
