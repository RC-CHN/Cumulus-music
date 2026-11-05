//! Scoped native entry for the proved M/308 miniBar transport branch.
use super::{
    PlaybackCommand, PlaybackController, PlaybackFence, PlaybackSnapshot, VerifiedOrdinarySkip,
};
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

struct Scope<'a> {
    groups: &'a PcExperimentGroups,
    environment: OrdinaryQueueEnvironment,
    authority: Cancellation,
    publish: bool,
    ended: bool,
}

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
        self.ordinary_minibar_skip_inner(
            token,
            expected,
            step,
            Scope {
                groups,
                environment,
                authority,
                publish: false,
                ended: false,
            },
        )
        .await
    }

    /// Only the product host's verified PC observation may add the notice.
    pub(crate) async fn verified_minibar_skip(
        &self,
        token: &SessionToken,
        expected: PlaybackFence,
        step: Step,
        verified: VerifiedOrdinarySkip,
        ended: bool,
    ) -> Result<OrdinarySkipOutcome> {
        self.ordinary_minibar_skip_inner(
            token,
            expected,
            step,
            Scope {
                groups: &verified.groups,
                environment: OrdinaryQueueEnvironment::OnlineWithoutMvExperiment {
                    app_online: true,
                },
                authority: verified.authority,
                publish: true,
                ended,
            },
        )
        .await
    }

    async fn ordinary_minibar_skip_inner(
        &self,
        token: &SessionToken,
        expected: PlaybackFence,
        step: Step,
        scope: Scope<'_>,
    ) -> Result<OrdinarySkipOutcome> {
        let Scope {
            groups,
            environment,
            authority,
            publish,
            ended,
        } = scope;
        fn unchanged(
            boundary: SkipBoundary,
            preserved: Option<PlaybackSnapshot>,
        ) -> OrdinarySkipOutcome {
            OrdinarySkipOutcome {
                decision: Decision::Unchanged(boundary),
                applied: preserved,
            }
        }
        authority.check()?;
        let (before, queue) = self.queue_with_state(token).await?;
        if before.fence() != expected {
            return Err(Error::StaleResponse);
        }
        let source_matches = if ended {
            before.ended_minibar_receipt.is_some()
        } else {
            before.play_id.is_some()
        };
        if matches!(environment, OrdinaryQueueEnvironment::Unresolved)
            || groups.group("PH-PC-Blacklist") == Some("t1")
            || before.fm
            || before.intelligence.is_some()
            || !source_matches
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
            return Ok(unchanged(
                SkipBoundary::ContextUnresolved,
                ended.then_some(before),
            ));
        }
        let trigger = OrdinarySkipTrigger::MiniBar(step);
        let item = match skip_policy::initial_row(&queue, trigger) {
            Ok(item) => item,
            Err(boundary) => return Ok(unchanged(boundary, ended.then_some(before))),
        };
        if !self
            .resolver
            .confirmed_plain_skip_denial(token, item)
            .await?
        {
            return Ok(unchanged(SkipBoundary::InitialRightsNotProven, None));
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
        if ended
            && matches!(
                decision,
                Decision::PauseCurrent { .. } | Decision::Unchanged(_)
            )
        {
            // There is no active decoder to pause. Once plain denial was
            // proved, an incomplete or unsupported scan must not start that
            // row through the generic fallback either.
            return Ok(OrdinarySkipOutcome {
                decision,
                applied: Some(after),
            });
        }
        if publish && let Decision::PlayExisting { target, .. } = &decision {
            let applied = self
                .select_verified_skip(token, target.clone(), expected, authority)
                .await?;
            return Ok(OrdinarySkipOutcome {
                decision,
                applied: Some(applied),
            });
        }
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
