//! Real WebView adapter for the independent native collection coordinator.
pub(crate) mod broker;
use super::{EventHost, compatibility::context_script, measurement::*, start_events, stop_events};
pub use broker::{MeasurementBridge, MeasurementFrame, Ticket};
use cumulus_core::{
    Error, Result, identity::WindowsBrowserIdentity, protocol::api::ApiFuture, watchman::*,
};
use serde_json::{Value, json};
use std::{sync::Mutex, time::Duration};
use tauri::Manager;

struct Active {
    epoch: u64,
    build: SdkBuild,
    events: u64,
}
/// Native-owned, one collection instance per main window. No credential, token
/// or network authority is passed to this browser adapter.
pub struct BrowserMeasurements<R: tauri::Runtime> {
    window: tauri::WebviewWindow<R>,
    bridge: MeasurementBridge,
    identity: Option<WindowsBrowserIdentity>,
    active: Mutex<Option<Active>>,
    owner: u64,
}
impl<R: tauri::Runtime> BrowserMeasurements<R> {
    pub fn new(
        window: tauri::WebviewWindow<R>,
        identity: Option<WindowsBrowserIdentity>,
    ) -> Result<Self> {
        if window.label() != "main" {
            return Err(Error::InvalidInput(
                "measurement source requires main window",
            ));
        }
        let bridge = window.state::<MeasurementBridge>().inner().clone();
        let owner = bridge.claim()?;
        Ok(Self {
            window,
            bridge,
            identity,
            active: Mutex::new(None),
            owner,
        })
    }
    async fn request(&self, expression: String) -> Result<Value> {
        let (ticket, reply) = self.bridge.begin()?;
        let _pending = broker::Pending {
            bridge: self.bridge.clone(),
            ticket,
        };
        let script = format!(
            r#"(async()=>{{let value;try{{value=await ({expression});
          value=JSON.parse(JSON.stringify(value,(_,v)=>typeof v==='number'&&!Number.isFinite(v)?0:v));
          const valid=v=>{{if(typeof v==='string'){{for(let i=0;i<v.length;i++){{const c=v.charCodeAt(i);if(c>=0xd800&&c<=0xdbff){{const d=v.charCodeAt(++i);if(!(d>=0xdc00&&d<=0xdfff))return false;}}else if(c>=0xdc00&&c<=0xdfff)return false;}}}}else if(v&&typeof v==='object')return Object.values(v).every(valid);return true;}};
          if(!valid(value))value={{value:null,failed:true}};
        }}catch(_){{value={{hostFailure:true}};}}
        await window.__TAURI_INTERNALS__.invoke('watchman_measurement',{{frame:{{epoch:{},request:{},value}}}});}})();"#,
            ticket.epoch, ticket.request
        );
        self.window
            .eval(script)
            .map_err(|_| Error::State("browser measurement dispatch failed"))?;
        let value = tokio::time::timeout(Duration::from_secs(5), reply)
            .await
            .map_err(|_| Error::State("browser measurement timed out"))?
            .map_err(|_| Error::Cancelled)??;
        if value["hostFailure"] == true {
            return Err(Error::State("browser measurement execution failed"));
        }
        Ok(value)
    }
    fn active(&self) -> Result<(u64, SdkBuild, u64)> {
        let active = self.active.lock().map_err(|_| Error::LockPoisoned)?;
        let active = active
            .as_ref()
            .ok_or(Error::State("browser collector is not initialized"))?;
        if self.bridge.epoch()? != active.epoch {
            return Err(Error::StaleResponse);
        }
        Ok((active.epoch, active.build, active.events))
    }
}
fn reading(value: Value) -> Result<MeasurementReading> {
    if value.get("kind").is_some() {
        return Ok(MeasurementReading::Fingerprint(serde_json::from_value(
            value,
        )?));
    }
    if value["failed"]
        .as_bool()
        .ok_or(Error::Response("missing browser measurement outcome"))?
    {
        Ok(MeasurementReading::Failed)
    } else {
        Ok(MeasurementReading::Value(
            value.get("value").cloned().unwrap_or(Value::Null),
        ))
    }
}
impl<R: tauri::Runtime> MeasurementSource for BrowserMeasurements<R> {
    fn epoch(&self) -> Result<u64> {
        self.bridge.epoch()
    }
    fn initialize<'a>(&'a self, build: SdkBuild, config: &'a Config) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            let previous = self.active.lock().map_err(|_| Error::LockPoisoned)?.take();
            if previous.is_some() {
                stop_events(&self.window)?;
            }
            let epoch = self.epoch()?;
            let context = self
                .identity
                .as_ref()
                .map(context_script)
                .transpose()?
                .unwrap_or("undefined".into());
            let fingerprints = ["Jb", "Qa", "dc", "jb"]
                .into_iter()
                .map(|key| {
                    Ok(format!(
                        "{}:({})",
                        serde_json::to_string(key)?,
                        fingerprint_script(build, key)?
                    ))
                })
                .collect::<Result<Vec<_>>>()?
                .join(",");
            let script = format!(
                r#"(()=>{{const context={context};const basic=({});const environment=({})(context);
          const fingerprints={{{fingerprints}}};let cors=false;try{{cors='withCredentials' in new XMLHttpRequest();}}catch(_){{}}
          window.__cumulusWatchmanMeasurement={{build:{},owner:{},context,basic,environment,fingerprints}};
          return {{cors}};}})()"#,
                device_script(build),
                environment_factory(build),
                json!(build.version()),
                self.owner
            );
            let initialized = self.request(script).await?;
            if self.epoch()? != epoch {
                return Err(Error::StaleResponse);
            }
            let cors = initialized["cors"]
                .as_bool()
                .ok_or(Error::Response("missing browser CORS measurement"))?;
            let events = start_events(&self.window, config, cors)?;
            // Retain ownership before the acknowledgement await. Cancellation
            // must still let Drop or the next initialize detach this observer.
            *self.active.lock().map_err(|_| Error::LockPoisoned)? = Some(Active {
                epoch,
                build,
                events,
            });
            let ready = self
                .request(format!(
                    "window.__cumulusWatchmanEvents?.generation==={events}"
                ))
                .await;
            if !matches!(ready, Ok(Value::Bool(true))) {
                stop_events(&self.window)?;
                return Err(Error::State("measurement event observer not ready"));
            }
            if self.epoch()? != epoch {
                return Err(Error::StaleResponse);
            }
            *self.active.lock().map_err(|_| Error::LockPoisoned)? = Some(Active {
                epoch,
                build,
                events,
            });
            Ok(())
        })
    }
    fn device<'a>(&'a self, key: &'a str, deferred: bool) -> ApiFuture<'a, MeasurementReading> {
        Box::pin(async move {
            let (_, build, _) = self.active()?;
            let code = match key {
                "Jb" | "Qa" | "dc" | "jb" => {
                    fingerprint_script(build, key)?;
                    format!("state.fingerprints[{}](state.context)", json!(key))
                }
                "ib" => "state.environment.fontCount()".into(),
                "La" => "state.environment.audio()".into(),
                _ => format!(
                    "state.basic(state.context,{}).find(row=>row.key==={})",
                    json!(key),
                    json!(key)
                ),
            };
            let value=self.request(format!("(async()=>{{{}const state=window.__cumulusWatchmanMeasurement;if(state?.build!=={})throw Error('stale build');return await ({code});}})()",if deferred {"await new Promise(resolve=>setTimeout(resolve,0));"}else{""},json!(build.version()))).await?;
            reading(value)
        })
    }
    fn environment<'a>(
        &'a self,
        key: &'a str,
        timing: [i64; 6],
        elapsed: u64,
    ) -> ApiFuture<'a, EnvironmentReading> {
        Box::pin(async move {
            self.active()?;
            let value = self
                .request(format!(
                    "window.__cumulusWatchmanMeasurement.environment.environment({},{},{elapsed})",
                    json!(key),
                    json!(timing)
                ))
                .await?;
            let automation_ms = if key == "gb" {
                value["timing"][5].as_i64()
            } else {
                None
            };
            Ok(EnvironmentReading {
                reading: reading(value)?,
                automation_ms,
            })
        })
    }
    fn drain_events<'a>(
        &'a self,
        _: &'a Config,
        reference: u64,
    ) -> ApiFuture<'a, Vec<Observation>> {
        Box::pin(async move {
            let (epoch, _, events) = self.active()?;
            let flushed=self.request(format!("(async()=>{{const observer=window.__cumulusWatchmanEvents;if(observer?.generation!=={events})throw Error('stale events');await observer.flushed();return true;}})()")).await?;
            if flushed != true || self.epoch()? != epoch {
                return Err(Error::StaleResponse);
            }
            self.window.state::<EventHost>().drain(events, reference)
        })
    }
}

impl<R: tauri::Runtime> Drop for BrowserMeasurements<R> {
    fn drop(&mut self) {
        if let Ok(active) = self.active.get_mut()
            && let Some(active) = active.take()
        {
            if let Some(host) = self.window.try_state::<EventHost>() {
                let _ = host.stop();
            }
            let _=self.window.eval(format!("if(window.__cumulusWatchmanEvents?.generation==={})window.__cumulusWatchmanEvents.stop();if(window.__cumulusWatchmanMeasurement?.owner==={})delete window.__cumulusWatchmanMeasurement;",active.events,self.owner));
        }
        self.bridge.release(self.owner);
    }
}

/// A plugin hook cannot be displaced by an example/application page callback.
pub fn lifecycle<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("cumulus-watchman-lifecycle")
        .on_page_load(|webview, payload| {
            if webview.label() != "main" {
                return;
            }
            if let Some(bridge) = webview.try_state::<MeasurementBridge>() {
                let _ = match payload.event() {
                    tauri::webview::PageLoadEvent::Started => bridge.page_started(),
                    tauri::webview::PageLoadEvent::Finished => bridge.page_finished(),
                };
            }
            if payload.event() == tauri::webview::PageLoadEvent::Started
                && let Some(host) = webview.try_state::<EventHost>()
            {
                let _ = host.stop();
            }
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                super::host::page_ready(webview);
            }
        })
        .build()
}
