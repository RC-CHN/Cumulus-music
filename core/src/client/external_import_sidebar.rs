//! Native assignment for the optional official PC sidebar import menu.
use super::{MusicClient, dto::AccountData};
use crate::{
    Error, Result,
    account::{SessionStatus, login_policy::AB_PATH},
};
use serde_json::{Value, json};
use std::time::Duration;

const EXPERIMENT: &str = "PH-PC-songlistTransfer";

fn assigned(value: &Value) -> bool {
    let Some(rows) = value["data"]
        .as_array()
        .filter(|rows| rows.len() <= 4096 && rows.iter().all(Value::is_object))
    else {
        return false;
    };
    let mut matching = rows.iter().filter(|row| row["expName"] == EXPERIMENT);
    matching
        .next()
        .is_some_and(|row| row["expGroupName"] == "t1")
        && matching.next().is_none()
}

impl MusicClient {
    /// Production desktop reads the native PC assignment. Renderer IPC only
    /// exposes the result; neither the name nor the assigned group is writable.
    pub fn with_official_external_import_sidebar(mut self) -> Self {
        self.external_import_sidebar = true;
        self
    }

    pub async fn external_import_sidebar(&self, epoch: &str) -> Result<AccountData<bool>> {
        let snapshot = self.session.snapshot()?;
        let token = snapshot.token;
        if token.generation.to_string() != epoch {
            return Err(Error::StaleResponse);
        }
        // A first-run PC account check with no saved credentials can leave
        // the native snapshot Unknown. It still uses the guest transport.
        let value = if self.external_import_sidebar && snapshot.status != SessionStatus::Expired {
            let api = self.session.native_api(&token)?;
            let reply = tokio::time::timeout(
                Duration::from_secs(3),
                api.call(AB_PATH, json!({"expNames":EXPERIMENT}), false, &[200]),
            )
            .await;
            self.session.require_api(&token, &api)?;
            matches!(reply, Ok(Ok(response)) if assigned(&response))
        } else {
            self.session.check_generation(&token)?;
            false
        };
        Ok(AccountData {
            epoch: epoch.into(),
            value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_exact_server_assignment_enables_the_sidebar() {
        let row = json!({"expName":EXPERIMENT,"expGroupName":"t1"});
        let unrelated = json!({"expName":"PH-PC-tagT","expGroupName":"c"});
        assert!(assigned(&json!({"data":[row, unrelated]})));
        for invalid in [
            json!({"data":[unrelated]}),
            json!({"data":[row, row]}),
            json!({"data":[row, null]}),
            json!({"data":[{"expName":EXPERIMENT,"expGroupName":"T1"}]}),
            json!({"data":[{"expName":EXPERIMENT,"expGroupName":null}]}),
            json!({}),
        ] {
            assert!(!assigned(&invalid));
        }
    }
}
