use super::{MusicClient, dto::AccountData};
use crate::{
    Result,
    account::{
        SessionToken,
        profile::{
            AccountProfile, SelfProfileDetailView,
            edit::{ProfileEditPage, ProfileEditPhase, ProfileEditor, ProfilePatch},
            regions::RegionTable,
        },
    },
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AccountProfileEditCommand {
    Change {
        id: String,
        expected_revision: String,
        patch: ProfilePatch,
    },
    Reconcile {
        id: String,
    },
    AcceptCurrent {
        id: String,
        expected_revision: String,
    },
}

impl MusicClient {
    pub async fn account_profile_detail(
        &self,
        epoch: &str,
    ) -> Result<AccountData<SelfProfileDetailView>> {
        let token = self.player_token(epoch)?;
        let value = AccountProfile::new(self.session.clone())
            .read_for(&token)
            .await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn account_profile_regions(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Option<RegionTable>>> {
        let token = self.player_token(epoch)?;
        let value = RegionTable::refresh_for(self.session.clone(), &token).await?;
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }

    pub async fn account_profile_edit_page(
        &self,
        epoch: &str,
        refresh: bool,
    ) -> Result<AccountData<ProfileEditPage>> {
        let token = self.player_token(epoch)?;
        let editor = ProfileEditor::new(self.session.clone())?;
        let page = if refresh {
            editor.refresh_for(&token).await?
        } else {
            editor.page_for(&token)?
        };
        self.profile_edit_result(epoch, &token, page)
    }

    pub async fn account_profile_edit_command(
        &self,
        epoch: &str,
        command: AccountProfileEditCommand,
    ) -> Result<AccountData<ProfileEditPage>> {
        let token = self.player_token(epoch)?;
        let editor = ProfileEditor::new(self.session.clone())?;
        match command {
            AccountProfileEditCommand::Change {
                id,
                expected_revision,
                patch,
            } => {
                editor
                    .change_for(&token, &id, &expected_revision, patch)
                    .await?;
            }
            AccountProfileEditCommand::Reconcile { id } => {
                editor.reconcile_for(&token, &id).await?;
            }
            AccountProfileEditCommand::AcceptCurrent {
                id,
                expected_revision,
            } => {
                editor
                    .accept_current_for(&token, &id, &expected_revision)
                    .await?;
            }
        }
        self.profile_edit_result(epoch, &token, editor.page_for(&token)?)
    }

    fn profile_edit_result(
        &self,
        epoch: &str,
        token: &SessionToken,
        page: ProfileEditPage,
    ) -> Result<AccountData<ProfileEditPage>> {
        // A journal already resolved the remote write. A failure here is
        // local metadata only; a later page read can retry this bookkeeping.
        if page.operations.iter().any(|op| {
            matches!(
                op.phase,
                ProfileEditPhase::Confirmed | ProfileEditPhase::AcceptedCurrentState
            )
        }) && let Some(client) = self.native_account_client()?
        {
            self.registry
                .record_current_for(&self.session, &client, token)?;
        }
        self.session.with_token(token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value: page,
            })
        })
    }
}
