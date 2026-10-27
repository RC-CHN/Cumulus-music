use super::{Config, ReportUpdate, StoredValue, encode_token};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStatus {
    BehaviorAccepted,
    TimeoutFallback,
    FailureFallback,
    /// Official second callback after 420 ignores both error and business code.
    /// Preserve the bytes, but never label this as accepted by the service.
    UncheckedRetry,
}
pub struct TokenResult {
    encoded: String,
    status: TokenStatus,
}
impl TokenResult {
    pub fn encoded_native(&self) -> &str {
        &self.encoded
    }
    pub fn status(&self) -> TokenStatus {
        self.status
    }
}
pub enum BehaviorStep {
    Resubmit,
    Token(TokenResult),
    IllegalBusiness,
    Ignored,
}
#[derive(Clone, Copy)]
enum Stage {
    First,
    Retry,
    Finished,
}

/// The host feeds transport callbacks and the timeout event in arrival order.
/// No network/environment collection is hidden here. Instances cannot emit a
/// second result after completion, timeout or cancellation.
pub struct BehaviorAttempt {
    config: Config,
    nonce: String,
    deadline_ms: u64,
    stage: Stage,
}
impl BehaviorAttempt {
    pub fn new(
        config: Config,
        business: &str,
        nonce: String,
        now_ms: u64,
        timeout_ms: Option<u64>,
    ) -> Result<Self> {
        if business.is_empty()
            || business.len() > 128
            || business.contains(['\r', '\n', '\0'])
            || nonce.len() != 32
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        {
            return Err(Error::InvalidInput("invalid Watchman business or nonce"));
        }
        if config.business_suppressed(business) {
            // The SDK silently returns with no callback here. A native caller
            // gets a finite explicit result rather than an unresolvable future.
            return Err(Error::State(
                "Watchman configuration suppresses this business",
            ));
        }
        let deadline_ms = now_ms
            .checked_add(timeout_ms.unwrap_or(config.token_timeout_ms))
            .ok_or(Error::InvalidInput("Watchman deadline overflow"))?;
        Ok(Self {
            config,
            nonce,
            deadline_ms,
            stage: Stage::First,
        })
    }
    pub fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }
    fn finish(&mut self, did: &str, status: TokenStatus) -> Result<BehaviorStep> {
        let fallback = matches!(
            status,
            TokenStatus::TimeoutFallback | TokenStatus::FailureFallback
        );
        let encoded = encode_token(
            &self.config.ret,
            StoredValue::Text(did),
            &self.nonce,
            fallback,
        )?;
        self.stage = Stage::Finished;
        Ok(BehaviorStep::Token(TokenResult { encoded, status }))
    }
    /// None means transport error. The device store applies valid response slots
    /// before this method, just as SDK event handlers update storage first.
    pub fn response(&mut self, business_code: Option<i64>, did: &str) -> Result<BehaviorStep> {
        match self.stage {
            Stage::Finished => Ok(BehaviorStep::Ignored),
            Stage::Retry => self.finish(
                did,
                if business_code == Some(200) {
                    TokenStatus::BehaviorAccepted
                } else {
                    TokenStatus::UncheckedRetry
                },
            ),
            Stage::First => match business_code {
                Some(200) => self.finish(did, TokenStatus::BehaviorAccepted),
                Some(420) => {
                    self.stage = Stage::Retry;
                    Ok(BehaviorStep::Resubmit)
                }
                Some(470) => {
                    self.stage = Stage::Finished;
                    Ok(BehaviorStep::IllegalBusiness)
                }
                _ => self.finish(did, TokenStatus::FailureFallback),
            },
        }
    }
    pub fn timeout(&mut self, now_ms: u64, did: &str) -> Result<BehaviorStep> {
        if matches!(self.stage, Stage::Finished) || now_ms < self.deadline_ms {
            return Ok(BehaviorStep::Ignored);
        }
        self.finish(did, TokenStatus::TimeoutFallback)
    }
    pub fn cancel(&mut self) {
        self.stage = Stage::Finished;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeviceStep {
    Resubmit,
    Complete,
    Failed,
    Ignored,
}
/// I(): one additional device report on 420. Unlike the behavior callback,
/// second device transport failure stays failed. The host still owns coalescing.
pub struct DeviceAttempt {
    stage: Stage,
}
impl Default for DeviceAttempt {
    fn default() -> Self {
        Self {
            stage: Stage::First,
        }
    }
}
impl DeviceAttempt {
    pub fn response(&mut self, update: Result<ReportUpdate>) -> DeviceStep {
        match self.stage {
            Stage::Finished => DeviceStep::Ignored,
            Stage::First if matches!(update, Ok(ReportUpdate::ClockCorrection)) => {
                self.stage = Stage::Retry;
                DeviceStep::Resubmit
            }
            _ => {
                self.stage = Stage::Finished;
                if matches!(
                    update,
                    Ok(ReportUpdate::Accepted | ReportUpdate::ClockCorrection)
                ) {
                    DeviceStep::Complete
                } else {
                    DeviceStep::Failed
                }
            }
        }
    }
    pub fn cancel(&mut self) {
        self.stage = Stage::Finished;
    }
}
