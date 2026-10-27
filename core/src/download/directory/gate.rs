use crate::{
    Error, Result,
    storage::{OperationLock, StateStore},
};
pub(in crate::download) const SETTINGS: &str = "download_directory_v1";
pub(super) const BATCHES: &str = "download_migrations_v1";
pub(in crate::download) fn active(store: &StateStore, owner: &str) -> Result<Option<String>> {
    store
        .get(owner, SETTINGS, "active")?
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}
pub(in crate::download) fn require(
    store: &StateStore,
    owner: &str,
    expected: Option<&str>,
) -> Result<()> {
    if active(store, owner)?.as_deref() != expected {
        return Err(Error::State(
            "download directory migration owns these tasks",
        ));
    }
    Ok(())
}
pub(in crate::download) fn lock(store: &StateStore, owner: &str) -> Result<OperationLock> {
    OperationLock::acquire(store.directory(), "download-directory", owner)
}
pub(in crate::download) fn require_transfer(
    store: &StateStore,
    owner: &str,
    id: &str,
) -> Result<()> {
    if store
        .get(owner, crate::download::service::TASKS, id)?
        .is_some()
    {
        require(store, owner, None)?;
    }
    Ok(())
}
