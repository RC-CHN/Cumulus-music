use super::*;
use crate::{
    listen_together::native_im::{NativeIm, NimConfig},
    storage::StateStore,
};
use serde_json::json;
use std::path::PathBuf;
mod production;

pub struct NativeFactory {
    directory: PathBuf,
    store: Arc<StateStore>,
}
impl NativeFactory {
    /// This is a trusted native-host path, never supplied by an IPC/page field.
    pub fn new(directory: PathBuf, store: Arc<StateStore>) -> Self {
        Self { directory, store }
    }
}
impl ImFactory for NativeFactory {
    fn connect(
        &self,
        session: Arc<AccountSession>,
        token: SessionToken,
        credentials: ImCredentials,
    ) -> Result<Box<dyn ImConnection>> {
        let key = session.with_token(&token, |owner| {
            self.store.transaction(|tx| {
                if let Some(value) = tx.get(owner, "nim-native-secret-v1", "database-key")? {
                    return value
                        .as_str()
                        .filter(|s| s.len() == 32)
                        .map(str::to_owned)
                        .ok_or(Error::State("invalid stored IM database key"));
                }
                let mut entropy = [0u8; 16];
                getrandom::fill(&mut entropy).map_err(|_| Error::Entropy)?;
                let key = hex::encode(entropy);
                tx.put(owner, "nim-native-secret-v1", "database-key", &json!(key))?;
                Ok(key)
            })
        })?;
        Ok(Box::new(NativeIm::open(
            NimConfig {
                library_directory: self.directory.clone(),
                data_root: self.store.directory().join("nim"),
                app_key: production::APP_KEY.into(),
                database_key: key,
            },
            session,
            token,
            credentials,
        )?))
    }
}
impl ImConnection for NativeIm {
    fn enter(&mut self, room: &Room) -> Result<()> {
        NativeIm::enter(self, room)
    }
    fn next_event(&mut self, timeout: Duration) -> Result<Option<ImEvent>> {
        NativeIm::next_event(self, timeout)
    }
    fn close(&mut self) -> Result<()> {
        NativeIm::close(self)
    }
}
