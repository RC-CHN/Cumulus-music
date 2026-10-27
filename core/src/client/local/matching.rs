use super::{LocalLinkView, LocalMatchCommand, LocalMatchView, MusicClient, view};
use crate::{
    Error, Result,
    account::SessionToken,
    client::{dto::AccountData, mapping},
    library::matching::LocalMatching,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

pub(super) struct Runtime {
    service: Arc<LocalMatching>,
    notify: Arc<Notify>,
    worker: tokio::task::JoinHandle<()>,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl MusicClient {
    fn matching_token(&self, epoch: &str) -> Result<SessionToken> {
        let token = self.session.token()?;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        };
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(token)
    }
    async fn matching_context(&self, epoch: &str) -> Result<(Arc<LocalMatching>, Arc<Notify>)> {
        let token = self.matching_token(epoch)?;
        let local = self.local.context().await?;
        let mut runtime = self.local.matching.lock().await;
        if let Some(current) = runtime.as_ref()
            && current.service.token() == &token
        {
            return Ok((current.service.clone(), current.notify.clone()));
        }
        if let Some(mut old) = runtime.take() {
            old.worker.abort();
            let _ = (&mut old.worker).await;
        }
        let service = Arc::new(LocalMatching::new(
            self.session.clone(),
            local.library.clone(),
        )?);
        if service.token() != &token {
            return Err(Error::StaleResponse);
        };
        let captured = service.clone();
        tokio::task::spawn_blocking(move || captured.recover())
            .await
            .map_err(|_| Error::State("matching recovery worker stopped"))??;
        let notify = Arc::new(Notify::new());
        let wake = notify.clone();
        let captured = service.clone();
        let session = self.session.clone();
        let worker = tokio::spawn(async move {
            loop {
                if session.with_token(captured.token(), |_| Ok(())).is_err() {
                    break;
                }
                match captured.step().await {
                    Ok(Some(_)) | Err(Error::Cancelled) => continue,
                    Err(Error::StaleResponse) => break,
                    Ok(None) | Err(_) => (),
                }
                // Failed jobs are durable and require an explicit resume.
                tokio::select! { _=wake.notified()=>{}, _=tokio::time::sleep(Duration::from_secs(1))=>{} }
            }
        });
        *runtime = Some(Runtime {
            service: service.clone(),
            notify: notify.clone(),
            worker,
        });
        Ok((service, notify))
    }
    pub async fn local_match_view(
        &self,
        epoch: &str,
        ids: Vec<String>,
    ) -> Result<AccountData<LocalMatchView>> {
        let token = self.matching_token(epoch)?;
        if ids.len() > 500 {
            return Err(Error::InvalidInput("local link page exceeds 500"));
        }
        let (service, _) = self.matching_context(epoch).await?;
        let local = self.local.context().await?;
        let session = self.session.clone();
        let expected = token.clone();
        let value = tokio::task::spawn_blocking(move || {
            let mappings = local.library.online_mappings(&session, &expected, &ids)?;
            let links = mappings
                .into_iter()
                .map(|(id, m)| {
                    Ok(LocalLinkView {
                        id,
                        revision: view::revision(&m.revision),
                        song_id: m.song_id,
                        track: m.song.as_ref().map(mapping::track).transpose()?,
                        source: m.source,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok::<_, Error>(LocalMatchView {
                job: service.state()?,
                links,
            })
        })
        .await
        .map_err(|_| Error::State("matching view worker stopped"))??;
        self.session.with_token(&token, |_| Ok(()))?;
        Ok(AccountData {
            epoch: token.generation.to_string(),
            value,
        })
    }
    pub async fn local_match_command(&self, epoch: &str, command: LocalMatchCommand) -> Result<()> {
        self.matching_token(epoch)?;
        let (service, notify) = self.matching_context(epoch).await?;
        let library = self.local.context().await?.library.clone();
        match command {
            LocalMatchCommand::Bind {
                id,
                revision,
                song_id,
            } => {
                let key = id.clone();
                let item = tokio::task::spawn_blocking(move || library.item(&key))
                    .await
                    .map_err(|_| Error::State("local mapping worker stopped"))??;
                if view::revision(&item.revision) != revision {
                    return Err(Error::QueueChanged);
                };
                service
                    .bind(&id, &item.revision, &serde_json::Value::String(song_id))
                    .await?;
            }
            command => {
                tokio::task::spawn_blocking(move || {
                    match command {
                        LocalMatchCommand::Start { query } => {
                            service.start(&query)?;
                        }
                        LocalMatchCommand::Control { id, action } => {
                            service.control(&id, action)?;
                        }
                        LocalMatchCommand::Clear { id, revision } => {
                            let item = library.item(&id)?;
                            if view::revision(&item.revision) != revision {
                                return Err(Error::QueueChanged);
                            };
                            service.clear(&id, &item.revision)?;
                        }
                        LocalMatchCommand::Bind { .. } => unreachable!(),
                    }
                    Ok::<_, Error>(())
                })
                .await
                .map_err(|_| Error::State("local mapping command worker stopped"))??;
            }
        }
        notify.notify_one();
        Ok(())
    }
}
