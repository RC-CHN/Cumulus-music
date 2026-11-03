//! Human-readable PC login notices. Raw toast objects and their authorization
//! URLs/tickets stay in the native candidate and never become renderer data.
use super::LoginStatus;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginNoticeKind {
    Risk,
    Upgrade,
    Disabled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginNoticeView {
    pub kind: LoginNoticeKind,
    pub title: String,
    pub paragraphs: Vec<String>,
}

fn text(value: &Value, limit: usize) -> Option<String> {
    value
        .as_str()
        .filter(|v| {
            !v.trim().is_empty()
            && v.chars().count() <= limit
            && !v.chars().any(|c| c.is_control() && c != '\n')
            // Notices are plain text, not a second authorization-link channel.
            && !v.contains("://")
        })
        .map(str::to_owned)
}

pub(super) fn project(status: LoginStatus, response: Option<&Value>) -> Option<LoginNoticeView> {
    match status {
        LoginStatus::RiskNotice => {
            let toast = response.and_then(|v| v.get("toast"));
            Some(LoginNoticeView {
                kind: LoginNoticeKind::Risk,
                title: toast
                    .and_then(|v| text(&v["title"], 128))
                    .unwrap_or_else(|| "登录风险提示".into()),
                paragraphs: toast
                    .and_then(|v| v["content"].as_array())
                    .filter(|v| v.len() <= 16)
                    .map(|v| v.iter().filter_map(|p| text(p, 512)).collect())
                    .unwrap_or_default(),
            })
        }
        LoginStatus::UpgradeRequired => Some(LoginNoticeView {
            kind: LoginNoticeKind::Upgrade,
            title: "版本更新提醒".into(),
            paragraphs: vec!["当前版本过低，暂不支持账号密码登录，请先更新至最新版本".into()],
        }),
        LoginStatus::AccountDisabled => Some(LoginNoticeView {
            kind: LoginNoticeKind::Disabled,
            title: "用户禁止登录".into(),
            paragraphs: vec![],
        }),
        _ => None,
    }
}
