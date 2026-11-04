//! Handoff of the original authorized bytes to an in-process FLAC decoder.
use crate::{
    Error, Result,
    account::profiles::ColdOfflineGrant,
    media::ncm::{AudioFormat, HeaderPolicy, NcmReader},
    membership::ColdEntitlement,
    playback::engine::{Cancellation, pinned_audio_format},
};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_READ: usize = 64 * 1024;

enum Payload {
    Plain(File),
    StrictNcm(Box<NcmReader<File>>),
}

impl Read for Payload {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(file) => file.read(out),
            Self::StrictNcm(reader) => reader.read(out),
        }
    }
}

impl Seek for Payload {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        match self {
            Self::Plain(file) => file.seek(position),
            Self::StrictNcm(reader) => reader.seek(position),
        }
    }
}

/// Native-only FLAC bytes from a verified, still-authorized cold download.
/// Coordinates are relative to the FLAC payload for both plain and NCM.
/// No path or raw unguarded file can be retrieved from this reader.
pub struct ColdOfflineFlacReader {
    authority: ColdOfflineFlacAuthority,
    payload: Payload,
    size: u64,
    offset: u64,
    scratch: Box<[u8; MAX_READ]>,
}

/// Owned validation handle a native host can retain after moving the reader
/// into its decoder. A denied host validation revokes every shared fence.
#[derive(Clone)]
pub struct ColdOfflineFlacAuthority {
    grant: Arc<ColdOfflineGrant>,
    entitlement: Option<ColdEntitlement>,
    cancellation: Cancellation,
}

impl ColdOfflineFlacAuthority {
    /// Share this fence with the decoder and sink: native decoders can
    /// produce buffered PCM without calling `Read` again.
    pub fn cancellation(&self) -> Cancellation {
        self.cancellation.clone()
    }

    /// Blocking host validation of profile binding and persisted membership.
    /// Call at publication and during ordinary host polling, never from an
    /// audio callback. A denial permanently closes this reader and its fence.
    pub fn validate(&self) -> Result<()> {
        let result = (|| {
            self.cancellation.check()?;
            self.grant.validate()?;
            if let Some(entitlement) = &self.entitlement {
                self.grant.validate_entitlement(entitlement)?;
            }
            self.cancellation.check()
        })();
        if result.is_err() {
            self.cancellation.cancel();
        }
        result
    }
}

impl ColdOfflineFlacReader {
    pub(crate) fn new(
        grant: Arc<ColdOfflineGrant>,
        mut file: File,
        entitlement: Option<ColdEntitlement>,
        lease: Cancellation,
    ) -> Result<Self> {
        let live_grant = grant.clone();
        let live_entitlement = entitlement.clone();
        let denied = AtomicBool::new(false);
        let cancellation = Cancellation::new(move || {
            if denied.load(Ordering::Acquire) {
                return false;
            }
            if lease.is_cancelled()
                || !live_grant.playback_current()
                || live_entitlement
                    .as_ref()
                    .is_some_and(|rights| !rights.valid())
            {
                denied.store(true, Ordering::Release);
            }
            !denied.load(Ordering::Acquire)
        });
        cancellation.check()?;
        let mut payload = if entitlement.is_some() {
            let reader = NcmReader::new(file, HeaderPolicy::Strict)?;
            if reader.info().audio_format != Some(AudioFormat::Flac) {
                return Err(Error::InvalidInput("cold offline FLAC payload required"));
            }
            Payload::StrictNcm(Box::new(reader))
        } else {
            if pinned_audio_format(&mut file)? != "flac" {
                return Err(Error::InvalidInput("cold offline FLAC payload required"));
            }
            Payload::Plain(file)
        };
        let size = payload.seek(SeekFrom::End(0))?;
        payload.rewind()?;
        let reader = Self {
            authority: ColdOfflineFlacAuthority {
                grant,
                entitlement,
                cancellation,
            },
            payload,
            size,
            offset: 0,
            scratch: Box::new([0; MAX_READ]),
        };
        reader.authority.validate()?;
        Ok(reader)
    }

    pub fn authority(&self) -> ColdOfflineFlacAuthority {
        self.authority.clone()
    }

    fn check_io(&self) -> io::Result<()> {
        self.authority.cancellation.check().map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cold offline FLAC authority ended",
            )
        })
    }
}

impl Read for ColdOfflineFlacReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.check_io()?;
        let remaining = self.size - self.offset;
        let bound = out
            .len()
            .min(MAX_READ)
            .min(remaining.try_into().unwrap_or(usize::MAX));
        if bound == 0 {
            return Ok(0);
        }
        let result = self.payload.read(&mut self.scratch[..bound]);
        self.check_io()?;
        let count = result?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "cold offline FLAC was truncated",
            ));
        }
        if count > bound {
            return Err(io::Error::other("invalid cold offline FLAC read count"));
        }
        out[..count].copy_from_slice(&self.scratch[..count]);
        self.offset += count as u64;
        Ok(count)
    }
}

impl Seek for ColdOfflineFlacReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.check_io()?;
        let offset = match from {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(delta) => self.offset.checked_add_signed(delta),
            SeekFrom::End(delta) => self.size.checked_add_signed(delta),
        }
        .filter(|offset| *offset <= self.size)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek outside cold offline FLAC payload",
            )
        })?;
        let result = self.payload.seek(SeekFrom::Start(offset));
        self.check_io()?;
        match result {
            Ok(actual) if actual == offset => {}
            Ok(_) => {
                self.authority.cancellation.cancel();
                return Err(io::Error::other("unexpected cold offline FLAC seek offset"));
            }
            Err(error) => {
                self.authority.cancellation.cancel();
                return Err(error);
            }
        }
        self.offset = offset;
        Ok(offset)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        self.check_io()?;
        Ok(self.offset)
    }
}
