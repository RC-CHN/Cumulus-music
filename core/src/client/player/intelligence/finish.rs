use super::*;
use crate::{
    catalog::intelligence::IntelligenceApi,
    playback::{
        PlaybackQueue,
        intelligence::{IntelligenceFailure, IntelligenceSession},
    },
};

impl MusicClient {
    async fn intelligence_pending(&self, flight: &Flight) -> Result<IntelligenceSession> {
        flight.check(&self.session)?;
        let state = self
            .player
            .controller()
            .await?
            .command(&flight.token, PlaybackCommand::Snapshot)
            .await?;
        let queue = self.session.with_token(&flight.token, |uid| {
            PlaybackQueue::new(Some(self.registry.store()), uid)
        })?;
        let session = queue
            .intelligence_session()
            .ok_or(Error::StaleResponse)?
            .clone();
        if state.intelligence.as_ref() != Some(&session.context)
            || session.context.phase != IntelligencePhase::Seeded
        {
            return Err(Error::StaleResponse);
        }
        Ok(session)
    }

    pub(super) async fn intelligence_finish(&self, flight: &Flight) -> Result<PlayerView> {
        let session = self.intelligence_pending(flight).await?;
        let mut metadata = session.liked_metadata.clone();
        // Legacy seeded records predate the durable metadata cache. Re-read
        // catalogue only as lookup; never replace original count or backup.
        if metadata.is_empty() {
            metadata = self
                .intelligence_catalog(flight, &session.request.playlist_id, None, None)
                .await?
                .0
                .into_iter()
                .flatten()
                .collect();
        }
        let response = flight
            .wait(
                &self.session,
                self.session.run_for(&flight.token, |api, _| async {
                    IntelligenceApi::new(api)
                        .recommend(&session.request, &metadata)
                        .await
                }),
            )
            .await;
        if response.as_ref().is_err_and(|e| e.is_service_code(301)) {
            return Err(response.err().unwrap());
        }
        flight.check(&self.session)?;
        let outcome = match response {
            Ok(batch) => match self
                .intelligence_filter(flight, batch.entries.into_iter().map(|e| e.song).collect())
                .await
            {
                Ok(resources) => Ok((resources, batch.recommendations)),
                Err(error) => {
                    if error.is_service_code(301) {
                        return Err(error);
                    }
                    flight.check(&self.session)?;
                    Err(IntelligenceFailure::InvalidRecommendations)
                }
            },
            Err(_) => Err(IntelligenceFailure::RequestFailed),
        };
        // Only retry the local fenced commit. Never replay HTTP or take audio
        // ownership from pause/seek/stop while recommendations were in flight.
        let controller = self.player.controller().await?;
        for _ in 0..4 {
            flight.check(&self.session)?;
            let state = controller
                .command(&flight.token, PlaybackCommand::Snapshot)
                .await?;
            if state.intelligence.as_ref() != Some(&session.context) {
                return Err(Error::StaleResponse);
            }
            let action = match &outcome {
                Ok((resources, recommendations)) => IntelligenceAction::Complete {
                    session_id: session.context.id.clone(),
                    resources: resources.clone(),
                    recommendations: recommendations.clone(),
                },
                Err(failure) => IntelligenceAction::Fail {
                    session_id: session.context.id.clone(),
                    failure: *failure,
                },
            };
            match controller
                .command_guarded(
                    &flight.token,
                    PlaybackCommand::Intelligence(IntelligenceUpdate {
                        expected: state.fence(),
                        action,
                    }),
                    state.fence(),
                    flight.native_cancel(),
                )
                .await
            {
                Ok(value) => {
                    flight.check(&self.session)?;
                    return project(&value, &flight.token);
                }
                Err(Error::StaleResponse | Error::QueueChanged) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(Error::QueueChanged)
    }
}
