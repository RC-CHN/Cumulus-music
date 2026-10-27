use super::{
    AttachmentKind, CloudApi,
    api::{validate_digest, validate_file_name},
};
use crate::{Error, Result, protocol::api::identifier};
use serde_json::{Value, json};

/// Construct only after the attachment upload has been acknowledged.
pub struct UploadedAttachment {
    pub kind: AttachmentKind,
    pub document_id: Value,
}

/// Audio acknowledged by the upload backend, or deduplicated by upload/check.
/// The upload ticket is opaque and is NOT a numeric catalog song identifier.
pub struct AudioMetadata<'a> {
    pub upload_ticket: &'a str,
    pub md5: &'a str,
    pub file_name: &'a str,
    pub bitrate: u64,
    pub resource_id: Option<&'a str>,
    pub object: Option<(&'a str, &'a str)>,
    pub song: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    pub attachments: &'a [UploadedAttachment],
}

impl CloudApi {
    /// Native MetaInfo builder 0x819c0. The transport must preserve numeric
    /// bitrate and native object header instead of frontend form conversion.
    pub async fn metadata(&self, metadata: AudioMetadata<'_>) -> Result<Value> {
        let ticket = metadata.upload_ticket;
        if ticket.is_empty() || ticket.chars().count() > 4096 || ticket.contains(['\r', '\n', '\0'])
        {
            return Err(Error::InvalidInput("invalid upload song ticket"));
        }
        validate_digest(metadata.md5)?;
        validate_file_name(metadata.file_name)?;
        let mut payload = json!({"md5":metadata.md5,"songid":ticket,"filename":metadata.file_name,"bitrate":metadata.bitrate});
        if let Some(resource) = metadata.resource_id.filter(|s| !s.is_empty()) {
            payload["resourceId"] = resource.into();
        }
        if let Some((bucket, key)) = metadata.object {
            if bucket.is_empty() || key.is_empty() {
                return Err(Error::InvalidInput("empty cloud object destination"));
            }
            // Native 0x74b5d..0x74be1 prefixes the bucket. Omitting it produced
            // a real service 409 in the original recovery experiments.
            payload["objectKey"] = format!("{bucket}/{key}").into();
        }
        for (key, text) in [
            ("song", metadata.song),
            ("artist", metadata.artist),
            ("album", metadata.album),
        ] {
            if !text.is_empty() {
                payload[key] = text.into();
            }
        }
        for attachment in metadata.attachments {
            let key = match attachment.kind {
                AttachmentKind::Cover => "coverid",
                AttachmentKind::Lyric => "lyricid",
                AttachmentKind::Cue => "cueid",
            };
            if payload.get(key).is_some() {
                return Err(Error::InvalidInput("duplicate attachment kind"));
            }
            payload[key] = identifier(&attachment.document_id)?.into();
            if attachment.kind == AttachmentKind::Lyric {
                payload["lyrictype"] = "lrc".into();
            }
        }
        self.api
            .call("/api/upload/cloud/info/v2", payload, false, &[200])
            .await
    }
}
