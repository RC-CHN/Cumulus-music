use super::*;
use std::io::Cursor;

fn riff(chunks: &[([u8; 4], &[u8])]) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
    for (id, content) in chunks {
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(&(content.len() as u32).to_le_bytes());
        bytes.extend_from_slice(content);
        if !content.len().is_multiple_of(2) {
            bytes.push(0);
        }
    }
    let riff_size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&riff_size.to_le_bytes());
    bytes
}

const PCM_FORMAT: [u8; 16] = [1, 0, 2, 0, 0x44, 0xac, 0, 0, 0x10, 0xb1, 2, 0, 4, 0, 16, 0];

pub(crate) fn wave(samples: impl IntoIterator<Item = [i16; 2]>) -> Vec<u8> {
    let payload: Vec<_> = samples
        .into_iter()
        .flatten()
        .flat_map(i16::to_le_bytes)
        .collect();
    riff(&[(*b"fmt ", &PCM_FORMAT), (*b"data", &payload)])
}

#[test]
fn signed_pcm_requires_one_bounded_stream_and_honors_riff_padding() {
    let source = riff(&[
        (*b"JUNK", b"pad"),
        (*b"fmt ", &PCM_FORMAT),
        (*b"LIST", b"INFO"),
        (*b"data", &[0; 12]),
    ]);
    let mut reader = Cursor::new(source.clone());
    let layout = PcmWave::inspect(&mut reader).unwrap();
    assert_eq!(layout.frames, 3);
    assert_eq!(reader.position(), 0);
    let last = layout.byte_offset(2).unwrap() as usize;
    assert_eq!(last + 4, source.len());
    assert!(layout.byte_offset(3).is_err());
    assert!(layout.byte_offset(u64::MAX).is_err());

    let mut truncated = source.clone();
    truncated.pop();
    let mut impossible = source.clone();
    impossible[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut wrong_magic = source;
    wrong_magic[..4].copy_from_slice(b"RF64");
    let mut bad_alignment = PCM_FORMAT;
    bad_alignment[12] = 2;
    let mut bad_rate = PCM_FORMAT;
    bad_rate[4..8].copy_from_slice(&48_000u32.to_le_bytes());
    let mut extensible = PCM_FORMAT;
    extensible[..2].copy_from_slice(&0xfffeu16.to_le_bytes());
    for invalid in [
        truncated,
        impossible,
        wrong_magic,
        riff(&[(*b"fmt ", &bad_alignment), (*b"data", &[0; 8])]),
        riff(&[(*b"fmt ", &bad_rate), (*b"data", &[0; 8])]),
        riff(&[(*b"fmt ", &extensible), (*b"data", &[0; 8])]),
        riff(&[(*b"fmt ", &PCM_FORMAT), (*b"data", &[0; 7])]),
        riff(&[(*b"fmt ", &PCM_FORMAT), (*b"data", &[])]),
        riff(&[
            (*b"fmt ", &PCM_FORMAT),
            (*b"data", &[0; 4]),
            (*b"data", &[0; 4]),
        ]),
        riff(&[
            (*b"fmt ", &PCM_FORMAT),
            (*b"fmt ", &PCM_FORMAT),
            (*b"data", &[0; 4]),
        ]),
        riff(&[
            (*b"fmt ", &PCM_FORMAT),
            (*b"LIST", b"wavl"),
            (*b"data", &[0; 4]),
        ]),
    ] {
        assert!(PcmWave::inspect(&mut Cursor::new(invalid)).is_err());
    }
    let mut many = vec![(*b"JUNK", b"".as_slice()); MAX_CHUNKS];
    many.extend([
        (*b"fmt ", PCM_FORMAT.as_slice()),
        (*b"data", [0; 4].as_slice()),
    ]);
    assert!(PcmWave::inspect(&mut Cursor::new(riff(&many))).is_err());
}
