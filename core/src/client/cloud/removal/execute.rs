use super::*;

impl MusicClient {
    pub async fn cloud_delete_execute(
        &self,
        epoch: &str,
        id: &str,
        delete_local: bool,
        retry_local: bool,
    ) -> Result<CloudRemovalView> {
        let context = self.cloud.inner.context(epoch).await?;
        let _lock = Arc::new(lock(&context, id)?);
        let mut plan = load(&context, id)?.ok_or(Error::State("cloud removal plan not found"))?;
        if plan.choice.is_some_and(|choice| choice != delete_local) {
            return Err(Error::State("cloud removal choice changed"));
        }
        if plan.blocked {
            return view::project(&context, &plan, context.library.find_operation(id)?, false);
        }
        if plan.choice.is_none() {
            plan.choice = Some(delete_local);
            save(&context, &plan)?;
        }
        let remote = match context
            .library
            .reserve_delete(id, &plan.ids, &plan.revision)
            .await
        {
            Ok(op) => op,
            Err(Error::StaleResponse) => return Err(Error::StaleResponse),
            Err(e) if e.is_service_code(301) || matches!(e.root(), Error::Storage(_)) => {
                return Err(e);
            }
            Err(e) => {
                plan.issue = Some(
                    if matches!(e.root(), Error::QueueChanged | Error::MutationUncertain) {
                        Issue::Preflight
                    } else {
                        Issue::RemoteUnavailable
                    },
                );
                plan.blocked = matches!(e.root(), Error::QueueChanged | Error::MutationUncertain);
                save(&context, &plan)?;
                return view::project(&context, &plan, context.library.find_operation(id)?, false);
            }
        };
        // A duplicate initial RPC only returns its receipt after local work;
        // retries of failed local entries require this explicit retry flag.
        if !delete_local && !plan.local_done {
            for file in &mut plan.files {
                file.state = CloudRemovalFileState::Skipped;
            }
            plan.local_done = true;
            save(&context, &plan)?;
        }
        if delete_local
            && (!plan.local_done
                || (retry_local
                    && plan.files.iter().any(|f| {
                        matches!(
                            f.state,
                            CloudRemovalFileState::Failed | CloudRemovalFileState::Removing
                        )
                    })))
        {
            let local = self.local.context().await?;
            let mut current = plan.clone();
            let captured = context.clone();
            let local_lock = _lock.clone();
            plan = tokio::task::spawn_blocking(move || {
                // Dropping an async caller does not stop a blocking file worker.
                // Keep its intent fenced until that worker actually finishes.
                let _held = local_lock;
                for index in 0..current.files.len() {
                    captured.guard()?;
                    let file = &current.files[index];
                    let pending = matches!(
                        file.state,
                        CloudRemovalFileState::Pending | CloudRemovalFileState::Removing
                    ) || (retry_local && file.state == CloudRemovalFileState::Failed);
                    if !pending {
                        continue;
                    }
                    if !delete_local {
                        current.files[index].state = CloudRemovalFileState::Skipped;
                        save(&captured, &current)?;
                        continue;
                    }
                    current.files[index].state = CloudRemovalFileState::Removing;
                    save(&captured, &current)?;
                    let file = &current.files[index];
                    let result = match &file.origin {
                        Origin::Upload { .. } => files::upload(&captured, &local.library, file),
                        Origin::Download { .. } => files::download(&captured, file),
                    };
                    captured.guard()?;
                    match result {
                        Ok((state, issue)) => {
                            current.files[index].state = state;
                            current.files[index].issue = issue;
                        }
                        Err(e) if matches!(e.root(), Error::StaleResponse | Error::Storage(_)) => {
                            return Err(e);
                        }
                        Err(e)
                            if matches!(
                                e.root(),
                                Error::State(
                                    "download changed after deletion confirmation"
                                        | "download file identity changed after confirmation"
                                        | "download or lyric was modified; refusing removal"
                                        | "download does not belong to this account"
                                        | "cloud source association changed"
                                ) | Error::Media(_)
                            ) =>
                        {
                            current.files[index].state = CloudRemovalFileState::Retained;
                            current.files[index].issue = Some(Issue::Changed);
                        }
                        Err(_) => {
                            current.files[index].state = CloudRemovalFileState::Failed;
                            current.files[index].issue = Some(Issue::Unavailable);
                        }
                    }
                    save(&captured, &current)?;
                }
                current.local_done = true;
                if matches!(current.issue, Some(Issue::RemoteUnavailable)) {
                    current.issue = None;
                }
                save(&captured, &current)?;
                Ok(current)
            })
            .await
            .map_err(|_| Error::State("cloud removal local worker stopped"))??;
        }
        let remote = if remote.phase == DeletePhase::Preparing {
            match context.library.send_reserved_delete(id).await {
                Ok(op) => Some(op),
                Err(Error::StaleResponse) => return Err(Error::StaleResponse),
                Err(e) if e.is_service_code(301) || matches!(e.root(), Error::Storage(_)) => {
                    return Err(e);
                }
                Err(e) => {
                    plan.issue = Some(if matches!(e.root(), Error::QueueChanged) {
                        Issue::RemoteChanged
                    } else {
                        Issue::RemoteUnavailable
                    });
                    save(&context, &plan)?;
                    context.library.find_operation(id)?
                }
            }
        } else {
            Some(remote)
        };
        context.guard()?;
        view::project(&context, &plan, remote, false)
    }
}
