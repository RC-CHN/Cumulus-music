use super::*;
use crate::download::{
    completed::{FileOptions, FilePhase},
    directory::{DirectoryOptions, MigrationPhase},
};
impl Host {
    pub(super) fn launch(
        self: &Arc<Self>,
        context: Context,
        id: String,
        migration: bool,
        cancel: bool,
        repair: bool,
    ) -> Result<()> {
        context.guard()?;
        let mut jobs = self.jobs.lock().map_err(|_| Error::LockPoisoned)?;
        jobs.retain(|_, j| j.running.load(Ordering::Acquire) || j.token == context.token);
        if let Some(job) = jobs.get(&id)
            && job.running.load(Ordering::Acquire)
        {
            if job.token != context.token {
                return Err(Error::MutationBusy);
            }
            if cancel {
                job.cancel.store(true, Ordering::Release);
                return Ok(());
            }
            return Err(Error::MutationBusy);
        }
        let job = Arc::new(LocalJob {
            token: context.token.clone(),
            id: id.clone(),
            kind: if repair {
                "repair"
            } else if migration {
                "migration"
            } else {
                "file"
            },
            stop: Arc::new(AtomicBool::new(false)),
            cancel: AtomicBool::new(cancel),
            running: AtomicBool::new(true),
            error: Mutex::new(None),
        });
        jobs.insert(id.clone(), job.clone());
        drop(jobs);
        let host = Arc::downgrade(self);
        tokio::spawn(async move {
            let result = if repair {
                context
                    .service
                    .run(
                        &id,
                        crate::download::service::DownloadRunOptions {
                            stop: job.stop.clone(),
                            ..Default::default()
                        },
                    )
                    .await
                    .map(|_| ())
            } else {
                let captured = context.clone();
                let running = job.clone();
                tokio::task::spawn_blocking(move || execute(&captured, &running, migration))
                    .await
                    .map_err(|_| Error::State("file worker stopped"))
                    .and_then(|r| r)
            };
            if let Err(error) = result
                && !matches!(error, Error::StaleResponse)
                && let Ok(mut value) = job.error.lock()
            {
                *value = Some(if job.stop.load(Ordering::Acquire) {
                    "操作已暂停，进度已保存"
                } else {
                    "操作尚未完成，请检查目录、空间与文件冲突后继续"
                });
            }
            job.running.store(false, Ordering::Release);
            if let Some(host) = host.upgrade()
                && context.guard().is_ok()
            {
                let _ = host.kick(&context);
            }
        });
        Ok(())
    }
    pub(super) fn stop(&self, context: &Context, id: &str) -> Result<()> {
        context.guard()?;
        let jobs = self.jobs.lock().map_err(|_| Error::LockPoisoned)?;
        let job = jobs
            .get(id)
            .filter(|j| j.token == context.token)
            .ok_or(Error::State("operation is not running"))?;
        job.stop.store(true, Ordering::Release);
        Ok(())
    }
}
fn execute(context: &Context, job: &Arc<LocalJob>, migration: bool) -> Result<()> {
    loop {
        context.guard()?;
        if job.stop.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let cancelling = job.cancel.load(Ordering::Acquire);
        let check = job.clone();
        let options = FileOptions {
            observer: Some(Arc::new(move |_, _| {
                if check.stop.load(Ordering::Acquire)
                    || (!cancelling && check.cancel.load(Ordering::Acquire))
                {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            })),
            ..Default::default()
        };
        if migration {
            let check = job.clone();
            let directory = context.directory.clone().with_options(DirectoryOptions {
                files: options,
                observer: Some(Arc::new(move |_, _| {
                    if check.stop.load(Ordering::Acquire)
                        || (!cancelling && check.cancel.load(Ordering::Acquire))
                    {
                        Err(Error::Cancelled)
                    } else {
                        Ok(())
                    }
                })),
                ..Default::default()
            });
            let result = if cancelling {
                directory.cancel(&job.id)
            } else {
                directory.resume(&job.id)
            };
            match result {
                Ok(value)
                    if matches!(
                        value.phase,
                        MigrationPhase::Complete | MigrationPhase::Cancelled
                    ) =>
                {
                    return Ok(());
                }
                Ok(_) => std::thread::sleep(Duration::from_millis(100)),
                Err(Error::Cancelled) if !cancelling && job.cancel.load(Ordering::Acquire) => {
                    continue;
                }
                Err(e) => return Err(e),
            }
        } else {
            let files = context.files.clone().with_options(options);
            let result = if cancelling {
                files.cancel(&job.id)
            } else {
                files.resume(&job.id)
            };
            match result {
                Ok(value) if matches!(value.phase, FilePhase::Complete | FilePhase::Cancelled) => {
                    return Ok(());
                }
                Ok(_) => return Err(Error::State("file operation did not reach a final state")),
                Err(Error::Cancelled) if !cancelling && job.cancel.load(Ordering::Acquire) => {
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }
}
