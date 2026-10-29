use cumulus_core::{Error, Result, client::mini::MiniSnapshot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Opening,
    Active,
}
pub struct Window {
    pub generation: String,
    pub label: String,
    pub phase: Phase,
    pub loaded: bool,
    pub ticket: u64,
    pub snapshot: Option<MiniSnapshot>,
}
#[derive(Default)]
pub struct Ledger {
    next: u64,
    pub window: Option<Window>,
}
impl Ledger {
    pub fn begin(&mut self) -> Result<(String, String)> {
        if self.window.is_some() {
            return Err(Error::MutationBusy);
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or(Error::State("mini lifetime exhausted"))?;
        let generation = self.next.to_string();
        let label = format!("mini-player-{generation}");
        self.window = Some(Window {
            generation: generation.clone(),
            label: label.clone(),
            phase: Phase::Opening,
            loaded: false,
            ticket: 0,
            snapshot: None,
        });
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
    pub fn observe(&mut self, generation: &str, value: MiniSnapshot) -> Result<String> {
        let window = self.get_mut(generation)?;
        if window
            .snapshot
            .as_ref()
            .is_none_or(|old| !old.same_controls(&value))
        {
            window.ticket = window
                .ticket
                .checked_add(1)
                .ok_or(Error::State("mini controls exhausted"))?;
        }
        window.snapshot = Some(value);
        Ok(window.ticket.to_string())
    }
    pub fn captured(&self, generation: &str, ticket: &str) -> Result<MiniSnapshot> {
        let window = self.get(generation)?;
        if window.phase != Phase::Active || window.ticket.to_string() != ticket {
            return Err(Error::StaleResponse);
        }
        window.snapshot.clone().ok_or(Error::StaleResponse)
    }
    pub fn ready(&mut self, generation: &str) -> Result<()> {
        let window = self.get_mut(generation)?;
        if window.phase != Phase::Opening || window.snapshot.is_none() {
            return Err(Error::StaleResponse);
        }
        window.phase = Phase::Active;
        window.loaded = true;
        Ok(())
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
    fn lifetime_does_not_reuse_old_ready_timeout_or_command() {
        let mut ledger = Ledger::default();
        let (a, label_a) = ledger.begin().unwrap();
        assert!(ledger.begin().is_err());
        assert!(ledger.ready(&a).is_err()); // no successful read/paint yet
        assert!(ledger.captured(&a, "0").is_err());
        ledger.take(Some(&a)).unwrap();
        let (b, label_b) = ledger.begin().unwrap();
        assert_ne!(a, b);
        assert_ne!(label_a, label_b);
        assert!(ledger.take(Some(&a)).is_none());
        assert!(ledger.ready(&a).is_err());
        assert!(ledger.captured(&a, "0").is_err());
        assert_eq!(ledger.get(&b).unwrap().phase, Phase::Opening);
    }
}
