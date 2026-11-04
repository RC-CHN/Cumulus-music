//! Optional PC account menu's read-only welfare catalog.
use super::{AccountData, MusicClient};
use crate::{
    Error, Result,
    account::{SessionStatus, SessionToken, login_policy::AB_PATH},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const EXPERIMENT: &str = "PH-PC-activityCenter";
const LIST: &str = "/api/music/multi/terminal/pc/welfare/center/list";
const LOOKUP_BUDGET: Duration = Duration::from_secs(3);
const ASSIGNMENT_AGE: Duration = Duration::from_secs(30 * 60);

#[derive(Default)]
pub(in crate::client) struct MemberWelfareHost {
    enabled: bool,
    assignment: tokio::sync::Mutex<Option<(SessionToken, Instant, bool)>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberWelfareView {
    right_tips: Option<String>,
    unread_count: u32,
    items: Vec<MemberWelfareItem>,
}

#[derive(Serialize)]
struct MemberWelfareItem {
    name: String,
    description: Option<String>,
    unread: bool,
}

fn enabled(response: &Value) -> bool {
    let Some(rows) = response["data"]
        .as_array()
        .filter(|rows| rows.len() <= 4096 && rows.iter().all(Value::is_object))
    else {
        return false;
    };
    let mut selected = rows.iter().filter(|row| row["expName"] == EXPERIMENT);
    selected
        .next()
        .is_some_and(|row| row["expGroupName"] == "t1")
        && selected.next().is_none()
}

fn label(value: &Value, max_bytes: usize) -> Option<String> {
    let text = value.as_str()?.trim();
    (!text.is_empty()
        && text.len() <= max_bytes
        && !text.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        }))
    .then(|| text.to_owned())
}

fn project(response: &Value) -> Option<MemberWelfareView> {
    // M/15's formatter uses strict numeric 200 and takes data directly.
    if response["code"] != 200 {
        return None;
    }
    let data = &response["data"];
    let rows = data["welfareList"]
        .as_array()
        .filter(|rows| rows.len() <= 128)?;
    let mut seen = HashSet::new();
    let items: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            let id = row["id"].as_str()?;
            let name = label(&row["name"], 192)?;
            if id.is_empty()
                || id.len() > 128
                || id.chars().any(char::is_control)
                || !seen.insert(id)
            {
                return None;
            }
            Some(MemberWelfareItem {
                name,
                description: label(&row["desc"], 512),
                unread: row["clicked"] != true,
            })
        })
        .collect();
    (!items.is_empty()).then(|| MemberWelfareView {
        right_tips: label(&data["rightTips"], 192),
        unread_count: data["unreadMsgCot"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0),
        items,
    })
}

impl MusicClient {
    /// Enable the native PC experiment read for the desktop host.
    pub fn with_official_member_welfare(mut self) -> Self {
        self.member_welfare.enabled = true;
        self
    }

    async fn welfare_assignment_for(&self, token: &SessionToken) -> Result<bool> {
        if !self.member_welfare.enabled {
            return Ok(false);
        }
        let mut slot = self.member_welfare.assignment.lock().await;
        self.session.with_token(token, |_| Ok(()))?;
        if let Some((owner, observed, result)) = slot.as_ref()
            && owner == token
            && observed.elapsed() < ASSIGNMENT_AGE
        {
            return Ok(*result);
        }
        let assignment = self
            .session
            .run_for(token, |api, _| async move {
                Ok(
                    match tokio::time::timeout(
                        LOOKUP_BUDGET,
                        api.call(AB_PATH, json!({"expNames":EXPERIMENT}), false, &[200]),
                    )
                    .await
                    {
                        Ok(Ok(response)) => enabled(&response),
                        _ => false,
                    },
                )
            })
            .await?;
        self.session.with_token(token, |_| {
            *slot = Some((token.clone(), Instant::now(), assignment));
            Ok(assignment)
        })
    }

    pub async fn member_welfare_available(&self, epoch: &str) -> Result<AccountData<bool>> {
        let snapshot = self.session.snapshot()?;
        let token = snapshot.token;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        let value = if snapshot.status == SessionStatus::Authenticated {
            self.welfare_assignment_for(&token).await?
        } else {
            false
        };
        self.session.check_generation(&token)?;
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }

    pub async fn member_welfare_list(
        &self,
        epoch: &str,
    ) -> Result<AccountData<Option<MemberWelfareView>>> {
        let token = self.member_token(epoch)?;
        let value = if self.welfare_assignment_for(&token).await? {
            self.session
                .run_for(&token, |api, _| async move {
                    // No clicked-ID history is collected or accepted from IPC.
                    // The established PC form normalization changes [] to "[]".
                    let result = tokio::time::timeout(
                        LOOKUP_BUDGET,
                        api.call(LIST, json!({"clickedIds":[]}), false, &[200]),
                    )
                    .await;
                    Ok(match result {
                        Ok(Ok(response)) => project(&response),
                        _ => None,
                    })
                })
                .await?
        } else {
            None
        };
        self.session.with_token(&token, |_| {
            Ok(AccountData {
                epoch: epoch.into(),
                value,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_unambiguous_assignment_opens_the_catalog() {
        let t1 = json!({"expName":EXPERIMENT,"expGroupName":"t1"});
        assert!(enabled(&json!({"data":[t1]})));
        assert!(!enabled(&json!({"data":[t1,t1]})));
        assert!(!enabled(
            &json!({"data":[{"expName":EXPERIMENT,"expGroupName":"c"}]})
        ));
        assert!(!enabled(&json!({"data":[]})));
    }

    #[test]
    fn welfare_projection_is_read_only_and_never_exposes_raw_service_links() {
        let response = json!({"code":200,"data":{"rightTips":"今日活动","unreadMsgCot":7,
            "welfareList":[{"id":"one","name":"合成活动","desc":"合成说明","clicked":false,
                "buttonTxt":"立即领取","buttonJumpUrl":"https://example.invalid/?token=synthetic",
                "img":"https://example.invalid/?ticket=synthetic"},
                {"id":"one","name":"重复活动"},
                {"id":"two","name":"已查看活动","clicked":true},
                {"id":"invalid","name":"\u{202e}"}]}});
        assert_eq!(
            serde_json::to_value(project(&response).unwrap()).unwrap(),
            json!({"rightTips":"今日活动","unreadCount":7,"items":[
                {"name":"合成活动","description":"合成说明","unread":true},
                {"name":"已查看活动","description":null,"unread":false}]})
        );
        assert!(project(&json!({"code":"200","data":response["data"]})).is_none());
        assert!(project(&json!({"code":200,"data":{"welfareList":[]}})).is_none());
    }
}
