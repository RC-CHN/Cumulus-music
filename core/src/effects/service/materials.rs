use super::{
    EffectsLibrary, MATERIALS, files,
    records::{MaterialHandle, MaterialOutcome, MaterialPhase, Record},
};
use crate::{
    Error, Result,
    account::SessionToken,
    download::transfer::{
        CreateTransfer, RunOptions, TransferDirective, TransferJob, TransferStatus,
    },
    effects::{api::EffectsApi, catalog::EffectId},
};
use std::sync::Arc;

impl EffectsLibrary {
    /// Every call (including resume/cache use) re-reads detail and authorization.
    /// A changed material fingerprint starts a separate immutable transfer.
    pub async fn acquire_for(
        &self,
        token: &SessionToken,
        effect: EffectId,
        mut options: RunOptions,
    ) -> Result<MaterialOutcome> {
        if options.byte_budget == Some(0) {
            return Err(Error::InvalidInput("material byte budget must be positive"));
        }
        let owner = self.owner(token)?;
        // Detail request type is a different enum from catalogue soundType.
        let sound_type = 10;
        let _lock = self.lock(&owner, &format!("material:{}:{sound_type}", effect.value()))?;
        let sound = self
            .session
            .run_for(token, |api, _| async move {
                EffectsApi::new(api).pc_detail(effect).await
            })
            .await?
            .ok_or(Error::Response("sound detail is unavailable"))?;
        let authorization = self
            .session
            .run_for(token, |api, _| async move {
                EffectsApi::new(api).authorization(effect).await
            })
            .await?
            .filter(|auth| auth.can_use)
            .ok_or(Error::State("effect material is not authorized"))?;
        let descriptor = sound
            .parameter_material()?
            .ok_or(Error::Response("sound has no parameter material"))?;
        let handle = MaterialHandle::new(effect, sound_type, &descriptor);
        let mut record = self.record(token, &handle)?.unwrap_or_else(|| Record {
            handle: handle.clone(),
            descriptor: descriptor.clone(),
            attempt: attempt(),
            abandoned: vec![],
            phase: MaterialPhase::Pending,
            plain_sha256: None,
        });
        if record.phase == MaterialPhase::Removing {
            return Err(Error::State(
                "finish removing this effect material before acquiring it",
            ));
        }
        if record.phase == MaterialPhase::Removed {
            record.attempt = attempt();
            record.phase = MaterialPhase::Pending;
            record.plain_sha256 = None;
        }
        // The URL may have expired. The fingerprint deliberately excludes URL;
        // the transfer layer keeps bytes only for identical size and MD5.
        record.descriptor = descriptor;
        record.phase = MaterialPhase::Pending;
        record.plain_sha256 = None;
        self.save(token, &record)?;
        let manager = self.transfers(&owner)?;
        let root = self.session.with_token(token, |_| {
            files::root(self.store.directory(), &owner, &record)
        })?;
        let mut child = manager
            .list()?
            .into_iter()
            .find(|job| job.id == record.attempt);
        if child.is_none() {
            // A process can die after create_new(.part) and before the child
            // journal commit. Retain that unowned file; never adopt or unlink it.
            // Reserve a new attempt durably before trying another path.
            while files::exists(&files::target(&root, &record.attempt))?
                || files::exists(&root.join(format!(".cumulus-{}.part", record.attempt)))?
            {
                if record.abandoned.len() >= 64 {
                    return Err(Error::State("too many orphaned effect material attempts"));
                }
                record
                    .abandoned
                    .push(std::mem::replace(&mut record.attempt, attempt()));
                self.save(token, &record)?;
            }
            child = Some(self.session.with_token(token, |_| {
                manager.create(CreateTransfer {
                    resource: record.descriptor.resource(),
                    target: files::target(&root, &record.attempt),
                    resource_id: Some(resource_id(&handle)),
                    id: Some(record.attempt.clone()),
                })
            })?);
        }
        let child = child.expect("existing or created material transfer");
        validate_child(&record, &child, &files::target(&root, &record.attempt))?;
        if !matches!(
            child.status,
            TransferStatus::Complete
                | TransferStatus::Publishing
                | TransferStatus::Cancelled
                | TransferStatus::Cancelling
        ) {
            self.session.with_token(token, |_| {
                manager.refresh_resource(&child.id, record.descriptor.resource())
            })?;
        }
        let session = self.session.clone();
        let expected = token.clone();
        let original = options.control.clone();
        options.control = Arc::new(move || {
            session.check_generation(&expected)?;
            original.checkpoint()
        });
        self.session.check_generation(token)?;
        let control = options.control.clone();
        let child = manager.run(&record.attempt, options).await?;
        self.session.check_generation(token)?;
        let material = if child.status == TransferStatus::Complete
            && control.checkpoint()? == TransferDirective::Continue
        {
            let verifying = child.clone();
            let material = tokio::task::spawn_blocking(move || files::verify(&verifying))
                .await
                .map_err(|_| Error::State("effect material verification worker stopped"))??;
            if control.checkpoint()? == TransferDirective::Continue {
                record.phase = MaterialPhase::Ready;
                record.plain_sha256 = Some(material.plain_sha256.clone());
                self.save(token, &record)?;
                Some(material)
            } else {
                None
            }
        } else {
            None
        };
        self.session.check_generation(token)?;
        Ok(MaterialOutcome {
            handle,
            sound,
            authorization,
            transfer_id: child.id,
            bytes: child.bytes,
            material,
        })
    }

    /// Remove only the recorded attempt's owned files. The durable Removing
    /// intent can be retried after a crash; unrelated/orphaned files are retained.
    /// Acquisition uses the same account/effect lock, so removal cannot race it.
    pub fn remove_for(&self, token: &SessionToken, handle: &MaterialHandle) -> Result<()> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner, &handle.lock_key())?;
        let mut record = self.record(token, handle)?.ok_or(Error::State(
            "effect material does not belong to this account",
        ))?;
        if record.phase == MaterialPhase::Removed {
            return Ok(());
        }
        record.phase = MaterialPhase::Removing;
        self.save(token, &record)?;
        let manager = self.transfers(&owner)?;
        if let Some(child) = manager
            .list()?
            .into_iter()
            .find(|job| job.id == record.attempt)
        {
            let root = files::root(self.store.directory(), &owner, &record)?;
            let target = files::target(&root, &record.attempt);
            validate_child(&record, &child, &target)?;
            self.session
                .with_token(token, |_| manager.discard_intermediate(&child.id, &target))?;
        }
        record.phase = MaterialPhase::Removed;
        record.plain_sha256 = None;
        self.save(token, &record)
    }

    pub fn phase_for(
        &self,
        token: &SessionToken,
        handle: &MaterialHandle,
    ) -> Result<Option<MaterialPhase>> {
        Ok(self.record(token, handle)?.map(|record| record.phase))
    }
    fn record(&self, token: &SessionToken, handle: &MaterialHandle) -> Result<Option<Record>> {
        self.session.with_token(token, |owner| {
            let record: Option<Record> = self
                .store
                .get(owner, MATERIALS, &handle.key)?
                .map(serde_json::from_value)
                .transpose()?;
            if let Some(record) = &record {
                record.validate(handle)?;
            }
            Ok(record)
        })
    }
    fn save(&self, token: &SessionToken, record: &Record) -> Result<()> {
        record.validate(&record.handle)?;
        self.session.with_token(token, |owner| {
            self.store.put(
                owner,
                MATERIALS,
                &record.handle.key,
                &serde_json::to_value(record)?,
            )
        })
    }
}
fn resource_id(handle: &MaterialHandle) -> String {
    format!("effect-material:{}", handle.key)
}
fn attempt() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
fn validate_child(record: &Record, child: &TransferJob, target: &std::path::Path) -> Result<()> {
    if child.id != record.attempt
        || child.target != target
        || child.resource_id.as_deref() != Some(&resource_id(&record.handle))
        || child.resource.size != Some(record.descriptor.size())
        || child.resource.md5.as_deref() != Some(record.descriptor.md5())
    {
        return Err(Error::State("effect material transfer identity mismatch"));
    }
    Ok(())
}
