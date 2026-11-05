use cumulus_core::effects::dsp::{BassTreble, Format, GraphicEq, ParametricEq, PlanarBlock};
fn block(format: Format) -> PlanarBlock {
    let mut block = PlanarBlock::new(format);
    for (i, value) in block.samples_mut().iter_mut().enumerate() {
        *value = ((i as f32) * 0.137).sin() * 0.1;
    }
    block
}
#[test]
fn actual_reset_synchronizes_pending_off_edges_without_an_empty_settle_process() {
    let format = Format::new(48000.0, 2, 64).unwrap();
    let original = block(format);
    let mut eq = GraphicEq::new(format).unwrap();
    eq.set_bypass(true).unwrap();
    eq.reset().unwrap();
    let mut bt = BassTreble::new(format).unwrap();
    bt.set_bypass(true).unwrap();
    bt.reset().unwrap();
    let mut peq = ParametricEq::new(format, false, true).unwrap();
    peq.set_bypass(true).unwrap();
    peq.reset().unwrap();
    let mut pcm = block(format);
    eq.process(&mut pcm, 7).unwrap();
    assert_eq!(pcm.samples(), original.samples());
    bt.process(&mut pcm, 7).unwrap();
    assert_eq!(pcm.samples(), original.samples());
    peq.process(&mut pcm, 7).unwrap();
    assert_eq!(pcm.samples(), original.samples());
}
#[test]
fn peq_reset_preserves_an_already_applied_gain_instead_of_restarting_its_ramp() {
    let format = Format::new(48000.0, 1, 64).unwrap();
    let mut warm = ParametricEq::new(format, false, true).unwrap();
    warm.set_gain(-6.0).unwrap();
    let mut pcm = block(format);
    warm.process(&mut pcm, 64).unwrap();
    warm.reset().unwrap();
    let mut fresh = ParametricEq::new(format, false, true).unwrap();
    fresh.set_gain(-6.0).unwrap();
    let mut a = block(format);
    let mut b = block(format);
    warm.process(&mut a, 64).unwrap();
    fresh.process(&mut b, 64).unwrap();
    assert_ne!(a.samples(), b.samples());
    let mut settled = ParametricEq::new(format, false, true).unwrap();
    settled.set_gain(-6.0).unwrap();
    let mut consumed = block(format);
    settled.process(&mut consumed, 64).unwrap();
    settled.reset().unwrap();
    let mut c = block(format);
    settled.process(&mut c, 64).unwrap();
    assert_eq!(a.samples(), c.samples());
}
