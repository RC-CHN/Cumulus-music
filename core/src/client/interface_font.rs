use super::MusicClient;
use crate::{Result, preferences::interface_font::InterfaceFont};

impl MusicClient {
    /// This presentation preference belongs to the installation, including while signed out.
    pub fn interface_font(&self) -> Result<InterfaceFont> {
        InterfaceFont::read(&self.registry.store())
    }

    pub fn set_interface_font(&self, family: Option<String>) -> Result<InterfaceFont> {
        InterfaceFont::replace(&self.registry.store(), family)
    }
}
