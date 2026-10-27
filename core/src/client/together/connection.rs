use super::*;
use crate::listen_together::transport::{
    ConnectionState, ConnectionView, ImFactory, NativeFactory,
};
use std::path::PathBuf;
impl MusicClient {
    /// Trusted native host configuration; no frontend path/key transport.
    pub fn with_native_im(self, library_directory: PathBuf) -> Self {
        let factory = Arc::new(NativeFactory::new(library_directory, self.registry.store()));
        self.with_together_im_factory(factory)
    }
    /// Non-Tauri native hosts and deterministic transport lifecycle tests.
    pub fn with_together_im_factory(mut self, factory: Arc<dyn ImFactory>) -> Self {
        self.together.factory = Some(factory);
        self
    }
    pub(super) fn together_connection(&self, token: &SessionToken) -> Result<ConnectionView> {
        if *self
            .together
            .cleanup_uncertain
            .lock()
            .map_err(|_| Error::LockPoisoned)?
        {
            return Ok(ConnectionView {
                state: ConnectionState::Failed,
                message: Some("消息连接清理失败，请重新启动客户端".into()),
                ..Default::default()
            });
        }
        let runtime = self
            .together
            .runtime
            .lock()
            .map_err(|_| Error::LockPoisoned)?;
        if let Some(runtime) = runtime.as_ref().filter(|r| &r.token == token)
            && let Some(im) = &runtime.im
        {
            return Ok(im.view());
        }
        Ok(ConnectionView {
            state: if self.together.factory.is_some() {
                ConnectionState::Idle
            } else {
                ConnectionState::Unavailable
            },
            ..Default::default()
        })
    }
}
