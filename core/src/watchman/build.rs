use super::{constants, current};
use crate::{Error, Result};

/// Only SDKs whose actual bytes and field contracts have been reviewed.
/// A remote build identifier never supplies executable code or codec tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkBuild {
    Af2952a4,
    B602a5ad7,
}
impl SdkBuild {
    pub fn parse(version: &str) -> Result<Self> {
        match version {
            "2.7.5_af2952a4" => Ok(Self::Af2952a4),
            "2.7.5_602a5ad7" => Ok(Self::B602a5ad7),
            _ => Err(Error::Response("unreviewed Watchman SDK version")),
        }
    }
    pub fn version(self) -> &'static str {
        match self {
            Self::Af2952a4 => constants::BUILD_VERSION,
            Self::B602a5ad7 => "2.7.5_602a5ad7",
        }
    }
    pub fn wire_version(self) -> &'static str {
        match self {
            Self::Af2952a4 => constants::WIRE_VERSION,
            Self::B602a5ad7 => "602a5ad7",
        }
    }
    pub fn source_sha256(self) -> &'static str {
        match self {
            Self::Af2952a4 => constants::SOURCE_SHA256,
            Self::B602a5ad7 => current::SOURCE_SHA256,
        }
    }
    pub(crate) fn alphabet(self) -> &'static [u8; 64] {
        match self {
            Self::Af2952a4 => constants::ALPHABET,
            Self::B602a5ad7 => current::ALPHABET,
        }
    }
    pub(crate) fn padding(self) -> u8 {
        match self {
            Self::Af2952a4 => b'P',
            Self::B602a5ad7 => b'3',
        }
    }
    pub(crate) fn secret(self) -> &'static [u8] {
        match self {
            Self::Af2952a4 => constants::SECRET,
            Self::B602a5ad7 => current::SECRET,
        }
    }
    pub(crate) fn substitution(self) -> &'static [u8; 256] {
        match self {
            Self::Af2952a4 => &constants::SBOX,
            Self::B602a5ad7 => &current::SBOX,
        }
    }
}
