use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CollectionQuery {
    SavedAlbums { offset: usize, limit: usize },
    PurchasedAlbums,
    FollowedArtists { cursor: String, size: usize },
}
pub(super) fn cursor(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 8192 || value.chars().any(char::is_control) {
        return Err(Error::InvalidInput("invalid collection cursor"));
    }
    Ok(())
}
impl CollectionQuery {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::SavedAlbums { offset, limit } => {
                if *offset > 50_000 || !(1..=200).contains(limit) {
                    return Err(Error::InvalidInput("invalid collection page"));
                }
            }
            Self::PurchasedAlbums => {}
            Self::FollowedArtists {
                cursor: value,
                size,
            } => {
                cursor(value)?;
                if !(1..=100).contains(size) {
                    return Err(Error::InvalidInput("invalid followed artist page size"));
                }
            }
        }
        Ok(())
    }
}
