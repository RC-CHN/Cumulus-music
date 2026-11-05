//! Scoped native entry for the proved M/308 miniBar transport branch.
use super::{PlaybackCommand, PlaybackController, PlaybackFence};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::{
        Step,
        engine::Cancellation,
        preflight::OrdinaryQueueEnvironment,
        skip_policy::{
            self, OrdinarySkipDecision as Decision, OrdinarySkipOutcome, OrdinarySkipTrigger,
            SkipBoundary,
        },
    },
    protocol::pc_experiments::PcExperimentGroups,
};

impl PlaybackController {
    /// Main bottom bar only; standard Next/Previous/EOF paths do not call
    /// this. The caller must own a verified PC experiment snapshot and
    /// connectivity observation. No renderer API supplies either input.
    ///
    /// Unchanged means this bounded branch did not consume the intent.
    /// The caller must retain its original scoped transport handling.
    pub async fn ordinary_minibar_skip(
        &self,
        token: &SessionToken,
        expected: PlaybackFence,
        step: Step,
        groups: &PcExperimentGroups,
        environment: OrdinaryQueueEnvironment,
        authority: Cancellation,
    ) -> Result<OrdinarySkipOutcome> {
        fn unchanged(boundary: SkipBoundary) -> OrdinarySkipOutcome {
            OrdinarySkipOutcome {
                decision: Decision::Unchanged(boundary),
                applied: None,
            }
        }
        authority.check()?;
        let (before, queue) = self.queue_with_state(token).await?;
        if before.fence() != expected {
            return Err(Error::StaleResponse);
        }
        if matches!(environment, OrdinaryQueueEnvironment::Unresolved)
            || groups.group("PH-PC-Blacklist") == Some("t1")
            || before.fm
            || before.intelligence.is_some()
            || before.play_id.is_none()
            || queue.items.iter().any(|row| {
                // M23.Z can make a denied song playable via M21.D. The
                // native ordinary audio scan must not substitute that MV.
                let alternative = &row.resource["song"]["noCopyrightRcmd"];
                alternative["type"].as_i64() == Some(3)
                    && !alternative["mvInfo"].is_null()
                    && alternative["expInfo"]["mvPlay"] == true
            })
            || !before.state.permits_minibar_step()
        {
            return Ok(unchanged(SkipBoundary::ContextUnresolved));
        }
        let trigger = OrdinarySkipTrigger::MiniBar(step);
        let item = match skip_policy::initial_row(&queue, trigger) {
            Ok(item) => item,
            Err(boundary) => return Ok(unchanged(boundary)),
        };
        if !self
            .resolver
            .confirmed_plain_skip_denial(token, item)
            .await?
        {
            return Ok(unchanged(SkipBoundary::InitialRightsNotProven));
        }
        authority.check()?;
        let scan = if groups.skip_songs_assigned() {
            Some(
                self.resolver
                    .ordinary_queue_preflight(token, &queue, environment)
                    .await?,
            )
        } else {
            None
        };
        let (after, current) = self.queue_with_state(token).await?;
        if after.fence() != expected || current != queue {
            return Err(Error::StaleResponse);
        }
        self.resolver.check_account_generation(token)?;
        authority.check()?;
        let Some(scan) = scan else {
            // M308 checks the initial row in both cohorts. With a definite
            // denial outside t1 it returns before dispatching a player
            // action: consume this click and retain the exact current play.
            return Ok(OrdinarySkipOutcome {
                decision: Decision::KeepCurrentDenied {
                    attempted: item.entry_id.clone(),
                },
                applied: Some(after),
            });
        };
        let decision = skip_policy::after_initial_denial(&queue, &scan, trigger)?;
        let command = match &decision {
            Decision::PlayExisting { target, .. } => Some(PlaybackCommand::Select(target.clone())),
            Decision::PauseCurrent { .. } => Some(PlaybackCommand::Pause(true)),
            Decision::StopAtErrorLimit { .. } => Some(PlaybackCommand::Stop),
            Decision::Unchanged(_) => None,
            Decision::KeepCurrentDenied { .. } => {
                unreachable!("known control denials do not enter the traversal")
            }
            Decision::StopAtDeniedTail { .. } => {
                unreachable!("only native EOF can stop at denied tail")
            }
        };
        let applied = match command {
            Some(command) => Some(
                self.command_guarded(token, command, expected, authority)
                    .await?,
            ),
            None => None,
        };
        Ok(OrdinarySkipOutcome { decision, applied })
    }
}
