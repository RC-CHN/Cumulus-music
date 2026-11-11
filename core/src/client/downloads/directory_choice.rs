use super::{Context, DownloadDirectoryPreview, DownloadResult, Host, MusicClient};
use crate::{
    Error, Result,
    account::SessionToken,
    download::directory::{DirectoryChoice, DirectoryChoiceDecision},
};
use std::{path::Path, sync::Arc};

struct Prepared {
    token: SessionToken,
    receipt: String,
    choice: DirectoryChoice,
}

/// One unconsumed preview for this process. Nothing in the filesystem identity
/// or previous account is sent to the renderer or persisted.
#[derive(Default)]
pub(super) struct Choices {
    sequence: u64,
    pending: Option<Prepared>,
}

impl Choices {
    fn begin(&mut self) -> Result<u64> {
        self.sequence = self.sequence.checked_add(1).ok_or(Error::StaleResponse)?;
        self.pending = None;
        Ok(self.sequence)
    }

    fn publish(
        &mut self,
        sequence: u64,
        token: &SessionToken,
        choice: &DirectoryChoice,
    ) -> Result<Option<String>> {
        if self.sequence != sequence {
            return Err(Error::StaleResponse);
        }
        if choice.decision != DirectoryChoiceDecision::Ready {
            return Ok(None);
        }
        let receipt = uuid::Uuid::new_v4().simple().to_string();
        self.pending = Some(Prepared {
            token: token.clone(),
            receipt: receipt.clone(),
            choice: choice.clone(),
        });
        Ok(Some(receipt))
    }

    fn take(
        &mut self,
        token: &SessionToken,
        target: &Path,
        revision: Option<&str>,
        receipt: &str,
    ) -> Result<DirectoryChoice> {
        if !self.pending.as_ref().is_some_and(|prepared| {
            &prepared.token == token
                && prepared.receipt == receipt
                && prepared.choice.target == target
                && prepared.choice.revision.as_deref() == revision
        }) {
            return Err(Error::StaleResponse);
        }
        Ok(self.pending.take().ok_or(Error::StaleResponse)?.choice)
    }
}

impl MusicClient {
    pub async fn download_directory_preview(
        &self,
        epoch: &str,
        root: String,
    ) -> Result<DownloadDirectoryPreview> {
        let host = self.downloads.inner.clone();
        let context = host.context(epoch)?;
        let sequence = host
            .directory_choice
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .begin()?;
        tokio::task::spawn_blocking(move || {
            context.guard()?;
            let choice = context.directory.preview_choice(Path::new(&root))?;
            context.guard()?;
            let receipt = host
                .directory_choice
                .lock()
                .map_err(|_| Error::LockPoisoned)?
                .publish(sequence, &context.token, &choice)?;
            context.guard()?;
            Ok(DownloadDirectoryPreview {
                epoch: context.token.generation.to_string(),
                receipt,
                choice,
            })
        })
        .await
        .map_err(|_| Error::State("directory preview worker stopped"))?
    }
}

impl Host {
    pub(super) async fn apply_directory_choice(
        self: &Arc<Self>,
        context: Context,
        root: &str,
        revision: Option<&str>,
        receipt: &str,
    ) -> Result<DownloadResult> {
        context.guard()?;
        let choice = self
            .directory_choice
            .lock()
            .map_err(|_| Error::LockPoisoned)?
            .take(&context.token, Path::new(root), revision, receipt)?;
        let captured = context.clone();
        let operation_id = tokio::task::spawn_blocking(move || {
            captured.guard()?;
            let operation = captured.directory.change_from_choice(&choice)?;
            captured.guard()?;
            Ok::<_, Error>(operation.map(|item| item.id))
        })
        .await
        .map_err(|_| Error::State("directory confirmation worker stopped"))??;
        context.guard()?;
        if let Some(id) = &operation_id {
            self.launch(context.clone(), id.clone(), true, false, false)?;
        }
        self.kick(&context)?;
        Ok(DownloadResult {
            epoch: context.token.generation.to_string(),
            operation_id,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{download::directory::DownloadDirectory, storage::StateStore};
    use std::sync::Arc;

    #[test]
    fn only_the_current_same_owner_receipt_can_be_consumed_once() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(StateStore::open(dir.path().join("state")).unwrap());
        let root = dir.path().join("music");
        std::fs::create_dir(&root).unwrap();
        let choice = DownloadDirectory::offline(state, "1")
            .unwrap()
            .preview_choice(&root)
            .unwrap();
        let token = SessionToken {
            generation: 1,
            uid: Some("1".into()),
        };
        let mut choices = Choices::default();
        let previous = choices.begin().unwrap();
        let current = choices.begin().unwrap();
        assert!(choices.publish(previous, &token, &choice).is_err());
        let receipt = choices.publish(current, &token, &choice).unwrap().unwrap();
        let old_token = SessionToken {
            generation: 0,
            ..token.clone()
        };
        assert!(
            choices
                .take(&old_token, &choice.target, None, &receipt)
                .is_err()
        );
        assert!(
            choices
                .take(&token, &dir.path().join("other"), None, &receipt)
                .is_err()
        );
        assert!(
            choices
                .take(&token, &choice.target, Some("other"), &receipt)
                .is_err()
        );
        assert_eq!(
            choices
                .take(&token, &choice.target, None, &receipt)
                .unwrap(),
            choice
        );
        assert!(
            choices
                .take(&token, &choice.target, None, &receipt)
                .is_err()
        );
    }
}
