use super::*;
use crate::account::credentials::RequestCredentials;
use crate::protocol::pc_experiments::{ANONYMOUS_PATH, PcAnonymousState};

/// Native only. Neither the guest identifier nor the credential revision is a
/// logged-in account, and neither can be supplied by a WebView.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct GuestSearchAuthority {
    uid: String,
    pub(crate) revision: String,
}

impl MusicHttpClient {
    pub(crate) fn is_isolated_anonymous(&self) -> bool {
        self.anonymous_store
    }

    pub(crate) fn guest_search_authority(&self) -> Result<Option<GuestSearchAuthority>> {
        if !self.is_isolated_anonymous() {
            return Ok(None);
        }
        let PcAnonymousState::Registered { uid, revision } = self.pc_anonymous_state()? else {
            return Ok(None);
        };
        for path in [
            "/api/search/default/keyword/get",
            crate::catalog::search::results::COMPREHENSIVE_PATH,
        ] {
            let target = super::super::ApiOrigin::Pc.request_url(WireRoute::Eapi, path)?;
            let snapshot = self.credentials.request_credentials(&target)?;
            if snapshot.revision != revision || !guest_search_cookies(&snapshot) {
                return Ok(None);
            }
        }
        Ok(Some(GuestSearchAuthority { uid, revision }))
    }

    pub(super) fn require_guest_search_target(
        &self,
        target: &url::Url,
        expected: &RequestCredentials,
    ) -> Result<()> {
        let current = self.credentials.request_credentials(target)?;
        if current.revision != expected.revision
            || current.device_id != expected.device_id
            || !guest_search_cookies(&current)
        {
            return Err(Error::CredentialChanged);
        }
        Ok(())
    }

    pub(crate) async fn read_guest_search(
        &self,
        path: &str,
        payload: Value,
        authority: &GuestSearchAuthority,
        permission: &(dyn Fn() -> Result<()> + Send + Sync),
    ) -> Result<Value> {
        // Catalog builders validate the typed user input. This second closed
        // list cannot authorize a playback URL, privilege, write, or login.
        let permitted = matches!(
            path,
            "/api/search/default/keyword/get"
                | "/api/search/pc/suggest/keyword/get"
                | "/api/search/pc/rcmd/keyword/get"
                | "/api/search/pc/chart/list"
                | "/api/search/pc/chart/detail"
                | crate::catalog::search::results::COMPREHENSIVE_PATH
                | "/api/search/song/list/page"
                | "/api/v1/search/playlist/get"
                | "/api/v1/search/artist/get"
                | "/api/v1/search/album/get"
                | "/api/v1/search/user/get"
                | "/api/search/voice/get"
                | "/api/search/voicelist/get"
                | "/api/search/resource/lyric"
                | "/api/search/mlog/get"
        );
        if !permitted {
            return Err(Error::InvalidInput("unsupported guest search endpoint"));
        }
        let origin = self.credentials.origin();
        let route = select_route_with_policy(origin, path, &payload, self.options.routing)?;
        if origin != super::super::ApiOrigin::Pc || route != WireRoute::Eapi {
            return Err(Error::State("guest search requires the native PC route"));
        }
        let target = origin.request_url(route, path)?;
        let snapshot = self.credentials.request_credentials(&target)?;
        let bound = || {
            permission()?;
            if self.guest_search_authority()?.as_ref() != Some(authority)
                || snapshot.revision != authority.revision
            {
                return Err(Error::CredentialChanged);
            }
            self.require_guest_search_target(&target, &snapshot)
        };
        bound()?;
        let reply = self
            .execute(
                path,
                payload,
                false,
                Some(&authority.revision),
                Jsk::None,
                Some(RequestBinding::GuestSearch(&bound)),
            )
            .await;
        // Do not publish a response after adoption or a credential commit,
        // even when that commit was carried by the search response itself.
        bound()?;
        reply
    }

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
                match self.credentials.begin_anonymous() {
                    Ok(intent) => intent,
                    Err(Error::StaleResponse) => {
                        return match self.credentials.anonymous_state()? {
                            registered @ PcAnonymousState::Registered { .. } => Ok(registered),
                            _ => Err(Error::StaleResponse),
                        };
                    }
                    Err(error) => return Err(error),
                };
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

fn guest_search_cookies(snapshot: &RequestCredentials) -> bool {
    if snapshot.device_id.is_empty() || snapshot.cookies.iter().any(|(name, _)| name == "MUSIC_U") {
        return false;
    }
    let mut anonymous = snapshot
        .cookies
        .iter()
        .filter_map(|(name, value)| (name == "MUSIC_A").then_some(value));
    anonymous.next().is_some_and(|value| !value.is_empty()) && anonymous.next().is_none()
}
