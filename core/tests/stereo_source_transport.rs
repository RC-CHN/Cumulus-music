use cumulus_core::{
    Error,
    effects::stereo::LocalStereoMaterialPlan,
    playback::engine::{Cancellation, StereoRead, StereoSourceTransport},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
fn plan(presence: i32) -> LocalStereoMaterialPlan {
    LocalStereoMaterialPlan::from_equalizer_json(&json!({"eq":{"on":false,"eqs":vec![0;10]},"bt":{"on":false,"bass":0,"treble":0},"rvb":{"on":false},"se":{"on":true,"presence":presence,"stereoizer":5,"sshaper":true,"ambience":1}})).unwrap()
}
fn samples(first: u64, frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|i| {
            [
                ((first + i as u64) % 31) as f32 * 0.0078125 - 0.125,
                ((first + i as u64) % 17) as f32 * 0.015625 - 0.125,
            ]
        })
        .collect()
}
#[test]
fn generation_seek_discards_old_pcm_and_tail_does_not_advance_held_source_clock() {
    let mut source =
        StereoSourceTransport::new(48000, 2, plan(3), 0, 5000, Cancellation::default()).unwrap();
    let old = source.start(0).unwrap();
    let mut pcm = [0.0; 128];
    assert_eq!(source.read(old, &mut pcm).unwrap(), StereoRead::Pending);
    assert!(source.offer_samples(old, 0, &samples(0, 2048)).unwrap());
    assert!(
        matches!(source.read(old, &mut pcm).unwrap(),StereoRead::Audio{generation,first_source_frame:0,..} if generation==old)
    );
    let new = source.start(1024).unwrap();
    assert_ne!(new, old);
    assert_eq!(source.read(new, &mut pcm).unwrap(), StereoRead::Pending);
    assert!(!source.offer_samples(old, u64::MAX, &[f32::NAN]).unwrap());
    assert!(source.offer_end(new, 1024).is_err());
    assert!(source.offer_samples(new, 0, &samples(0, 100)).is_err());
    assert!(
        source
            .offer_samples(new, 1024, &samples(1024, 2048))
            .unwrap()
    );
    source.configure(plan(-3)).unwrap();
    assert!(
        source
            .offer_samples(new, 3072, &samples(3072, 1928))
            .unwrap()
    );
    assert!(source.offer_end(new, 5000).unwrap());
    let mut total = 0;
    let mut clock = 1024;
    let mut tail_after_source_end = false;
    for _ in 0..1000 {
        match source.read(new, &mut pcm).unwrap() {
            StereoRead::Audio {
                generation,
                frames,
                source_clock,
                ..
            } => {
                assert_eq!(generation, new);
                assert!(source_clock >= clock && source_clock <= 5000);
                clock = source_clock;
                total += u64::from(frames);
                tail_after_source_end |= source_clock == 5000;
            }
            StereoRead::End {
                generation,
                source_clock,
            } => {
                assert_eq!(generation, new);
                assert_eq!(source_clock, 5000);
                break;
            }
            StereoRead::Pending => panic!("verified complete decoder EOF must drain"),
        }
    }
    assert_eq!(total, 5000 - 1024 + 48000);
    assert!(tail_after_source_end);
    assert!(source.start(5001).is_err());
}
#[test]
fn revoked_authority_zeros_already_buffered_output_and_cannot_be_restarted() {
    let active = Arc::new(AtomicBool::new(true));
    let check = active.clone();
    let authority = Cancellation::new(move || check.load(Ordering::SeqCst));
    let mut source = StereoSourceTransport::new(48000, 2, plan(3), 0, 2048, authority).unwrap();
    let generation = source.start(0).unwrap();
    source
        .offer_samples(generation, 0, &samples(0, 2048))
        .unwrap();
    active.store(false, Ordering::SeqCst);
    let mut pcm = [0.5; 128];
    assert!(source.read(generation, &mut pcm).is_err());
    assert_eq!(pcm, [0.0; 128]);
    active.store(true, Ordering::SeqCst);
    assert!(source.start(0).is_err());
    assert!(source.offer_end(generation, 2048).is_err());
}

#[test]
fn independent_transport_generations_cannot_offer_or_consume_each_others_pcm() {
    let make =
        || StereoSourceTransport::new(48000, 2, plan(3), 0, 2048, Cancellation::default()).unwrap();
    let mut first = make();
    let mut second = make();
    let a = first.start(0).unwrap();
    let b = second.start(0).unwrap();
    assert_ne!(a, b);
    assert!(!second.offer_samples(a, 0, &samples(0, 2048)).unwrap());
    assert!(!second.offer_end(a, 0).unwrap());
    assert!(second.offer_samples(b, 0, &samples(0, 2048)).unwrap());
    let mut pcm = [0.5; 128];
    assert!(matches!(
        second.read(a, &mut pcm),
        Err(Error::StaleResponse)
    ));
    assert_eq!(pcm, [0.0; 128]);
    assert!(matches!(
        second.read(b, &mut pcm).unwrap(),
        StereoRead::Audio { generation, first_source_frame: 0, .. } if generation == b
    ));
    assert!(first.offer_samples(a, 0, &samples(0, 2048)).unwrap());
}

#[test]
fn retired_sink_generation_cannot_pop_new_seek_output_or_advance_its_clock() {
    let mut source =
        StereoSourceTransport::new(48000, 2, plan(3), 0, 4096, Cancellation::default()).unwrap();
    let old = source.start(0).unwrap();
    source.offer_samples(old, 0, &samples(0, 2048)).unwrap();
    let new = source.start(2048).unwrap();
    source
        .offer_samples(new, 2048, &samples(2048, 2048))
        .unwrap();
    let mut pcm = [0.5; 128];
    assert!(matches!(
        source.read(old, &mut pcm),
        Err(Error::StaleResponse)
    ));
    assert_eq!(pcm, [0.0; 128]);
    assert!(matches!(
        source.read(new, &mut pcm).unwrap(),
        StereoRead::Audio {
            generation,
            first_source_frame: 2048,
            frames: 64,
            source_clock: 2112,
        } if generation == new
    ));
}
#[test]
fn decoder_overrun_nonfinite_packet_and_incomplete_eof_are_atomic_rejections() {
    assert!(
        StereoSourceTransport::new(48000, 1, plan(3), 0, 2048, Cancellation::default()).is_err()
    );
    let mut source =
        StereoSourceTransport::new(48000, 2, plan(3), 100, 2100, Cancellation::default()).unwrap();
    assert!(source.start(0).is_err());
    let generation = source.start(100).unwrap();
    assert!(
        source
            .offer_samples(generation, 100, &[f32::NAN, 0.0])
            .is_err()
    );
    assert!(
        source
            .offer_samples(generation, 100, &samples(100, 2001))
            .is_err()
    );
    assert!(source.offer_end(generation, 100).is_err());
    assert!(
        source
            .offer_samples(generation, 100, &samples(100, 2000))
            .unwrap()
    );
    assert!(source.offer_end(generation, 2100).unwrap());
}
