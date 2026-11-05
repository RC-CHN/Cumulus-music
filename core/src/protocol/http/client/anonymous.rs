use super::*;
use crate::protocol::pc_experiments::{ANONYMOUS_PATH, PcAnonymousState};

impl MusicHttpClient {
    pub(super) async fn register_anonymous(&self) -> Result<PcAnonymousState> {
        if !self.anonymous_store {
            return Err(Error::State(
                "anonymous registration requires an isolated guest store",
            ));
        }
        self.pc_experiment_revision_for(false)?;
        match self.credentials.anonymous_state()? {
            state @ PcAnonymousState::Registered { .. } => return Ok(state),
            PcAnonymousState::Uncertain => return Ok(PcAnonymousState::Uncertain),
            PcAnonymousState::Unsupported => {
                return Err(Error::State(
                    "anonymous registration cannot replace an account Cookie",
                ));
            }
            PcAnonymousState::Unregistered => (),
        }
        // M198 retries a definite code=200/missing-UID response. Three is a
        // community lifetime bound; an unreadable/lost ACK is never replayed.
        for _ in 0..3 {
            let intent: crate::account::credentials::AnonymousIntent =
                self.credentials.begin_anonymous()?;
            let allowed = || {
                if self.pc_experiment_revision_for(false)? != intent.revision
                    || matches!(
                        self.credentials.anonymous_state()?,
                        PcAnonymousState::Unsupported
                    )
                {
                    return Err(Error::CredentialChanged);
                }
                Ok(())
            };
            let value = self
                .execute(
                    ANONYMOUS_PATH,
                    json!({"username":&intent.username}),
                    false,
                    Some(&intent.revision),
                    Jsk::None,
                    Some(RequestBinding::AnonymousRegistration {
                        permission: &allowed,
                        dispatched: &intent.dispatched,
                    }),
                )
                .await?;
            // The original helper uses strict numeric 200 before a truthy UID.
            if value["code"].as_i64() != Some(200) {
                intent.rejected()?;
                return Err(Error::service(ANONYMOUS_PATH, value["code"].clone(), value));
            }
            if crate::protocol::wire::serialization::js_truthy(&value["userId"]) {
                let uid = crate::protocol::api::identifier(&value["userId"])?;
                return intent.acknowledged(uid);
            }
            intent.rejected()?;
        }
        Err(Error::State(
            "PC anonymous registration returned no identity within the local bound",
        ))
    }
}
