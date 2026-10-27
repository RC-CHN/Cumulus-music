use crate::{Error, Result, podcast::Episode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub fn scene_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::InvalidInput("invalid scene FM identifier"));
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SceneFmCategory {
    pub id: String,
    pub name: String,
    pub label: String,
    pub algorithm: String,
    pub voices: Vec<Episode>,
    pub selected: Option<String>,
}
impl SceneFmCategory {
    pub fn choose_unheard(&mut self, heard: &BTreeSet<String>) {
        self.selected = self
            .voices
            .iter()
            .find(|v| !heard.contains(&v.id))
            .or(self.voices.first())
            .map(|v| v.id.clone());
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SceneFmSnapshot {
    pub revision: u64,
    pub active_id: Option<String>,
    pub scenes: Vec<SceneFmCategory>,
    pub pending: bool,
}
impl SceneFmSnapshot {
    pub fn active(&self) -> Option<&SceneFmCategory> {
        self.scenes
            .iter()
            .find(|s| Some(&s.id) == self.active_id.as_ref())
    }
    pub fn selected(&self) -> Option<&Episode> {
        let s = self.active()?;
        s.voices.iter().find(|v| Some(&v.id) == s.selected.as_ref())
    }
    pub(crate) fn bump(&mut self) -> Result<()> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(Error::State("scene FM revision exhausted"))?;
        Ok(())
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.scenes.len() > 200 {
            return Err(Error::State("too many scene FM categories"));
        }
        let mut seen = BTreeSet::new();
        for s in &self.scenes {
            scene_id(&s.id)?;
            if !seen.insert(&s.id) || s.voices.len() > 5000 {
                return Err(Error::State("invalid scene FM categories"));
            }
            let mut voices = BTreeSet::new();
            for v in &s.voices {
                crate::protocol::api::identifier(&serde_json::json!(v.id))?;
                if !voices.insert(&v.id) {
                    return Err(Error::State("duplicate scene FM voice"));
                }
            }
            if s.selected.as_ref().is_some_and(|id| !voices.contains(id)) {
                return Err(Error::State("scene FM selection missing"));
            }
        }
        if self.active_id.is_some() && self.active().is_none() {
            return Err(Error::State("scene FM active category missing"));
        }
        Ok(())
    }
}
