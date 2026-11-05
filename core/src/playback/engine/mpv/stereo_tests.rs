use super::*;
use crate::playback::engine::stereo_pcm::channel;

#[test]
fn already_destroyed_native_sink_does_not_strand_unpublished_pcm_ownership() {
    let mut engine = MpvEngine {
        native: None,
        equalizer: None,
        equalizer_reserved: false,
        loudness: None,
        loudness_reserved: false,
        parametric: None,
        equalizer_library: None,
        sequence: 0,
        events: VecDeque::new(),
        entry: None,
        null_output: true,
        api_version: 0,
        pinned_authority: None,
        pinned_bmff_options: false,
        stereo_pcm: None,
    };
    let (reader, control, _) = channel(44100, 0, 5000, Cancellation::default(), None).unwrap();
    assert!(matches!(
        engine.load_stereo_pcm(reader, &Cancellation::default()),
        Err(Error::Playback {
            operation: "native sink retired",
            ..
        })
    ));
    let state = control.snapshot().unwrap();
    assert!(state.reader_closed && state.native_sink_retired);
    assert!(state.native_entry.is_none());
}
