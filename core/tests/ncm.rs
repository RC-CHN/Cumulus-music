use base64::{Engine, engine::general_purpose::STANDARD};
use cumulus_core::{
    Error,
    media::{ncm::*, validate_cover},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{self, Cursor, Read, Seek, SeekFrom};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ncm.json")).unwrap()
}
fn decode(value: &Value) -> Vec<u8> {
    STANDARD.decode(value.as_str().unwrap()).unwrap()
}

#[test]
fn independent_cpp_vectors_stream_and_seek_at_nonperiodic_boundaries() {
    for row in fixture()["rows"].as_array().unwrap() {
        let data = decode(&row["ncm"]);
        let expected = decode(&row["audio"]);
        let mut reader = NcmReader::new(Cursor::new(&data), HeaderPolicy::Compatible).unwrap();
        assert_eq!(
            serde_json::to_value(reader.info().audio_format).unwrap(),
            row["format"]
        );
        assert_eq!(reader.info().crc_verified, row["strict"]);
        for block in [1, 13, 255, 256, 257, 65537] {
            reader.rewind().unwrap();
            let mut output = Vec::new();
            let mut chunk = vec![0; block];
            loop {
                let n = reader.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                output.extend_from_slice(&chunk[..n]);
            }
            assert_eq!(output, expected, "{} block {block}", row["name"]);
            assert_eq!(hex::encode(Sha256::digest(&output)), row["sha256"]);
        }
        for pos in [
            0,
            1,
            3,
            expected.len() / 2,
            expected.len() - 1,
            expected.len(),
        ] {
            reader.seek(SeekFrom::Start(pos as u64)).unwrap();
            let mut chunk = vec![0; 521];
            let n = reader.read(&mut chunk).unwrap();
            assert_eq!(&chunk[..n], &expected[pos..(pos + n)]);
        }
        reader.seek(SeekFrom::End(-3)).unwrap();
        reader.seek(SeekFrom::Current(1)).unwrap();
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, &expected[expected.len() - 2..]);
        assert!(reader.seek(SeekFrom::End(1)).is_err());
        assert!(reader.seek(SeekFrom::Current(-10000)).is_err());
        assert_eq!(
            NcmReader::new(Cursor::new(data), HeaderPolicy::Strict).is_ok(),
            row["strict"]
        );
    }
}

#[test]
fn writer_matches_legacy_bytes_and_strict_native_header_crc() {
    let fixture = fixture();
    let row = &fixture["writer"];
    let key = decode(&row["key"]);
    let cover = decode(&row["cover"]);
    let audio = decode(&row["audio"]);
    let mut out = Vec::new();
    let report = write_stream(
        &mut Cursor::new(&audio),
        &mut out,
        NcmWriteOptions {
            metadata: Some(&row["metadata"]),
            cover: &cover,
            audio_key: Some(&key),
        },
    )
    .unwrap();
    assert_eq!(out, decode(&row["expected_ncm"]));
    assert_eq!(report.audio_sha256, hex::encode(Sha256::digest(&audio)));
    let mut reader = NcmReader::new(Cursor::new(&out), HeaderPolicy::Strict).unwrap();
    assert_eq!(reader.info().metadata.as_ref(), Some(&row["metadata"]));
    assert_eq!(reader.cover(), cover);
    let mut restored = Vec::new();
    reader.read_to_end(&mut restored).unwrap();
    assert_eq!(restored, audio);
    assert_eq!(reader.info().crc32_field, report.header_crc32);
    let mut corrupted = out;
    corrupted[9] ^= 1;
    assert!(NcmReader::new(Cursor::new(&corrupted), HeaderPolicy::Strict).is_err());
    assert!(
        !NcmReader::new(Cursor::new(corrupted), HeaderPolicy::Compatible)
            .unwrap()
            .info()
            .crc_verified
    );
}

#[test]
fn invalid_fields_metadata_truncation_and_unknown_export_are_rejected() {
    let fixture = fixture();
    let data = decode(&fixture["writer"]["expected_ncm"]);
    for cut in [0, 7, 10, 13, 17, 30] {
        assert!(NcmReader::new(Cursor::new(&data[..cut]), HeaderPolicy::Compatible).is_err());
    }
    for n in [0, 17, u32::MAX] {
        let mut broken = data.clone();
        broken[10..14].copy_from_slice(&n.to_le_bytes());
        assert!(NcmReader::new(Cursor::new(broken), HeaderPolicy::Compatible).is_err());
    }
    let key_size = u32::from_le_bytes(data[10..14].try_into().unwrap()) as usize;
    let meta_start = 18 + key_size;
    for offset in [0, 14, meta_start, meta_start + 21] {
        let mut broken = data.clone();
        broken[offset] ^= 0xff;
        assert!(NcmReader::new(Cursor::new(broken), HeaderPolicy::Compatible).is_err());
    }
    let meta_size =
        u32::from_le_bytes(data[14 + key_size..18 + key_size].try_into().unwrap()) as usize;
    let cover_header = meta_start + meta_size + 5;
    for (offset, n) in [(cover_header, u32::MAX), (cover_header + 4, u32::MAX)] {
        let mut broken = data.clone();
        broken[offset..offset + 4].copy_from_slice(&n.to_le_bytes());
        assert!(NcmReader::new(Cursor::new(broken), HeaderPolicy::Compatible).is_err());
    }
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("unknown.ncm");
    let output = tmp.path().join("output.flac");
    let unknown = fixture["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "unknown")
        .unwrap();
    std::fs::write(&input, decode(&unknown["ncm"])).unwrap();
    assert!(matches!(
        extract(input, &output, HeaderPolicy::Compatible),
        Err(Error::Media(_))
    ));
    assert!(!output.exists());
}

#[test]
fn filesystem_publication_is_no_clobber_and_failed_writer_removes_staging() {
    let tmp = tempfile::tempdir().unwrap();
    let fixture = fixture();
    let audio = decode(&fixture["writer"]["audio"]);
    let input = tmp.path().join("source.flac");
    let output = tmp.path().join("output.ncm");
    let exported = tmp.path().join("export.flac");
    std::fs::write(&input, &audio).unwrap();
    write_file(&input, &output, NcmWriteOptions::default()).unwrap();
    let existing = std::fs::read(&output).unwrap();
    assert!(write_file(&input, &output, NcmWriteOptions::default()).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), existing);
    extract(&output, &exported, HeaderPolicy::Strict).unwrap();
    assert_eq!(std::fs::read(&exported).unwrap(), audio);
    for target in [&exported, &output] {
        let before = std::fs::read(target).unwrap();
        assert!(extract(&output, target, HeaderPolicy::Strict).is_err());
        assert_eq!(std::fs::read(target).unwrap(), before);
    }
    let empty = tmp.path().join("empty");
    std::fs::write(&empty, []).unwrap();
    let failed = tmp.path().join("failed.ncm");
    assert!(write_file(&empty, &failed, NcmWriteOptions::default()).is_err());
    assert!(!failed.exists());
    assert!(!std::fs::read_dir(tmp.path()).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".cumulus-ncm-")
    }));
    assert_eq!(std::fs::read(input).unwrap(), audio);
}

#[test]
fn writer_validates_cover_and_key_before_output_and_handles_short_reads() {
    let fixture = fixture();
    let cover = decode(&fixture["writer"]["cover"]);
    let info = validate_cover(&cover).unwrap();
    assert_eq!((info.width, info.height, info.mime), (2, 2, "image/png"));
    for bytes in [&b"bad"[..], &cover[..20]] {
        assert!(validate_cover(bytes).is_err());
    }
    let mut broken = cover.clone();
    let last = broken.len() - 1;
    broken[last] ^= 255;
    assert!(validate_cover(&broken).is_err());
    for key in [vec![], vec![1; 257]] {
        let mut out = Vec::new();
        assert!(
            write_stream(
                &mut Cursor::new(b"fLaCdata"),
                &mut out,
                NcmWriteOptions {
                    audio_key: Some(&key),
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert!(out.is_empty());
    }
    let mut out = Vec::new();
    assert!(
        write_stream(
            &mut Cursor::new(b"fLaCdata"),
            &mut out,
            NcmWriteOptions {
                metadata: Some(&json!([])),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(out.is_empty());
    struct Short(Cursor<Vec<u8>>);
    impl Read for Short {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = out.len().min(13);
            self.0.read(&mut out[..n])
        }
    }
    let audio = decode(&fixture["writer"]["audio"]);
    let mut out = Vec::new();
    write_stream(
        &mut Short(Cursor::new(audio.clone())),
        &mut out,
        NcmWriteOptions::default(),
    )
    .unwrap();
    let mut reader = NcmReader::new(Short(Cursor::new(out)), HeaderPolicy::Strict);
    // Read+Seek is required to address the payload, supplied by this wrapper.
    let mut decoded = Vec::new();
    reader.as_mut().unwrap().read_to_end(&mut decoded).unwrap();
    assert_eq!(decoded, audio);
    impl Seek for Short {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.0.seek(pos)
        }
    }
}
