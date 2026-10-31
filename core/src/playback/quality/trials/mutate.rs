use super::super::{QualityApi, QualityRight, TrialPhase};
use super::{QualityTrial, QualityTrialPhase, QualityTrials, TIMEOUT, TrialConsent};
use crate::{Error, Result, account::SessionToken};

impl QualityTrials {
    /// expected_remaining_ms comes from the offer reviewed by the user. It is
    /// checked again immediately before saving Sending and issuing auth/set.
    pub async fn start_for(
        &self,
        token: &SessionToken,
        right: QualityRight,
        expected_remaining_ms: Option<u64>,
        consent: TrialConsent,
    ) -> Result<QualityTrial> {
        if consent != TrialConsent::ExplicitStart {
            return Err(Error::State("quality trial requires explicit consent"));
        }
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        if let Some(record) = self.get(&owner, right)?
            && !record.phase.terminal()
        {
            return Err(Error::MutationUncertain);
        }
        let offer = self
            .query(token, right)
            .await?
            .filter(|r| r.phase() == TrialPhase::Eligible)
            .ok_or(Error::State("quality trial offer changed"))?;
        if offer.remaining_ms != expected_remaining_ms {
            return Err(Error::State("quality trial offer changed"));
        }
        let mut record = QualityTrial {
            right,
            operation: uuid::Uuid::new_v4().simple().to_string(),
            revision: 1,
            phase: QualityTrialPhase::Sending,
            acknowledged: false,
            observed: None,
            read_failed: false,
        };
        self.session
            .with_token(token, |_| self.save(&owner, &record))?;
        // Store a positive receipt in its original account before run_for's
        // post-I/O generation check. A cancelled future leaves Sending durable.
        let send = tokio::time::timeout(
            TIMEOUT,
            self.session.run_for(token, |api, _| async {
                let result = QualityApi::new(api).activate(right).await;
                match &result {
                    Ok(()) => {
                        record.acknowledged = true;
                        record.phase = QualityTrialPhase::Acknowledged;
                    }
                    Err(error) => {
                        record.phase = if [301, 400, 403, 405]
                            .iter()
                            .any(|c| error.is_service_code(*c))
                        {
                            QualityTrialPhase::Rejected
                        } else {
                            QualityTrialPhase::Uncertain
                        };
                    }
                }
                record.advance()?;
                self.save(&owner, &record)?;
                result
            }),
        )
        .await;
        self.session.check_generation(token)?;
        match send {
            Ok(Ok(())) => self.observe_locked(token, &owner, record).await,
            Ok(Err(error)) => {
                // A known rejection is distinct from a lost or false receipt.
                if record.phase == QualityTrialPhase::Rejected {
                    return Err(error);
                }
                if record.phase == QualityTrialPhase::Acknowledged {
                    // The server replied positively but saving that receipt
                    // failed. The durable Sending record remains unresolved;
                    // do not publish this in-memory record as a saved result.
                    return Err(Error::MutationUncertain);
                }
                if record.phase == QualityTrialPhase::Sending {
                    record.phase = QualityTrialPhase::Uncertain;
                    record.advance()?;
                    self.save(&owner, &record)?;
                }
                Ok(record)
            }
            Err(_) => {
                record.phase = QualityTrialPhase::Uncertain;
                record.advance()?;
                self.save(&owner, &record)?;
                Ok(record)
            }
        }
    }
    /// An already active official-client trial may be used without auth/set.
    /// This does not claim that this process started or acknowledged it.
    pub async fn adopt_for(
        &self,
        token: &SessionToken,
        right: QualityRight,
    ) -> Result<QualityTrial> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        if let Some(record) = self.get(&owner, right)?
            && !record.phase.terminal()
        {
            return self.observe_locked(token, &owner, record).await;
        }
        let observed = self
            .query(token, right)
            .await?
            .filter(|r| matches!(r.phase(), TrialPhase::Active | TrialPhase::Granted))
            .ok_or(Error::State("quality trial is not active"))?;
        let record = QualityTrial {
            right,
            operation: uuid::Uuid::new_v4().simple().to_string(),
            revision: 1,
            phase: QualityTrialPhase::Active,
            acknowledged: false,
            observed: Some(observed),
            read_failed: false,
        };
        self.session
            .with_token(token, |_| self.save(&owner, &record))?;
        Ok(record)
    }
    pub async fn observe_for(
        &self,
        token: &SessionToken,
        right: QualityRight,
    ) -> Result<QualityTrial> {
        let owner = self.owner(token)?;
        let _lock = self.lock(&owner)?;
        let record = self
            .get(&owner, right)?
            .ok_or(Error::State("no quality trial to reconcile"))?;
        self.observe_locked(token, &owner, record).await
    }
    async fn observe_locked(
        &self,
        token: &SessionToken,
        owner: &str,
        mut record: QualityTrial,
    ) -> Result<QualityTrial> {
        if record.phase == QualityTrialPhase::Sending {
            record.phase = QualityTrialPhase::Uncertain;
        }
        let result = self.query(token, record.right).await;
        self.session.check_generation(token)?;
        match result {
            Ok(observed) => {
                let fresh_phase = observed.as_ref().map(|r| r.phase());
                // An eligible/denied query cannot prove a timed-out write will
                // never commit later. Only Active resolves that uncertainty.
                if !record.phase.terminal() {
                    match fresh_phase {
                        Some(TrialPhase::Active | TrialPhase::Granted) => {
                            record.phase = QualityTrialPhase::Active
                        }
                        Some(TrialPhase::Unavailable)
                            if matches!(
                                record.phase,
                                QualityTrialPhase::Active | QualityTrialPhase::Acknowledged
                            ) =>
                        {
                            record.phase = QualityTrialPhase::Expired
                        }
                        _ => (),
                    }
                }
                // Keep the last positive observation for an Active journal's
                // invariant, but mark unsuccessful/unknown refreshes explicitly.
                if record.phase != QualityTrialPhase::Active
                    || matches!(fresh_phase, Some(TrialPhase::Active | TrialPhase::Granted))
                {
                    record.observed = observed;
                }
                record.read_failed = fresh_phase.is_none()
                    || (record.phase == QualityTrialPhase::Active
                        && !matches!(fresh_phase, Some(TrialPhase::Active | TrialPhase::Granted)));
            }
            Err(error) => {
                if error.is_service_code(301)
                    || matches!(error.root(), Error::StaleResponse | Error::Cancelled)
                {
                    return Err(error);
                }
                record.read_failed = true;
            }
        }
        record.advance()?;
        self.session
            .with_token(token, |_| self.save(owner, &record))?;
        Ok(record)
    }
}
