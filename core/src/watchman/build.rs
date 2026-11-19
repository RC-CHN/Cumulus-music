use super::{constants, current, e2891084};
use crate::{Error, Result};

/// Only SDKs whose actual bytes and field contracts have been reviewed.
/// A remote build identifier never supplies executable code or codec tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkBuild {
    Af2952a4,
    B602a5ad7,
    E2891084,
}
impl SdkBuild {
    pub fn parse(version: &str) -> Result<Self> {
        match version {
            "2.7.5_af2952a4" => Ok(Self::Af2952a4),
            "2.7.5_602a5ad7" => Ok(Self::B602a5ad7),
            "2.7.5_e2891084" => Ok(Self::E2891084),
            _ => Err(Error::VerificationSdkUnsupported),
        }
    }
    pub fn version(self) -> &'static str {
        match self {
            Self::Af2952a4 => constants::BUILD_VERSION,
            Self::B602a5ad7 => "2.7.5_602a5ad7",
            Self::E2891084 => e2891084::BUILD_VERSION,
        }
    }
    pub fn wire_version(self) -> &'static str {
        match self {
            Self::Af2952a4 => constants::WIRE_VERSION,
            Self::B602a5ad7 => "602a5ad7",
            Self::E2891084 => e2891084::WIRE_VERSION,
        }
    }
    pub fn source_sha256(self) -> &'static str {
        match self {
            Self::Af2952a4 => constants::SOURCE_SHA256,
            Self::B602a5ad7 => current::SOURCE_SHA256,
            Self::E2891084 => e2891084::SOURCE_SHA256,
        }
    }
    pub(crate) fn alphabet(self) -> &'static [u8; 64] {
        match self {
            Self::Af2952a4 => constants::ALPHABET,
            Self::B602a5ad7 => current::ALPHABET,
            Self::E2891084 => e2891084::ALPHABET,
        }
    }
    pub(crate) fn padding(self) -> u8 {
        match self {
            Self::Af2952a4 => b'P',
            Self::B602a5ad7 => b'3',
            Self::E2891084 => e2891084::PADDING,
        }
    }
    pub(crate) fn secret(self) -> &'static [u8] {
        match self {
            Self::Af2952a4 => constants::SECRET,
            Self::B602a5ad7 => current::SECRET,
            Self::E2891084 => e2891084::SECRET,
        }
    }
    pub(crate) fn substitution(self) -> &'static [u8; 256] {
        match self {
            Self::Af2952a4 => &constants::SBOX,
            Self::B602a5ad7 => &current::SBOX,
            Self::E2891084 => &e2891084::SBOX,
        }
    }
}
