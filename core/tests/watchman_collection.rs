use cumulus_core::{Error, Result, protocol::api::ApiFuture, watchman::*};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Default)]
struct Source {
    epoch: AtomicU64,
    calls: Mutex<Vec<String>>,
    fail_device: Mutex<Option<String>>,
    fail_environment_at: AtomicU64,
    env_count: AtomicU64,
    invalid_title: AtomicBool,
    clock_moved_back: AtomicBool,
    change_epoch: AtomicBool,
    hold: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
fn value(key: &str) -> Value {
    let f = descriptors()
        .unwrap()
        .into_iter()
        .find(|f| f.key == key)
        .unwrap();
    match f.kind {
        FieldKind::String => json!("fixture"),
        FieldKind::Hex => json!("0123456789abcdef0123456789abcdef"),
        FieldKind::Number => json!(7),
        FieldKind::Boolean => json!(false),
        FieldKind::Array => json!([1, 2, 3, 4]),
    }
}
impl Source {
    fn calls(&self, prefix: &str) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter_map(|c| c.strip_prefix(prefix).map(str::to_owned))
            .collect()
    }
}
impl MeasurementSource for Source {
    fn epoch(&self) -> Result<u64> {
        Ok(self.epoch.load(Ordering::SeqCst))
    }
    fn initialize<'a>(&'a self, build: SdkBuild, _: &'a Config) -> ApiFuture<'a, ()> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push(format!("init:{}", build.version()));
            Ok(())
        })
    }
    fn device<'a>(&'a self, key: &'a str, _: bool) -> ApiFuture<'a, MeasurementReading> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(format!("d:{key}"));
            if self.change_epoch.swap(false, Ordering::SeqCst) {
                self.epoch.fetch_add(1, Ordering::SeqCst);
            }
            if self.hold.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            Ok(
                if self.fail_device.lock().unwrap().as_deref() == Some(key) {
                    MeasurementReading::Failed
                } else {
                    MeasurementReading::Value(value(key))
                },
            )
        })
    }
    fn environment<'a>(
        &'a self,
        key: &'a str,
        timing: [i64; 6],
        elapsed: u64,
    ) -> ApiFuture<'a, EnvironmentReading> {
        Box::pin(async move {
            self.calls.lock().unwrap().push(format!("e:{key}"));
            let count = self.env_count.fetch_add(1, Ordering::SeqCst) + 1;
            let reading = if self.fail_environment_at.load(Ordering::SeqCst) == count {
                MeasurementReading::Failed
            } else {
                let v = match key {
                    "lb" => json!(&timing[1..]),
                    "ub" => json!(timing[0]),
                    "cc" => json!(elapsed),
                    "Wb" if self.invalid_title.load(Ordering::SeqCst) => {
                        let f = descriptors()?.into_iter().find(|f| f.key == key).unwrap();
                        let FieldLimit::Scalar(n) = f.limit else {
                            panic!()
                        };
                        json!("x".repeat(n - 1) + "🎵")
                    }
                    _ => value(key),
                };
                MeasurementReading::Value(v)
            };
            Ok(EnvironmentReading {
                reading,
                automation_ms: (key == "gb").then_some(
                    if self.clock_moved_back.load(Ordering::SeqCst) {
                        -13
                    } else {
                        13
                    },
                ),
            })
        })
    }
    fn drain_events<'a>(
        &'a self,
        _: &'a Config,
        reference: u64,
    ) -> ApiFuture<'a, Vec<Observation>> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push(format!("drain:{reference}"));
            Ok(vec![])
        })
    }
}
fn config(build: SdkBuild, options: Value) -> Config {
    Config::from_conf_for(
        build,
        "fixture",
        "",
        &encode_text(&options.to_string()).unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn official_device_selection_failure_and_lifetime_cache_match_both_builds() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/watchman-collection-official.json")).unwrap();
    for fixture in fixtures {
        let build = SdkBuild::parse(fixture["build"].as_str().unwrap()).unwrap();
        for case in fixture["device"].as_array().unwrap() {
            let source = Arc::new(Source::default());
            *source.fail_device.lock().unwrap() = case["failKey"].as_str().map(str::to_owned);
            let collector = NativeCollection::new(source.clone());
            let options = config(build, case["raw"].clone());
            let output = collector
                .collect(CollectionMode::Device, &options, 0)
                .await
                .unwrap();
            let actual: Vec<_> = output
                .device
                .iter()
                .map(|v| json!({"key":v.key,"failed":v.failed}))
                .collect();
            let mut actual = actual;
            actual.sort_by_key(|v| v["key"].as_str().unwrap().to_owned());
            let mut expected = case["fields"].as_array().unwrap().clone();
            expected.sort_by_key(|v| v["key"].as_str().unwrap().to_owned());
            assert_eq!(actual, expected);
            assert_eq!(json!(source.calls("d:")), case["order"]);
            let later = config(build, json!({"ejcd":true,"ews":true}));
            let cached = collector
                .collect(CollectionMode::Device, &later, 0)
                .await
                .unwrap();
            assert_eq!(cached.device.len(), output.device.len());
            assert_eq!(source.calls("d:").len(), output.device.len());
            source.epoch.fetch_add(1, Ordering::SeqCst);
            let reopened = collector
                .collect(CollectionMode::Device, &later, 0)
                .await
                .unwrap();
            assert_eq!(reopened.device.len(), 38);
            assert_eq!(source.calls("init:").len(), 2);
        }
    }
}

#[tokio::test]
async fn official_environment_expands_persistently_and_stops_on_measurement_or_encoding_failure() {
    let fixture: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/watchman-collection-official.json")).unwrap();
    for build_fixture in fixture {
        let build = SdkBuild::parse(build_fixture["build"].as_str().unwrap()).unwrap();
        let expected = &build_fixture["environment"][0];
        let source = Arc::new(Source::default());
        let collector = NativeCollection::new(source.clone());
        let cfg = config(build, json!({}));
        for (mode, name) in [
            (CollectionMode::Device, "firstDevice"),
            (CollectionMode::Behavior, "behavior"),
            (CollectionMode::Device, "laterDevice"),
        ] {
            let out = collector.collect_at(mode, &cfg, 198, 777).await.unwrap();
            let mut keys: Vec<_> = out.environment.iter().map(|f| f.key.clone()).collect();
            keys.sort();
            let mut expected: Vec<_> = expected[name]["fields"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect();
            expected.sort();
            assert_eq!(keys, expected);
        }
        assert_eq!(source.calls("drain:"), vec!["777"]);
        let failing = Arc::new(Source::default());
        failing.fail_environment_at.store(2, Ordering::SeqCst);
        let out = NativeCollection::new(failing.clone())
            .collect(CollectionMode::Device, &cfg, 0)
            .await
            .unwrap();
        assert_eq!(out.environment.len(), 1);
        assert_eq!(failing.calls("e:").len(), 2);
        let invalid = Arc::new(Source::default());
        invalid.invalid_title.store(true, Ordering::SeqCst);
        let out = NativeCollection::new(invalid.clone())
            .collect(CollectionMode::Behavior, &cfg, 0)
            .await
            .unwrap();
        let calls = invalid.calls("e:");
        assert_eq!(calls.last().unwrap(), "Wb");
        assert_eq!(out.environment.len() + 1, calls.len());
        assert!(out.environment.iter().all(|v| !v.failed));
    }
}

#[tokio::test]
async fn stale_page_and_cancelled_partial_collection_cannot_publish_device_cache() {
    let cfg = config(SdkBuild::B602a5ad7, json!({}));
    let source = Arc::new(Source::default());
    source.change_epoch.store(true, Ordering::SeqCst);
    let collector = Arc::new(NativeCollection::new(source.clone()));
    assert!(matches!(
        collector.collect(CollectionMode::Device, &cfg, 0).await,
        Err(Error::StaleResponse)
    ));
    let output = collector
        .collect(CollectionMode::Device, &cfg, 0)
        .await
        .unwrap();
    assert_eq!(output.device.len(), 35);
    assert_eq!(source.calls("d:").len(), 36);
    source.epoch.fetch_add(1, Ordering::SeqCst);
    source.hold.store(true, Ordering::SeqCst);
    let task = tokio::spawn({
        let collector = collector.clone();
        let cfg = cfg.clone();
        async move { collector.collect(CollectionMode::Device, &cfg, 0).await }
    });
    source.entered.notified().await;
    task.abort();
    assert!(matches!(task.await,Err(error) if error.is_cancelled()));
    let output = collector
        .collect(CollectionMode::Device, &cfg, 0)
        .await
        .unwrap();
    assert_eq!(output.device.len(), 35);
    assert_eq!(source.calls("d:").len(), 72);
}

#[tokio::test]
async fn behavior_observes_native_timings_and_drains_before_environment() {
    let source = Arc::new(Source::default());
    let collector = NativeCollection::new(source.clone());
    let cfg = config(
        SdkBuild::B602a5ad7,
        json!({"ejcd":"true","ews":1,"Lc":false}),
    );
    let first = collector
        .collect(CollectionMode::Device, &cfg, 0)
        .await
        .unwrap();
    assert_eq!(first.device.len(), 35);
    assert!(first.device.iter().any(|f| f.key == "Qa"));
    collector.record_timing(TimingSlot::Packet, 111).unwrap();
    collector
        .record_timing(TimingSlot::Configuration, 222)
        .unwrap();
    source.calls.lock().unwrap().clear();
    let output = collector
        .collect_at(CollectionMode::Behavior, &cfg, 345, 1234)
        .await
        .unwrap();
    assert_eq!(source.calls.lock().unwrap().first().unwrap(), "drain:1234");
    let timing = &output
        .environment
        .iter()
        .find(|f| f.key == "lb")
        .unwrap()
        .value;
    assert_eq!(timing[0], 111);
    assert_eq!(timing[1], 222);
    assert_eq!(
        output
            .environment
            .iter()
            .find(|f| f.key == "cc")
            .unwrap()
            .value,
        345
    );
    assert!(output.device.is_empty());
    source.clock_moved_back.store(true, Ordering::SeqCst);
    collector
        .collect(CollectionMode::Behavior, &cfg, 0)
        .await
        .unwrap();
    let after = collector
        .collect(CollectionMode::Behavior, &cfg, 0)
        .await
        .unwrap();
    assert_eq!(
        after
            .environment
            .iter()
            .find(|v| v.key == "lb")
            .unwrap()
            .value[4],
        -13
    );
}
