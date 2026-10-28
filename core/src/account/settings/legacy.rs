use super::{
    AccountSettings, SettingChange, SettingKey, SettingsMutation, SettingsPhase, service::JOURNAL,
};
use crate::{
    Error, Result,
    account::{
        SessionToken,
        resources::{MutationPhase, MutationRecord, ResourceKind},
    },
};

impl AccountSettings {
    /// Earlier native callers used one resource journal slot per setting.
    /// Move those records atomically into the durable intent journal. In-flight
    /// legacy processes must not share a writable profile across an upgrade.
    pub(super) fn migrate_legacy(&self, token: &SessionToken) -> Result<()> {
        self.session.with_token(token, |uid| {
            self.store.transaction(|tx| {
                for (key, raw) in tx.values(uid, "resource-mutations")? {
                    if !key.starts_with("settings:") {
                        continue;
                    }
                    let record: MutationRecord = serde_json::from_value(raw)?;
                    if record.kind != ResourceKind::Settings
                        || key != format!("settings:{}", record.key)
                    {
                        return Err(Error::State("invalid legacy settings journal"));
                    }
                    let setting = SettingKey::parse(&record.key)?;
                    let desired = setting
                        .value(&record.desired)
                        .ok_or(Error::State("invalid legacy settings value"))?;
                    if !record.created_at.is_finite() || record.created_at < 0.0 {
                        return Err(Error::State("invalid legacy settings time"));
                    }
                    let id = uuid::Uuid::new_v4().to_string();
                    let op = SettingsMutation {
                        id: id.clone(),
                        change: SettingChange {
                            key: setting,
                            value: desired,
                        },
                        expected_revision: String::new(),
                        before: setting.value(&record.before),
                        phase: match record.phase {
                            MutationPhase::Sending | MutationPhase::Verifying => {
                                SettingsPhase::Pending
                            }
                            MutationPhase::Uncertain => SettingsPhase::Uncertain,
                            MutationPhase::Confirmed => SettingsPhase::Confirmed,
                            MutationPhase::Rejected => SettingsPhase::Rejected,
                            MutationPhase::Unchanged => SettingsPhase::Unchanged,
                        },
                        created_at: (record.created_at * 1000.0) as u64,
                        // A fresh full read is required before accepting an old
                        // unresolved request; old per-field checks are insufficient.
                        last_read_at: None,
                        read_revision: None,
                        observed: record.observed.as_ref().and_then(|v| setting.value(v)),
                        service_code: record.service_code.and_then(|v| v.as_i64()),
                    };
                    tx.put(uid, JOURNAL, &id, &serde_json::to_value(op)?)?;
                    tx.delete(uid, "resource-mutations", &key)?;
                }
                Ok(())
            })
        })
    }
    pub(crate) fn compatibility_operations(
        &self,
        token: &SessionToken,
    ) -> Result<Vec<MutationRecord>> {
        // Do not block unrelated collection reads on an active settings write.
        let mut ops = self.operations(token)?;
        ops.sort_by_key(|op| op.created_at);
        ops.into_iter()
            .filter(|op| {
                !matches!(
                    op.phase,
                    SettingsPhase::Cancelled | SettingsPhase::AcceptedCurrentState
                )
            })
            .map(compatibility_record)
            .collect()
    }
    pub(crate) async fn reconcile_compatibility(
        &self,
        token: &SessionToken,
        key: &str,
    ) -> Result<MutationRecord> {
        let setting = SettingKey::parse(key)?;
        let op = self
            .mutations_for(token)?
            .into_iter()
            .filter(|op| op.change.key == setting)
            .max_by_key(|op| op.created_at)
            .ok_or(Error::State("no operation to reconcile"))?;
        let result = self.reconcile_for(token, &op.id).await?;
        if result.phase.unresolved() {
            return Err(Error::MutationUncertain);
        }
        compatibility_record(result)
    }
}
pub(crate) fn compatibility_record(op: SettingsMutation) -> Result<MutationRecord> {
    Ok(MutationRecord {
        kind: ResourceKind::Settings,
        key: op.change.key.key().into(),
        before: serde_json::to_value(op.before)?,
        desired: serde_json::to_value(op.change.value)?,
        created_at: op.created_at as f64 / 1000.0,
        phase: match op.phase {
            SettingsPhase::Pending => MutationPhase::Sending,
            SettingsPhase::Confirmed => MutationPhase::Confirmed,
            SettingsPhase::Rejected => MutationPhase::Rejected,
            SettingsPhase::Uncertain => MutationPhase::Uncertain,
            SettingsPhase::Unchanged => MutationPhase::Unchanged,
            SettingsPhase::AcceptedCurrentState | SettingsPhase::Cancelled => {
                return Err(Error::State(
                    "settings intent resolved without confirmation",
                ));
            }
        },
        observed: op.observed.map(serde_json::to_value).transpose()?,
        checked_at: op.last_read_at.map(|n| n as f64 / 1000.0),
        service_code: op.service_code.map(serde_json::Value::from),
        error_class: None,
    })
}
