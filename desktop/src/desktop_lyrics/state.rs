use super::*;
use cumulus_core::client::{desktop_lyrics::DesktopLyricDocument, mini::MiniSnapshot};
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentState {
    #[default]
    Idle,
    Loading,
    Ready,
    Failed,
}
#[derive(Default)]
pub struct Content {
    pub key: Option<(String, String)>,
    pub attempt: u64,
    pub document: Option<DesktopLyricDocument>,
    pub state: ContentState,
}
pub struct Window {
    pub generation: String,
    pub label: String,
    pub ready: bool,
    pub loaded: bool,
    pub locked: bool,
    pub ticket: u64,
    pub media: Option<MiniSnapshot>,
    pub content: Arc<tokio::sync::Mutex<Content>>,
    pub fetch: Option<tokio::task::AbortHandle>,
    pub geometry_revision: u64,
}
impl Drop for Window {
    fn drop(&mut self) {
        if let Some(fetch) = self.fetch.take() {
            fetch.abort();
        }
    }
}
#[derive(Default)]
pub struct Ledger {
    pub serial: u64,
    pub window: Option<Window>,
    pub error: Option<&'static str>,
}
impl Ledger {
    pub fn begin(&mut self) -> Result<(String, String)> {
        if self.window.is_some() {
            return Err(Error::MutationBusy);
        }
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or(Error::State("lyric lifetime exhausted"))?;
        let generation = self.serial.to_string();
        let label = format!("desktop-lyrics-{generation}");
        self.window = Some(Window {
            generation: generation.clone(),
            label: label.clone(),
            ready: false,
            loaded: false,
            locked: false,
            ticket: 0,
            media: None,
            content: Arc::new(tokio::sync::Mutex::new(Content::default())),
            fetch: None,
            geometry_revision: 0,
        });
        self.error = None;
        Ok((generation, label))
    }
    pub fn get(&self, generation: &str) -> Result<&Window> {
        self.window
            .as_ref()
            .filter(|w| w.generation == generation)
            .ok_or(Error::StaleResponse)
    }
    pub fn get_mut(&mut self, generation: &str) -> Result<&mut Window> {
        self.window
            .as_mut()
            .filter(|w| w.generation == generation)
            .ok_or(Error::StaleResponse)
    }
    pub fn observe(&mut self, generation: &str, media: MiniSnapshot) -> Result<String> {
        let w = self.get_mut(generation)?;
        if w.media
            .as_ref()
            .is_none_or(|old| !old.same_controls(&media))
        {
            w.ticket = w
                .ticket
                .checked_add(1)
                .ok_or(Error::State("lyric controls exhausted"))?;
        }
        w.media = Some(media);
        Ok(w.ticket.to_string())
    }
    pub fn captured(&self, generation: &str, ticket: &str) -> Result<MiniSnapshot> {
        let w = self.get(generation)?;
        if !w.ready || w.locked || w.ticket.to_string() != ticket {
            return Err(Error::StaleResponse);
        }
        w.media.clone().ok_or(Error::StaleResponse)
    }
    pub fn take(&mut self, generation: Option<&str>) -> Option<Window> {
        if generation.is_some_and(|g| self.get(g).is_err()) {
            return None;
        }
        self.window.take()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_window_cannot_close_or_control_its_replacement() {
        let mut state = Ledger::default();
        let (first, _) = state.begin().unwrap();
        assert!(state.begin().is_err());
        assert!(state.captured(&first, "0").is_err());
        state.take(Some(&first)).unwrap();
        let (next, _) = state.begin().unwrap();
        assert_ne!(first, next);
        assert!(state.take(Some(&first)).is_none());
        assert!(state.get(&first).is_err());
        assert!(!state.get(&next).unwrap().locked);
    }
}
