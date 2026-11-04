//! M/2264 fetchEventList and M/15.dc: a profile's notes, not the followed feed.
use super::{
    CommunityFeedAction, CommunityFeedPage, CommunityHost, ReadLease, State, handle, project,
};
use crate::{
    Error, Result,
    account::SessionToken,
    client::{MusicClient, dto::AccountData},
    error::code_matches,
};
use serde_json::{Value, json};
use uuid::Uuid;

const LIMIT: usize = 20;
const MAX_CURSOR: u64 = 9_007_199_254_740_991;

fn cursor(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse::<u64>().ok())
        .filter(|value| *value > 0 && *value <= MAX_CURSOR)
}

fn forbidden(error: &Error) -> bool {
    error.is_service_code(403)
        || error.is_service_code(404)
        || matches!(
            error.root(),
            Error::HttpStatus {
                status: 403 | 404,
                ..
            }
        )
}

impl CommunityHost {
    fn begin_profile(
        &self,
        token: &SessionToken,
        uid: &str,
        action: CommunityFeedAction,
    ) -> Result<ReadLease<'_>> {
        let mut state = self.state(token)?;
        if state.profile.as_deref() != Some(uid) {
            if matches!(action, CommunityFeedAction::Next { .. }) {
                return Err(Error::StaleResponse);
            }
            *state = State {
                generation: Some(token.generation),
                profile: Some(uid.into()),
                ..Default::default()
            };
        }
        if state.pending.is_some() {
            return Err(Error::State("profile notes read is pending"));
        }
        let (cursor, refresh) = match action {
            CommunityFeedAction::First | CommunityFeedAction::Refresh => (json!(-1), true),
            CommunityFeedAction::Next { continuation } => {
                let key = handle(&continuation)?;
                let (_, value) = state
                    .continuation
                    .as_ref()
                    .filter(|(id, _)| *id == key)
                    .ok_or(Error::StaleResponse)?;
                (value.clone(), false)
            }
        };
        let request = Uuid::new_v4();
        state.pending = Some(request);
        Ok(ReadLease {
            host: self,
            generation: token.generation,
            request,
            cursor,
            refresh,
        })
    }
}

impl MusicClient {
    pub async fn profile_notes(
        &self,
        epoch: &str,
        uid: &str,
        action: CommunityFeedAction,
    ) -> Result<AccountData<CommunityFeedPage>> {
        let token = self.playlist_token(epoch)?;
        let uid = project::id(&json!(uid)).ok_or(Error::InvalidInput("invalid notes profile"))?;
        let request = self.session.with_token(&token, |_| {
            self.profile_notes.begin_profile(&token, &uid, action)
        })?;
        let args = json!({"offset":0,"limit":LIMIT,"getcounts":true,"time":request.cursor});
        let response = self
            .session
            .run_for(&token, |api, viewer| {
                let uid = &uid;
                let token = &token;
                let request = &request;
                async move {
                    if project::id(&json!(viewer)).is_none() {
                        return Err(Error::State("profile notes require an account"));
                    }
                    if viewer != *uid {
                        let path = format!("/api/w/v1/user/detail/{uid}");
                        let detail = match api
                            .call(
                                &path,
                                json!({"all":true,"userId":uid}),
                                false,
                                &[200, 403, 404],
                            )
                            .await
                        {
                            Ok(result) if code_matches(&result["code"], 200) => result,
                            Ok(_) => return Ok(None),
                            Err(error) if forbidden(&error) => return Ok(None),
                            Err(error) => return Err(error),
                        };
                        if project::id(&detail["profile"]["userId"]).as_ref() != Some(uid) {
                            return Err(Error::Response("notes profile identity mismatch"));
                        }
                        if detail["profile"]["inBlacklist"].as_bool() == Some(true) {
                            return Ok(None);
                        }
                        self.session.require_api(token, &api)?;
                    }
                    self.session.with_token(token, |_| {
                        let state = request.host.state(token)?;
                        if state.pending != Some(request.request)
                            || state.profile.as_ref() != Some(uid)
                        {
                            return Err(Error::StaleResponse);
                        }
                        Ok(())
                    })?;
                    let path = format!("/api/event/get/{uid}");
                    match api.call(&path, args, false, &[200, 403, 404]).await {
                        Ok(result) if code_matches(&result["code"], 200) => Ok(Some(result)),
                        Ok(_) => Ok(None),
                        Err(error) if forbidden(&error) => Ok(None),
                        Err(error) => Err(error),
                    }
                }
            })
            .await?;
        self.session.with_token(&token, |_| {
            let value = if let Some(response) = response {
                let raw = response["events"]
                    .as_array()
                    .filter(|rows| rows.len() <= LIMIT)
                    .ok_or(Error::Response("invalid profile note rows"))?;
                let more = response["more"]
                    .as_bool()
                    .ok_or(Error::Response("missing profile note continuation flag"))?;
                let next = if more {
                    let after = cursor(&response["lasttime"])
                        .filter(|after| {
                            cursor(&request.cursor).is_none_or(|before| *after < before)
                        })
                        .ok_or(Error::Response("profile note continuation did not advance"))?;
                    Some(json!(after))
                } else {
                    None
                };
                request.settle_rows(&token, raw, next, request.refresh, Some(&uid))?
            } else {
                let mut page = request.settle_rows(&token, &[], None, true, Some(&uid))?;
                page.unavailable = true;
                page
            };
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}
