use super::{DecodedPacket, DecoderPayload, Generation, PACKET_FRAMES, PcmWave};
use crate::{Error, Result, playback::engine::pcm_wave::BYTES_PER_FRAME};
use std::io::{self, Read, Seek, SeekFrom};

pub(super) struct WavePackets<'a, R> {
    reader: &'a mut R,
    generation: Generation,
    cursor: u64,
    total: u64,
    ended: bool,
}

impl<'a, R: Read + Seek> WavePackets<'a, R> {
    pub(super) fn open(
        reader: &'a mut R,
        wave: PcmWave,
        first: u64,
        generation: Generation,
    ) -> Result<Self> {
        let offset = wave.byte_offset(first)?;
        if reader.seek(SeekFrom::Start(offset))? != offset {
            return Err(Error::Media("PCM WAV decoder seek did not reach its range"));
        }
        Ok(Self {
            reader,
            generation,
            cursor: first,
            total: wave.frames,
            ended: false,
        })
    }

    pub(super) fn next(&mut self) -> io::Result<Option<DecodedPacket>> {
        if self.ended {
            return Ok(None);
        }
        let first_source_frame = self.cursor;
        let frames = (self.total - self.cursor).min(PACKET_FRAMES as u64) as usize;
        let payload = if frames == 0 {
            self.ended = true;
            DecoderPayload::End
        } else {
            let mut bytes = [0; PACKET_FRAMES * BYTES_PER_FRAME];
            let pcm = &mut bytes[..frames * BYTES_PER_FRAME];
            self.reader.read_exact(pcm)?;
            let mut samples = Vec::new();
            samples
                .try_reserve_exact(frames * 2)
                .map_err(io::Error::other)?;
            samples.extend(
                pcm.chunks_exact(2)
                    .map(|pair| f32::from(i16::from_le_bytes([pair[0], pair[1]])) / 32768.0),
            );
            self.cursor += frames as u64;
            DecoderPayload::Samples(samples)
        };
        Ok(Some(DecodedPacket {
            generation: self.generation,
            first_source_frame,
            payload,
        }))
    }
}

#[cfg(test)]
mod tests;
