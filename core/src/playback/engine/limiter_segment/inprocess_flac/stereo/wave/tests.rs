use super::*;
use crate::playback::engine::pcm_wave::tests::wave;
use std::io::Cursor;

#[test]
fn pcm_seek_sign_and_short_end_are_exact() {
    let mut reader = Cursor::new(wave([[1024, -1024], [-32768, 32767], [-1, 1]]));
    let layout = PcmWave::inspect(&mut reader).unwrap();
    let mut decoder = WavePackets::open(&mut reader, layout, 1, Generation(2)).unwrap();
    let packet = decoder.next().unwrap().unwrap();
    assert_eq!(packet.generation, Generation(2));
    assert_eq!(packet.first_source_frame, 1);
    let DecoderPayload::Samples(pcm) = packet.payload else {
        panic!("missing PCM")
    };
    assert_eq!(
        pcm.iter()
            .map(|sample| sample.to_bits())
            .collect::<Vec<_>>(),
        [0xbf80_0000, 0x3f7f_fe00, 0xb800_0000, 0x3800_0000]
    );
    let end = decoder.next().unwrap().unwrap();
    assert_eq!(end.first_source_frame, 3);
    assert!(matches!(end.payload, DecoderPayload::End));
    assert!(decoder.next().unwrap().is_none());

    // A physical short read after qualification is a failure, not a fake EOF.
    reader.get_mut().pop();
    let mut decoder = WavePackets::open(&mut reader, layout, 1, Generation(2)).unwrap();
    assert!(matches!(decoder.next(), Err(error) if error.kind() == io::ErrorKind::UnexpectedEof));
}
