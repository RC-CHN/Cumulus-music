//! Composed, account-owned local/remote removal. Renderer inputs contain only
//! resource/plan identities; paths and file facts stay in the native journal.
mod execute;
mod files;
mod preview;
mod types;
mod view;
use super::*;
use crate::{
    cloud::library::{DeleteOperation, DeletePhase},
    storage::{OperationLock, StateStore},
};
use std::sync::Arc;
pub use types::{CloudRemovalFileState, CloudRemovalFileView, CloudRemovalPhase, CloudRemovalView};
use types::{FileRecord, Issue, Origin, Plan};
const PLANS: &str = "cloud_removal_plans_v1";

fn valid_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidInput("invalid cloud removal identity"));
    }
    Ok(())
}
fn checked_ids(ids: &[String]) -> Result<()> {
    if ids.is_empty() || ids.len() > 5000 {
        return Err(Error::InvalidInput("invalid cloud removal selection"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        crate::protocol::api::identifier(&serde_json::json!(id))?;
        if !seen.insert(id) {
            return Err(Error::InvalidInput("duplicate cloud removal selection"));
        }
    }
    Ok(())
}
fn store(context: &Context) -> Result<Arc<StateStore>> {
    context.session.persistent_store()
}
fn owner(context: &Context) -> Result<String> {
    context
        .session
        .with_token(&context.token, |uid| Ok(uid.to_owned()))
}
fn lock(context: &Context, id: &str) -> Result<OperationLock> {
    valid_id(id)?;
    OperationLock::acquire(
        store(context)?.directory(),
        "cloud-removal",
        &serde_json::to_string(&(owner(context)?, id))?,
    )
}
fn load(context: &Context, id: &str) -> Result<Option<Plan>> {
    valid_id(id)?;
    let s = store(context)?;
    context
        .session
        .with_token(&context.token, |uid| s.get(uid, PLANS, id))?
        .map(|v| {
            let p: Plan = serde_json::from_value(v)?;
            p.validate()?;
            if p.id != id {
                return Err(Error::State("cloud removal identity mismatch"));
            }
            Ok(p)
        })
        .transpose()
}
fn save(context: &Context, plan: &Plan) -> Result<()> {
    plan.validate()?;
    let s = store(context)?;
    context.session.with_token(&context.token, |uid| {
        s.put(uid, PLANS, &plan.id, &serde_json::to_value(plan)?)
    })
}
fn now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::State("clock before epoch"))?
        .as_millis()
        .try_into()
        .map_err(|_| Error::State("clock overflow"))
}

impl MusicClient {
    pub async fn cloud_delete_view(
        &self,
        epoch: &str,
        id: &str,
        reconcile: bool,
    ) -> Result<CloudRemovalView> {
        let context = self.cloud.inner.context(epoch).await?;
        let held = match lock(&context, id) {
            Ok(held) => Some(held),
            Err(Error::MutationBusy) => None,
            Err(e) => return Err(e),
        };
        let running = held.is_none();
        let mut remote = context.library.find_operation(id)?;
        if reconcile
            && !running
            && remote
                .as_ref()
                .is_some_and(|o| o.phase.pending() && o.phase != DeletePhase::Preparing)
        {
            remote = Some(context.library.reconcile_delete(id).await?);
        }
        match load(&context, id)? {
            Some(plan) => view::project(&context, &plan, remote, running),
            None => view::legacy(
                &context,
                remote.ok_or(Error::State("cloud removal plan not found"))?,
                running,
            ),
        }
    }
    pub async fn cloud_delete_discard(&self, epoch: &str, id: &str) -> Result<()> {
        let context = self.cloud.inner.context(epoch).await?;
        let _lock = lock(&context, id)?;
        if let Some(plan) = load(&context, id)?
            && plan.choice.is_none()
            && context.library.find_operation(id)?.is_none()
        {
            let s = store(&context)?;
            context
                .session
                .with_token(&context.token, |uid| s.delete(uid, PLANS, id))?;
        }
        context.guard()
    }
}
