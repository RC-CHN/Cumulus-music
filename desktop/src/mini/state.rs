use cumulus_core::{Error, Result, client::mini::MiniSnapshot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Opening,
    Active,
}
pub struct Window {
    pub open_epoch: String,
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Intent {
    serial: u64,
    generation: Option<String>,
}
impl Intent {
    pub fn is_mini(&self) -> bool {
        self.generation.is_some()
    }
}
impl Ledger {
    pub fn intent(&self) -> Intent {
        Intent {
            serial: self.next,
            generation: self.window.as_ref().map(|w| w.generation.clone()),
        }
    }
    pub fn check_intent(&self, intent: &Intent) -> Result<()> {
        if &self.intent() == intent {
            Ok(())
        } else {
            Err(Error::StaleResponse)
        }
    }
    pub fn begin(&mut self, epoch: String) -> Result<(String, String)> {
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
            open_epoch: epoch,
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
        let (a, label_a) = ledger.begin("1".into()).unwrap();
        assert!(ledger.begin("1".into()).is_err());
        assert!(ledger.ready(&a).is_err()); // no successful read/paint yet
        assert!(ledger.captured(&a, "0").is_err());
        ledger.take(Some(&a)).unwrap();
        let (b, label_b) = ledger.begin("1".into()).unwrap();
        assert_ne!(a, b);
        assert_ne!(label_a, label_b);
        assert!(ledger.take(Some(&a)).is_none());
        assert!(ledger.ready(&a).is_err());
        assert!(ledger.captured(&a, "0").is_err());
        assert_eq!(ledger.get(&b).unwrap().phase, Phase::Opening);
    }
    #[test]
    fn captured_mode_never_targets_a_later_window_or_reopens_after_return() {
        let mut ledger = Ledger::default();
        let empty = ledger.intent();
        ledger.check_intent(&empty).unwrap();
        let (first, _) = ledger.begin("1".into()).unwrap();
        assert!(ledger.check_intent(&empty).is_err());
        assert_eq!(ledger.get(&first).unwrap().open_epoch, "1");
        let close_first = ledger.intent();
        assert!(close_first.is_mini());
        ledger.take(Some(&first));
        assert!(ledger.check_intent(&close_first).is_err());
        assert!(ledger.check_intent(&empty).is_err());
        let (second, _) = ledger.begin("2".into()).unwrap();
        assert_ne!(first, second);
        assert!(ledger.check_intent(&close_first).is_err());
        ledger.check_intent(&ledger.intent()).unwrap();
    }
}
