//! Synthetic device catalog around real libmpv null decoding. UI policy proof
//! only; actual endpoint routing has a separate isolated Pulse integration test.
use cumulus_core::{Error, Result, playback::engine::*};
use std::sync::{Arc, Mutex};
pub struct Catalog(pub Mutex<Vec<OutputDevice>>);
impl Default for Catalog {
    fn default() -> Self {
        Self(Mutex::new(Self::devices()))
    }
}
impl Catalog {
    pub fn devices() -> Vec<OutputDevice> {
        vec![
            OutputDevice {
                id: "auto".into(),
                description: "系统默认".into(),
            },
            OutputDevice {
                id: "fixture/a".into(),
                description: "合成 USB 耳机 A · 很长的输出设备名称用于验证小窗口布局".into(),
            },
            OutputDevice {
                id: "fixture/b".into(),
                description: "合成显示器 B".into(),
            },
        ]
    }
}
pub struct Native {
    pub inner: super::relay_fixture::NativeAudio,
    pub catalog: Arc<Catalog>,
    pub configured: String,
}
impl AudioEngine for Native {
    fn output_devices(&mut self) -> Result<Option<OutputDevices>> {
        Ok(Some(OutputDevices {
            devices: self.catalog.0.lock().unwrap().clone(),
            configured: self.configured.clone(),
            active_driver: Some("null".into()),
        }))
    }
    fn set_output_device(&mut self, id: &str) -> Result<()> {
        if !self.catalog.0.lock().unwrap().iter().any(|d| d.id == id) {
            return Err(Error::State("synthetic output disappeared"));
        }
        self.configured = id.into();
        Ok(())
    }
    fn load(&mut self, r: LoadRequest, c: &Cancellation) -> Result<LoadedMedia> {
        self.inner.load(r, c)
    }
    fn poll(&mut self) -> Result<EngineSnapshot> {
        self.inner.poll()
    }
    fn pause(&mut self, v: bool) -> Result<()> {
        self.inner.pause(v)
    }
    fn seek(&mut self, v: f64) -> Result<()> {
        self.inner.seek(v)
    }
    fn volume(&mut self, v: f64) -> Result<()> {
        self.inner.volume(v)
    }
    fn speed(&mut self, v: f64) -> Result<()> {
        self.inner.speed(v)
    }
    fn stop(&mut self) -> Result<()> {
        self.inner.stop()
    }
}
