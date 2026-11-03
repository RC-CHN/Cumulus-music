//! A deliberately small, nonfragmented, local-data-only M4A subset.
//! This parser is admission control; libmpv separately denies referenced opens.
use crate::{Error, Result};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    ops::Range,
};

const MAX_BOXES: usize = 2_048;
const MAX_METADATA: u64 = 16 * 1024 * 1024;
const REJECT: Error = Error::InvalidInput("unsupported pinned audio format");

struct Atom {
    kind: [u8; 4],
    data: Range<u64>,
}

struct Reader<'a, R> {
    file: &'a mut R,
    remaining: usize,
}

impl<R: Read + Seek> Reader<'_, R> {
    fn at<const N: usize>(&mut self, data: &Range<u64>, offset: u64) -> Result<[u8; N]> {
        let start = data.start.checked_add(offset).ok_or(REJECT)?;
        if start.checked_add(N as u64).ok_or(REJECT)? > data.end {
            return Err(REJECT);
        }
        let mut result = [0; N];
        self.file.seek(SeekFrom::Start(start))?;
        self.file.read_exact(&mut result)?;
        Ok(result)
    }

    fn children(&mut self, data: Range<u64>) -> Result<Vec<Atom>> {
        let mut children = Vec::new();
        let mut position = data.start;
        while position < data.end {
            self.remaining = self.remaining.checked_sub(1).ok_or(REJECT)?;
            let rest = position..data.end;
            let head = self.at::<8>(&rest, 0)?;
            let mut size = u64::from(u32::from_be_bytes(head[..4].try_into().unwrap()));
            let header = if size == 1 {
                size = u64::from_be_bytes(self.at::<8>(&rest, 8)?);
                16
            } else {
                8
            };
            // Zero-to-end boxes are intentionally outside this subset.
            let end = position.checked_add(size).ok_or(REJECT)?;
            if size < header || end > data.end {
                return Err(REJECT);
            }
            children.push(Atom {
                kind: head[4..8].try_into().unwrap(),
                data: position + header..end,
            });
            position = end;
        }
        Ok(children)
    }

    fn boxes(&mut self, data: Range<u64>, allow: &[[u8; 4]]) -> Result<Vec<Atom>> {
        let children = self.children(data)?;
        if children.iter().any(|atom| !allow.contains(&atom.kind)) {
            return Err(REJECT);
        }
        Ok(children)
    }

    fn table(&mut self, atom: &Atom, width: u64) -> Result<u64> {
        let header = self.at::<8>(&atom.data, 0)?;
        if header[..4] != [0; 4] {
            return Err(REJECT);
        }
        let count = u64::from(u32::from_be_bytes(header[4..].try_into().unwrap()));
        if count == 0 || 8 + width * count != atom.data.end - atom.data.start {
            return Err(REJECT);
        }
        Ok(count)
    }

    fn metadata(&mut self, data: Range<u64>) -> Result<()> {
        let children = self.boxes(data, &[*b"meta", *b"free"])?;
        let Some(meta) = optional(&children, b"meta")? else {
            return Ok(());
        };
        if self.at::<4>(&meta.data, 0)? != [0; 4] {
            return Err(REJECT);
        }
        let children = self.boxes(
            meta.data.start + 4..meta.data.end,
            &[*b"hdlr", *b"ilst", *b"free"],
        )?;
        if self.at::<4>(&one(&children, b"hdlr")?.data, 8)? != *b"mdir" {
            return Err(REJECT);
        }
        if let Some(items) = optional(&children, b"ilst")? {
            for item in self.children(items.data.clone())? {
                let freeform = item.kind == *b"----";
                if !freeform
                    && !matches!(
                        &item.kind,
                        b"\xa9nam"
                            | b"\xa9ART"
                            | b"aART"
                            | b"\xa9alb"
                            | b"trkn"
                            | b"disk"
                            | b"\xa9too"
                            | b"covr"
                            | b"\xa9lyr"
                            | b"\xa9cmt"
                            | b"\xa9day"
                            | b"\xa9gen"
                            | b"gnre"
                            | b"cpil"
                            | b"tmpo"
                            | b"pgap"
                    )
                {
                    return Err(REJECT);
                }
                const FREEFORM: &[[u8; 4]] = &[*b"data", *b"name", *b"mean"];
                const VALUE: &[[u8; 4]] = &[*b"data"];
                let fields = self.boxes(item.data, if freeform { FREEFORM } else { VALUE })?;
                if freeform {
                    one(&fields, b"name")?;
                    one(&fields, b"mean")?;
                }
                if !fields.iter().any(|atom| atom.kind == *b"data") {
                    return Err(REJECT);
                }
            }
        }
        Ok(())
    }

    fn sample_table(&mut self, data: Range<u64>, media: &Range<u64>) -> Result<()> {
        let children = self.boxes(
            data,
            &[
                *b"stsd", *b"stts", *b"stsc", *b"stsz", *b"stco", *b"co64", *b"sgpd", *b"sbgp",
            ],
        )?;
        for key in [b"stts", b"stsz"] {
            one(&children, key)?;
        }
        let desc = one(&children, b"stsd")?;
        if self.at::<8>(&desc.data, 0)? != [0, 0, 0, 0, 0, 0, 0, 1] {
            return Err(REJECT);
        }
        let entries = self.boxes(desc.data.start + 8..desc.data.end, &[*b"mp4a"])?;
        let entry = one(&entries, b"mp4a")?;
        // Reserved bytes, local dref index 1, version-0 audio entry.
        if self.at::<10>(&entry.data, 0)? != [0, 0, 0, 0, 0, 0, 0, 1, 0, 0]
            || entry.data.end - entry.data.start < 28
        {
            return Err(REJECT);
        }
        let config = self.boxes(entry.data.start + 28..entry.data.end, &[*b"esds", *b"btrt"])?;
        self.aac_descriptor(one(&config, b"esds")?)?;
        optional(&config, b"btrt")?;

        let stsc = one(&children, b"stsc")?;
        let (offsets, wide) = match (optional(&children, b"stco")?, optional(&children, b"co64")?) {
            (Some(atom), None) => (atom, false),
            (None, Some(atom)) => (atom, true),
            _ => return Err(REJECT),
        };
        let chunks = self.table(offsets, if wide { 8 } else { 4 })?;
        let mappings = self.table(stsc, 12)?;
        // Bound I/O work even for a large table with tiny entries.
        if chunks > 16_384 || mappings > chunks {
            return Err(REJECT);
        }
        let mut previous = 0;
        self.file.seek(SeekFrom::Start(stsc.data.start + 8))?;
        let mut reader = BufReader::new(&mut *self.file);
        for i in 0..mappings {
            let mut entry = [0; 12];
            reader.read_exact(&mut entry)?;
            let chunk = u64::from(u32::from_be_bytes(entry[..4].try_into().unwrap()));
            if (i == 0 && chunk != 1)
                || chunk <= previous
                || chunk > chunks
                || entry[4..8] == [0; 4]
                || entry[8..] != [0, 0, 0, 1]
            {
                return Err(REJECT);
            }
            previous = chunk;
        }
        self.file.seek(SeekFrom::Start(offsets.data.start + 8))?;
        let mut reader = BufReader::new(&mut *self.file);
        for _ in 0..chunks {
            let mut offset = [0; 8];
            reader.read_exact(if wide { &mut offset } else { &mut offset[4..] })?;
            let position = u64::from_be_bytes(offset);
            if !media.contains(&position) {
                return Err(REJECT);
            }
        }
        Ok(())
    }

    fn aac_descriptor(&mut self, esds: &Atom) -> Result<()> {
        let length = esds.data.end - esds.data.start;
        if !(4..=256).contains(&length) {
            return Err(REJECT);
        }
        let mut buffer = [0; 256];
        self.file.seek(SeekFrom::Start(esds.data.start))?;
        self.file.read_exact(&mut buffer[..length as usize])?;
        if buffer[..4] != [0; 4] {
            return Err(REJECT);
        }
        let (es, leftover) = descriptor(&buffer[4..length as usize], 3)?;
        // No dependent stream, URL or OCR fields are admitted.
        if !leftover.is_empty() || es.get(2) != Some(&0) {
            return Err(REJECT);
        }
        let (decoder, es_tail) = descriptor(&es[3..], 4)?;
        if decoder.len() < 13 || decoder[0] != 0x40 || decoder[1] >> 2 != 5 {
            return Err(REJECT);
        }
        let (specific, decoder_tail) = descriptor(&decoder[13..], 5)?;
        let (sl, es_tail) = descriptor(es_tail, 6)?;
        if specific.len() < 2 || !decoder_tail.is_empty() || sl != [2] || !es_tail.is_empty() {
            return Err(REJECT);
        }
        Ok(())
    }

    fn track(&mut self, data: Range<u64>, media: &Range<u64>) -> Result<()> {
        let children = self.boxes(data, &[*b"tkhd", *b"mdia", *b"edts"])?;
        one(&children, b"tkhd")?;
        if let Some(edit) = optional(&children, b"edts")? {
            one(&self.boxes(edit.data.clone(), &[*b"elst"])?, b"elst")?;
        }
        let children = self.boxes(
            one(&children, b"mdia")?.data.clone(),
            &[*b"mdhd", *b"hdlr", *b"minf"],
        )?;
        one(&children, b"mdhd")?;
        if self.at::<4>(&one(&children, b"hdlr")?.data, 8)? != *b"soun" {
            return Err(REJECT);
        }
        let children = self.boxes(
            one(&children, b"minf")?.data.clone(),
            &[*b"smhd", *b"dinf", *b"stbl"],
        )?;
        one(&children, b"smhd")?;
        let references = self.boxes(one(&children, b"dinf")?.data.clone(), &[*b"dref"])?;
        let dref = one(&references, b"dref")?;
        if self.at::<8>(&dref.data, 0)? != [0, 0, 0, 0, 0, 0, 0, 1] {
            return Err(REJECT);
        }
        let entries = self.boxes(dref.data.start + 8..dref.data.end, &[*b"url "])?;
        let local = one(&entries, b"url ")?;
        if local.data.end - local.data.start != 4 || self.at::<4>(&local.data, 0)? != [0, 0, 0, 1] {
            return Err(REJECT);
        }
        self.sample_table(one(&children, b"stbl")?.data.clone(), media)
    }

    fn validate(&mut self, length: u64) -> Result<()> {
        let top = self.boxes(0..length, &[*b"ftyp", *b"moov", *b"mdat", *b"free"])?;
        let ftyp = one(&top, b"ftyp")?;
        if top.first().is_none_or(|atom| atom.kind != *b"ftyp")
            || !matches!(
                &self.at::<4>(&ftyp.data, 0)?,
                b"M4A " | b"isom" | b"mp41" | b"mp42"
            )
            || !(8..=256).contains(&(ftyp.data.end - ftyp.data.start))
            || !(ftyp.data.end - ftyp.data.start).is_multiple_of(4)
        {
            return Err(REJECT);
        }
        let media = &one(&top, b"mdat")?.data;
        let moov = &one(&top, b"moov")?.data;
        if moov.end - moov.start > MAX_METADATA {
            return Err(REJECT);
        }
        let children = self.boxes(moov.clone(), &[*b"mvhd", *b"trak", *b"udta", *b"free"])?;
        one(&children, b"mvhd")?;
        self.track(one(&children, b"trak")?.data.clone(), media)?;
        if let Some(metadata) = optional(&children, b"udta")? {
            self.metadata(metadata.data.clone())?;
        }
        Ok(())
    }
}

fn descriptor(data: &[u8], kind: u8) -> Result<(&[u8], &[u8])> {
    if data.first() != Some(&kind) {
        return Err(REJECT);
    }
    let mut length = 0;
    for i in 1..=4 {
        let byte = *data.get(i).ok_or(REJECT)?;
        length = (length << 7) | usize::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            let rest = &data[i + 1..];
            return rest.split_at_checked(length).ok_or(REJECT);
        }
    }
    Err(REJECT)
}

fn optional<'a>(boxes: &'a [Atom], kind: &[u8; 4]) -> Result<Option<&'a Atom>> {
    let mut found = boxes.iter().filter(|atom| &atom.kind == kind);
    let first = found.next();
    if found.next().is_some() {
        return Err(REJECT);
    }
    Ok(first)
}

fn one<'a>(boxes: &'a [Atom], kind: &[u8; 4]) -> Result<&'a Atom> {
    optional(boxes, kind)?.ok_or(REJECT)
}

pub(super) fn validate(file: &mut File) -> Result<()> {
    let length = file.metadata()?.len();
    validate_stream(file, length)
}

/// The declared length is the bounded logical stream, not the backing file.
pub(super) fn validate_stream(file: &mut (impl Read + Seek), length: u64) -> Result<()> {
    let result = Reader {
        file,
        remaining: MAX_BOXES,
    }
    .validate(length);
    file.rewind()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const AUDIO: &[u8] = include_bytes!("../../../tests/fixtures/tag_audio/seed.m4a");

    fn fixture(bytes: &[u8]) -> File {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(bytes).unwrap();
        file
    }

    fn field(bytes: &[u8], tag: &[u8; 4]) -> usize {
        bytes.windows(4).position(|value| value == tag).unwrap()
    }

    #[test]
    fn admits_the_frozen_local_audio_and_rewinds() {
        let mut file = fixture(AUDIO);
        assert!(validate(&mut file).is_ok());
        assert_eq!(file.stream_position().unwrap(), 0);
    }

    #[test]
    fn refuses_external_reference_types_and_nonlocal_sample_descriptions() {
        let mut external = AUDIO.to_vec();
        let url = field(&external, b"url ");
        external[url + 7] = 0;
        assert!(matches!(
            validate(&mut fixture(&external)),
            Err(Error::InvalidInput(_))
        ));
        for alias in [b"alis", b"urn "] {
            external[url..url + 4].copy_from_slice(alias);
            assert!(matches!(
                validate(&mut fixture(&external)),
                Err(Error::InvalidInput(_))
            ));
        }
        let mut wrong_index = AUDIO.to_vec();
        let index = field(&wrong_index, b"mp4a") + 11;
        wrong_index[index] = 2;
        assert!(matches!(
            validate(&mut fixture(&wrong_index)),
            Err(Error::InvalidInput(_))
        ));
    }

    #[test]
    fn refuses_fragmented_malformed_and_out_of_media_chunks() {
        for changed in [b"moof", b"cmov", b"uuid"] {
            let mut bytes = AUDIO.to_vec();
            let atom = field(&bytes, b"moov");
            bytes[atom..atom + 4].copy_from_slice(changed);
            assert!(matches!(
                validate(&mut fixture(&bytes)),
                Err(Error::InvalidInput(_))
            ));
        }
        let mut bytes = AUDIO.to_vec();
        let chunk = field(&bytes, b"stco") + 12;
        bytes[chunk..chunk + 4].fill(0);
        assert!(matches!(
            validate(&mut fixture(&bytes)),
            Err(Error::InvalidInput(_))
        ));
        let mut bytes = AUDIO.to_vec();
        let atom = field(&bytes, b"moov");
        bytes[atom - 4..atom].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(matches!(
            validate(&mut fixture(&bytes)),
            Err(Error::InvalidInput(_))
        ));
    }
}
