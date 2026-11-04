//! One local, account-fenced receipt for the official first-recommendation cue.
use super::state::Actor;
use crate::{
    Error, Result,
    account::SessionToken,
    playback::{controller::PlaybackStatus, queue_more::KIND},
};
use serde::Deserialize;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

const KEY: &str = "tip-seen";
const SHOW_FOR: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueueMoreTipAction {
    Begin { play_id: String },
    Seen { ticket: String },
}

pub(super) struct Request {
    pub token: SessionToken,
    pub action: QueueMoreTipAction,
    pub reply: oneshot::Sender<Result<Option<String>>>,
}

#[derive(Default)]
pub(super) struct Cue {
    owner: Option<SessionToken>,
    claimed: bool,
    offer: Option<(String, Instant)>,
}

impl Cue {
    fn bind(&mut self, token: &SessionToken) {
        if self.owner.as_ref() != Some(token) {
            *self = Self {
                owner: Some(token.clone()),
                ..Default::default()
            };
        }
    }

    fn available(&self, token: &SessionToken) -> bool {
        self.owner.as_ref() != Some(token) || !self.claimed
    }

    fn claim(&mut self, already_seen: bool, now: Instant) -> Option<String> {
        if self.claimed {
            return None;
        }
        self.claimed = true;
        if already_seen {
            return None;
        }
        let ticket = uuid::Uuid::new_v4().simple().to_string();
        self.offer = Some((ticket.clone(), now));
        Some(ticket)
    }

    fn ready(&self, ticket: &str, now: Instant) -> bool {
        self.offer.as_ref().is_some_and(|(id, start)| {
            id == ticket && now.saturating_duration_since(*start) >= SHOW_FOR
        })
    }
}

fn early_position(position: Option<f64>) -> bool {
    position.is_some_and(|seconds| seconds.is_finite() && seconds > 0.0 && seconds <= 30.0)
}

impl Actor {
    pub(super) fn queue_more_tip_eligible(&self) -> bool {
        self.queue_more.supported
            && self.token.uid.is_some()
            && self.queue_more_tip.available(&self.token)
            && self.loaded.is_some()
            && self.state != PlaybackStatus::Error
            && self.queue.current_is_active_recommendation()
    }

    pub(super) async fn queue_more_tip_request(&mut self, request: Request) {
        if request.reply.is_closed() {
            return;
        }
        let result = self
            .handle_queue_more_tip(&request.token, request.action)
            .await;
        let _ = request.reply.send(result);
    }

    async fn handle_queue_more_tip(
        &mut self,
        token: &SessionToken,
        action: QueueMoreTipAction,
    ) -> Result<Option<String>> {
        self.session.check_generation(token)?;
        self.rebind().await?;
        if &self.token != token {
            return Err(Error::StaleResponse);
        }
        self.queue_more_tip.bind(token);
        let now = Instant::now();
        match action {
            QueueMoreTipAction::Begin { play_id } => {
                // Read the latest committed rows without changing transport.
                self.queue.reload()?;
                if !self.queue_more_tip_eligible()
                    || self.play_id.as_deref() != Some(&play_id)
                    || !early_position(self.position)
                    || !self.queue.more_unattached()?
                    || self.loaded.as_ref().map(|l| l.item.entry_id.as_str())
                        != self.queue.current_id()
                {
                    return Ok(None);
                }
                let seen = self.session.with_token(token, |uid| {
                    match self.store.get(uid, KIND, KEY)? {
                        None | Some(Value::Bool(false)) => Ok(false),
                        Some(Value::Bool(true)) => Ok(true),
                        _ => Err(Error::State("invalid recommendation cue receipt")),
                    }
                })?;
                Ok(self.queue_more_tip.claim(seen, now))
            }
            QueueMoreTipAction::Seen { ticket } => {
                if !self.queue_more_tip.ready(&ticket, now) {
                    return Ok(None);
                }
                self.session.with_token(token, |uid| {
                    self.store.put(uid, KIND, KEY, &Value::Bool(true))
                })?;
                self.queue_more_tip.offer = None;
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_waits_for_five_seconds_and_never_carries_across_owner_epochs() {
        let a = SessionToken {
            uid: Some("1".into()),
            generation: 4,
        };
        let b = SessionToken {
            uid: Some("1".into()),
            generation: 5,
        };
        let now = Instant::now();
        let mut cue = Cue::default();
        cue.bind(&a);
        let offer = cue.claim(false, now).unwrap();
        assert!(!cue.available(&a));
        assert!(cue.claim(false, now).is_none());
        assert!(!cue.ready(&offer, now + Duration::from_millis(4999)));
        assert!(!cue.ready("unrelated", now + SHOW_FOR));
        assert!(cue.ready(&offer, now + SHOW_FOR));
        cue.bind(&b);
        assert!(!cue.ready(&offer, now + SHOW_FOR));
        assert!(cue.claim(true, now + SHOW_FOR).is_none());
        assert!(!cue.available(&b));
    }

    #[test]
    fn progress_window_is_elapsed_and_inclusive_at_thirty() {
        for value in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(30.001),
            Some(f64::INFINITY),
        ] {
            assert!(!early_position(value));
        }
        assert!(early_position(Some(0.01)));
        assert!(early_position(Some(30.0)));
    }
}
