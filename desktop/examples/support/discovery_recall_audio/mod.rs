//! Existing sealed WAV/libmpv helper; this wrapper only supplies its fixture
//! dependencies so the discovery example can reuse its exact source check.
#[allow(dead_code)]
#[path = "../catalog_fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "../relay_fixture.rs"]
mod relay;
#[allow(dead_code)]
#[path = "../settings_fixture.rs"]
mod settings_fixture;

pub(super) use relay::{NativeAudio, wave};
