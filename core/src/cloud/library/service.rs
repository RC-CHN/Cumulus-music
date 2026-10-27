use super::*;
pub(super) const CACHE: &str = "cloud_library_v1";
pub(super) const OPERATIONS: &str = "cloud_deletions_v1";
#[derive(Clone)]
pub struct CloudLibrary {
    pub(super) session: Arc<AccountSession>,
    pub(super) token: SessionToken,
    pub(super) store: Arc<StateStore>,
    pub(super) owner: String,
}
impl CloudLibrary {
    pub fn new(session: Arc<AccountSession>) -> Result<Self> {
        let token = session.token()?;
        let owner = session.with_token(&token, |uid| Ok(uid.to_owned()))?;
        Ok(Self {
            store: session.persistent_store()?,
            session,
            token,
            owner,
        })
    }
    pub(super) fn guard(&self) -> Result<()> {
        self.session.with_token(&self.token, |_| Ok(()))
    }
    pub(super) fn lock(&self) -> Result<OperationLock> {
        self.guard()?;
        OperationLock::acquire(self.store.directory(), "cloud-library", &self.owner)
    }
    pub fn cached(&self) -> Result<Option<CloudSnapshot>> {
        self.session.with_token(&self.token, |_| {
            let value = self
                .store
                .get(&self.owner, CACHE, "snapshot")?
                .map(serde_json::from_value::<CloudSnapshot>)
                .transpose()?;
            if let Some(value) = &value {
                value.validate()?;
            }
            Ok(value)
        })
    }
    pub async fn refresh(&self) -> Result<CloudSnapshot> {
        let _lock = self.lock()?;
        self.fetch().await
    }
    pub(super) async fn fetch(&self) -> Result<CloudSnapshot> {
        let value = self
            .session
            .run_for(&self.token, |api, _| async move {
                CloudApi::new(api).snapshot().await
            })
            .await?;
        value.validate()?;
        self.session.with_token(&self.token, |_| {
            self.store.put(
                &self.owner,
                CACHE,
                "snapshot",
                &serde_json::to_value(&value)?,
            )
        })?;
        Ok(value)
    }
    pub fn operations(&self) -> Result<Vec<DeleteOperation>> {
        self.session
            .with_token(&self.token, |_| self.store.values(&self.owner, OPERATIONS))?
            .into_values()
            .map(|v| {
                let op: DeleteOperation = serde_json::from_value(v)?;
                op.validate()?;
                Ok(op)
            })
            .collect()
    }
    pub fn operation(&self, id: &str) -> Result<DeleteOperation> {
        types::valid_id(id)?;
        let op: DeleteOperation = self.session.with_token(&self.token, |_| {
            Ok(serde_json::from_value(
                self.store
                    .get(&self.owner, OPERATIONS, id)?
                    .ok_or(Error::State("cloud operation not found"))?,
            )?)
        })?;
        op.validate()?;
        if op.id != id {
            return Err(Error::State("cloud deletion identity mismatch"));
        }
        Ok(op)
    }
    pub(super) fn save(&self, op: &DeleteOperation) -> Result<()> {
        op.validate()?;
        self.session.with_token(&self.token, |_| {
            self.store
                .put(&self.owner, OPERATIONS, &op.id, &serde_json::to_value(op)?)
        })
    }
}
