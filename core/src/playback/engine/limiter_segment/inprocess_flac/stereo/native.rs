//! Ordinary native file owner. No path can be opened through this module.
use super::{
    DecoderKind, FlacProfile, HeldInput, HeldPass, HeldSource, PcmWave, Selection, SourceAuthority,
    SourceGraph, Start,
};
use crate::{
    Error, Result,
    effects::stereo::LocalEqualizerMaterialPlan,
    playback::engine::{
        Cancellation, StereoPcmControl, StereoPcmReader, stereo_pcm::StereoGraphControl,
    },
};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) enum NativeStereoSelection {
    Qualified(Box<NativeStereoSource>),
    Stock(NativeStereoFile),
}

pub(crate) enum NativeStereoStart {
    Ready(Box<NativeStereoPass>),
    Stock(Box<NativeStereoSource>),
}

pub(crate) struct NativeStereoSource(HeldSource<NativeStereoFile>);
pub(crate) struct NativeStereoPass(HeldPass<NativeStereoFile>);

impl NativeStereoSource {
    /// Consume the descriptor returned by the native path/fingerprint check.
    /// The caller owns its authorization lifetime, independent of a single
    /// paused load or seek attempt; it is checked in decoder callbacks and
    /// must not lock the native engine. The library path is a trusted host path.
    pub(crate) fn select(
        file: File,
        authority: Cancellation,
        library: &Path,
        plan: LocalEqualizerMaterialPlan,
    ) -> Result<NativeStereoSelection> {
        Self::select_flac(file, authority, library, plan, FlacProfile::NativeOpen)
    }

    /// The ordinary indexed caller has consumed its one-shot verified FLAC
    /// receipt. Only that lane opts into reframing larger decoder callbacks.
    pub(crate) fn select_indexed(
        file: File,
        authority: Cancellation,
        library: &Path,
        plan: LocalEqualizerMaterialPlan,
    ) -> Result<NativeStereoSelection> {
        Self::select_flac(file, authority, library, plan, FlacProfile::IndexedLocal)
    }

    fn select_flac(
        file: File,
        authority: Cancellation,
        library: &Path,
        plan: LocalEqualizerMaterialPlan,
        profile: FlacProfile,
    ) -> Result<NativeStereoSelection> {
        let reader = NativeStereoFile::new(file, authority)?;
        Ok(
            match HeldSource::select_flac(reader, library, profile, |format| {
                SourceGraph::new_local(format, plan)
            })? {
                Selection::Qualified(source) => {
                    NativeStereoSelection::Qualified(Box::new(Self(*source)))
                }
                Selection::Stock(reader) => NativeStereoSelection::Stock(reader),
            },
        )
    }

    pub(crate) fn select_wave(
        file: File,
        authority: Cancellation,
        plan: LocalEqualizerMaterialPlan,
    ) -> Result<NativeStereoSelection> {
        let mut reader = NativeStereoFile::new(file, authority)?;
        let layout = PcmWave::inspect(&mut reader)?;
        reader.wave_layout = Some(layout);
        Ok(
            match HeldSource::qualified(
                reader,
                DecoderKind::Wave(layout),
                layout.rate,
                layout.frames,
                |format| SourceGraph::new_local(format, plan),
            )? {
                Selection::Qualified(source) => {
                    NativeStereoSelection::Qualified(Box::new(Self(*source)))
                }
                Selection::Stock(reader) => NativeStereoSelection::Stock(reader),
            },
        )
    }

    pub(crate) fn total_frames(&self) -> u64 {
        self.0.total
    }

    pub(crate) fn into_reader(self) -> Result<NativeStereoFile> {
        self.0.into_reader()
    }

    pub(crate) fn configure_reclaimed(
        &mut self,
        plan: LocalEqualizerMaterialPlan,
        master_bypassed: bool,
    ) -> Result<()> {
        self.0.reader.source_authority().validate()?;
        self.0.graph.apply(if master_bypassed {
            StereoGraphControl::DisableLocalMaster
        } else {
            StereoGraphControl::ApplyLocal(plan)
        })?;
        self.0.reader.source_authority().validate()
    }

    /// A live pass remains bound to its source authority, not the request
    /// that successfully finished preparing it.
    pub(crate) fn start(self, first: u64, cancel: &Cancellation) -> Result<NativeStereoStart> {
        Ok(match self.0.start(first, cancel)? {
            Start::Ready(pass) => NativeStereoStart::Ready(Box::new(NativeStereoPass(*pass))),
            Start::Stock(source) => NativeStereoStart::Stock(Box::new(Self(*source))),
        })
    }
}

impl NativeStereoPass {
    pub(crate) fn control(&self) -> StereoPcmControl {
        self.0.control.clone()
    }

    pub(crate) fn take_reader(&mut self) -> Result<StereoPcmReader> {
        self.0.take_reader()
    }

    pub(crate) fn discard_retired(mut self) -> Result<()> {
        // Retirement drops the old source. Its revoked authorization cannot
        // be used to open, seek, or pass a descriptor into the new sink.
        self.0.join_retired().map(drop)
    }
    pub(crate) fn reclaim(self) -> Result<NativeStereoSource> {
        self.0.reclaim().map(NativeStereoSource)
    }

    /// Queue an in-place full apply. The returned revision is only accepted;
    /// the owner must await and fence the actual worker acknowledgement.
    pub(crate) fn apply_material(&self, plan: LocalEqualizerMaterialPlan) -> Result<u64> {
        self.0
            .control
            .queue_graph_control(StereoGraphControl::ApplyLocal(plan))
    }

    pub(crate) fn disable_master(&self) -> Result<u64> {
        self.0
            .control
            .queue_graph_control(StereoGraphControl::DisableLocalMaster)
    }
}

/// Own exactly one verified local handle, never a path or a cloned file.
pub(crate) struct NativeStereoFile {
    file: File,
    authority: Cancellation,
    wave_layout: Option<PcmWave>,
}

impl NativeStereoFile {
    fn new(file: File, authority: Cancellation) -> Result<Self> {
        authority.check()?;
        if !file.metadata()?.is_file() {
            return Err(Error::InvalidInput(
                "native stereo source must be a regular file",
            ));
        }
        // A source predicate that was observed revoked cannot revive a held
        // descriptor, even if a later caller predicate becomes true again.
        let revoked = AtomicBool::new(false);
        let authority = Cancellation::new(move || {
            if authority.is_cancelled() {
                revoked.store(true, Ordering::Release);
            }
            !revoked.load(Ordering::Acquire)
        });
        authority.check()?;
        Ok(Self {
            file,
            authority,
            wave_layout: None,
        })
    }

    /// Revalidate, rewind and move the original descriptor and its lifetime
    /// together into the stock pinned sink. No stock path reauthorizes it.
    pub(crate) fn into_stock(mut self) -> Result<(File, Cancellation)> {
        self.authority.check()?;
        if let Some(layout) = self.wave_layout
            && PcmWave::inspect(&mut self)? != layout
        {
            return Err(Error::Media("held PCM WAV layout changed"));
        }
        self.rewind()?;
        self.authority.check()?;
        Ok((self.file, self.authority))
    }

    fn check_io(&self) -> io::Result<()> {
        self.authority.check().map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "native stereo source authority ended",
            )
        })
    }
}
impl HeldInput for NativeStereoFile {
    fn source_authority(&self) -> SourceAuthority {
        SourceAuthority::Native(self.authority.clone())
    }
}
impl Read for NativeStereoFile {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.check_io()?;
        let result = self.file.read(out);
        if let Err(error) = self.check_io() {
            out.fill(0);
            return Err(error);
        }
        result
    }
}
impl Seek for NativeStereoFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.check_io()?;
        let result = self.file.seek(position);
        self.check_io()?;
        result
    }
}

#[cfg(test)]
mod tests;
