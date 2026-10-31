use super::*;
use crate::playback::intelligence::{self as domain, IntelligenceAction, IntelligenceSession};

impl QueueSnapshot {
    pub(super) fn validate_intelligence_identity(&self) -> Result<()> {
        if let Some(context) = &self.intelligence {
            context.validate()?;
            if self.fm_source.is_some()
                || self.items.is_empty()
                || self.current.is_none()
                || self.items.iter().any(|e| e.source["kind"] == "fm")
                || (context.active() && self.mode != PlaybackMode::OrderLoop)
            {
                return Err(Error::State("invalid intelligence queue ownership"));
            }
        }
        Ok(())
    }
}

pub(crate) struct IntelligenceApplied {
    pub preserve_media: bool,
    pub start_seed: bool,
}
pub(super) struct OwnedRead {
    pub queue: Option<Value>,
    pub session: Option<String>,
    pub stamp: Option<(i64, i64)>,
}
impl PlaybackQueue {
    pub(super) fn read_owned(store: Option<&StateStore>, owner: &str) -> Result<OwnedRead> {
        match store {
            Some(store) => store.transaction(|tx| {
                Ok(OwnedRead {
                    queue: tx.get(owner, "playback", "queue")?,
                    session: tx.get_raw(owner, domain::KIND, domain::KEY)?,
                    stamp: Some(tx.change_stamp()?),
                })
            }),
            None => Ok(OwnedRead {
                queue: None,
                session: None,
                stamp: None,
            }),
        }
    }
    pub(super) fn load_intelligence(
        queue: &QueueSnapshot,
        raw: Option<&str>,
    ) -> Result<Option<Arc<IntelligenceSession>>> {
        // An orphan cannot revive playback. Keep its raw CAS token so the next
        // explicit ordinary mutation can remove it without touching other work.
        if queue.intelligence.is_none() {
            return Ok(None);
        }
        let session: IntelligenceSession =
            serde_json::from_str(raw.ok_or(Error::State("missing intelligence session"))?)?;
        session.validate(queue)?;
        Ok(Some(Arc::new(session)))
    }
    pub fn intelligence_session(&self) -> Option<&IntelligenceSession> {
        self.intelligence.as_deref()
    }
    pub fn intelligence_context(&self) -> Option<&domain::IntelligenceContext> {
        self.state.intelligence.as_ref()
    }
    pub(crate) fn intelligence_transition(
        &mut self,
        action: IntelligenceAction,
    ) -> Result<IntelligenceApplied> {
        let mut plan = domain::transition(&self.state, self.intelligence.as_deref(), action)?;
        if plan.randomize {
            plan.queue.order.shuffle(&mut self.rng);
        }
        self.commit_owned(plan.queue, true, None, plan.session.map(Arc::new))?;
        Ok(IntelligenceApplied {
            preserve_media: plan.preserve_media,
            start_seed: plan.start_seed,
        })
    }
}
