use crate::{
    Error, Result,
    comments::{Comment, CommentResource},
    protocol::api::identifier,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Official TextArea uses JS string.length (UTF-16), not weighted playlist text.
pub fn validate_comment_text(text: &str) -> Result<()> {
    let whitespace = |c: char| matches!(c,'\u{9}'..='\u{d}'|'\u{20}'|'\u{a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}');
    if text.trim_matches(whitespace).is_empty() || text.encode_utf16().count() > 1000 {
        return Err(Error::InvalidInput(
            "comment must contain 1 to 1000 UTF-16 units",
        ));
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CommentWriteCommand {
    Publish {
        content: String,
        #[serde(default)]
        reply_to: Option<String>,
    },
    Delete {
        comment_id: String,
    },
}
impl CommentWriteCommand {
    pub(super) fn validate(&self) -> Result<()> {
        match self {
            Self::Publish { content, reply_to } => {
                validate_comment_text(content)?;
                if let Some(id) = reply_to {
                    identifier(&json!(id))?;
                }
            }
            Self::Delete { comment_id } => {
                identifier(&json!(comment_id))?;
            }
        }
        Ok(())
    }
    pub(super) fn target(&self) -> Option<&str> {
        match self {
            Self::Publish { reply_to, .. } => reply_to.as_deref(),
            Self::Delete { comment_id } => Some(comment_id),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentWritePhase {
    Prepared,
    Sending,
    Verifying,
    Uncertain,
    Confirmed,
    Rejected,
    Abandoned,
}
impl CommentWritePhase {
    pub fn pending(self) -> bool {
        matches!(
            self,
            Self::Prepared | Self::Sending | Self::Verifying | Self::Uncertain
        )
    }
}
/// Private account-scoped journal. Hosts project Comment/author data separately.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommentWrite {
    pub id: String,
    pub resource: CommentResource,
    pub command: CommentWriteCommand,
    pub phase: CommentWritePhase,
    pub created_at: u64,
    pub acknowledged: bool,
    pub receipt_id: Option<String>,
    pub receipt: Option<Comment>,
    pub before: Option<Comment>,
    pub observed: Option<Comment>,
    pub observed_absent: bool,
    pub service_code: Option<i64>,
}
pub(super) fn validate_id(id: &str) -> Result<()> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::InvalidInput("invalid comment operation ID"));
    }
    Ok(())
}
impl CommentWrite {
    pub(super) fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        self.resource.thread()?;
        self.command.validate()?;
        if let Some(id) = &self.receipt_id {
            identifier(&json!(id))?;
        }
        if (self.receipt_id.is_some() || self.phase == CommentWritePhase::Confirmed)
            && !self.acknowledged
        {
            return Err(Error::State("comment receipt lacks acknowledgement"));
        }
        if self
            .before
            .as_ref()
            .is_some_and(|c| Some(c.id.as_str()) != self.command.target())
            || self
                .receipt
                .as_ref()
                .is_some_and(|c| Some(c.id.as_str()) != self.receipt_id.as_deref())
        {
            return Err(Error::State("comment write evidence identity mismatch"));
        }
        if matches!(self.command, CommentWriteCommand::Delete { .. }) && self.before.is_none() {
            return Err(Error::State("comment delete lacks original observation"));
        }
        Ok(())
    }
}
