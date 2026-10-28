use super::*;
fn validate(id: &str, revision: &str) -> Result<()> {
    super::super::validate_id(id)?;
    if revision.len() != 64 || !revision.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidInput("invalid relay setting revision"));
    }
    Ok(())
}
fn same(op: &RelaySettingIntent, device: &str, revision: &str, enabled: bool) -> Result<()> {
    if op.device_id != device || op.expected_revision != revision || op.enabled != enabled {
        return Err(Error::InvalidInput("relay setting intent reused"));
    }
    Ok(())
}
fn intent(id: &str, device_id: String, revision: &str, enabled: bool) -> RelaySettingIntent {
    RelaySettingIntent {
        id: id.into(),
        device_id,
        expected_revision: revision.into(),
        enabled,
        before: None,
        phase: SettingsPhase::Pending,
        created_at: now(),
        last_read_at: None,
        read_revision: None,
        observed: None,
        service_code: None,
    }
}
impl RelaySettings {
    pub fn cancel_unseen_for(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
        enabled: bool,
    ) -> Result<RelaySettingIntent> {
        validate(id, revision)?;
        let (owner, _lock) = self.relay.lock(token)?;
        let device = self.require_device(token)?;
        if let Some(op) = self
            .all_operations(token)?
            .into_iter()
            .find(|op| op.id == id)
        {
            same(&op, &device, revision, enabled)?;
            return Ok(op);
        }
        let mut op = intent(id, device, revision, enabled);
        op.phase = SettingsPhase::Cancelled;
        self.relay
            .session
            .with_token(token, |_| self.save_original(&owner, &op))?;
        Ok(op)
    }
    pub async fn change_for(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
        enabled: bool,
    ) -> Result<RelaySettingIntent> {
        validate(id, revision)?;
        let (owner, _lock) = self.relay.lock(token)?;
        let device = self.require_device(token)?;
        let prior = self.all_operations(token)?;
        if let Some(op) = prior.iter().find(|op| op.id == id) {
            same(op, &device, revision, enabled)?;
            return Ok(op.clone());
        }
        if prior
            .iter()
            .any(|op| op.device_id == device && op.phase.unresolved())
        {
            return Err(Error::MutationUncertain);
        }
        let cached = self
            .snapshot_for(token)?
            .ok_or(Error::State("read relay setting first"))?;
        if cached.revision != revision {
            return Err(Error::State("relay setting changed"));
        }
        if cached.enabled.is_none() {
            return Err(Error::State("relay setting unavailable"));
        }
        let eligible = self
            .relay
            .session
            .run_for(token, |api, _| async move {
                RelayApi::new(api).experiment().await
            })
            .await?;
        if !eligible {
            return Err(Error::State("relay setting experiment unavailable"));
        }
        let snapshot = self.read(token).await?;
        if snapshot.revision != revision || snapshot.device_id != device {
            return Err(Error::State("relay setting changed"));
        }
        let mut op = intent(id, device.clone(), revision, enabled);
        op.before = snapshot.enabled;
        if op.before == Some(enabled) {
            op.phase = SettingsPhase::Unchanged;
        }
        self.relay
            .session
            .with_token(token, |_| self.save_original(&owner, &op))?;
        if op.phase == SettingsPhase::Unchanged {
            return Ok(op);
        }
        // A pending or interrupted device switch cannot authorize a stale alert.
        self.relay.invalidate(token)?;
        let result = self
            .relay
            .session
            .run_for(token, |api, _| async move {
                if api.local_device_id()?.as_deref() != Some(device.as_str()) {
                    return Err(Error::StaleResponse);
                }
                RelayApi::new(api).update_setting(&device, enabled).await
            })
            .await;
        if let Err(error) = result {
            op.phase = if matches!(error.root(), Error::RequestNotSent(_))
                || [301, 403, 505, 506, 511, 512]
                    .iter()
                    .any(|c| error.is_service_code(*c))
            {
                SettingsPhase::Rejected
            } else {
                SettingsPhase::Uncertain
            };
            op.service_code = match error.root() {
                Error::Service { code, .. } => code.as_i64(),
                _ => None,
            };
            self.save_original(&owner, &op)?;
            return Err(error);
        }
        self.readback(token, op).await
    }
    async fn readback(
        &self,
        token: &SessionToken,
        mut op: RelaySettingIntent,
    ) -> Result<RelaySettingIntent> {
        let snapshot = self.read(token).await?;
        if snapshot.device_id != op.device_id {
            return Err(Error::StaleResponse);
        }
        op.observed = snapshot.enabled;
        op.last_read_at = Some(snapshot.fetched_at);
        op.read_revision = Some(snapshot.revision);
        op.phase = if op.observed == Some(op.enabled) {
            SettingsPhase::Confirmed
        } else {
            SettingsPhase::Uncertain
        };
        self.relay
            .session
            .with_token(token, |owner| self.save_original(owner, &op))?;
        Ok(op)
    }
    pub async fn reconcile_for(
        &self,
        token: &SessionToken,
        id: &str,
    ) -> Result<RelaySettingIntent> {
        let (_owner, _lock) = self.relay.lock(token)?;
        let op = self.operation(token, id)?;
        if !op.phase.unresolved() {
            return Ok(op);
        }
        self.readback(token, op).await
    }
    pub async fn accept_current_for(
        &self,
        token: &SessionToken,
        id: &str,
        revision: &str,
    ) -> Result<RelaySettingIntent> {
        validate(id, revision)?;
        let (_owner, _lock) = self.relay.lock(token)?;
        let mut op = self.operation(token, id)?;
        if !op.phase.unresolved() {
            return Ok(op);
        }
        if op.last_read_at.is_none() || op.read_revision.as_deref() != Some(revision) {
            return Err(Error::State("read back relay setting before acceptance"));
        }
        let snapshot = self.read(token).await?;
        if snapshot.revision != revision || snapshot.device_id != op.device_id {
            return Err(Error::State("relay setting changed"));
        }
        op.phase = SettingsPhase::AcceptedCurrentState;
        op.last_read_at = Some(snapshot.fetched_at);
        op.observed = snapshot.enabled;
        self.relay
            .session
            .with_token(token, |owner| self.save_original(owner, &op))?;
        Ok(op)
    }
}
