//! Exercise the actual cold offline MusicClient host with native libmpv.
use super::{Api, FLAC_BLOCK, Fixture, RATE, replace, sdk_path};
use crate::{
    Error,
    account::SessionStatus,
    client::{ColdOfflinePlayerView, MusicClient},
    effects::material::material_limiter_plan,
    playback::engine::{
        MpvConfig, MpvEngine,
        limiter_segment::{
            native_decoder::{SEEK_FRAME, SOURCE_END, input_pair},
            probe_wav::seconds,
        },
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::Ordering,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn client(fixture: &Fixture, library: Option<PathBuf>) -> MusicClient {
    let client = MusicClient::with_session(fixture.cold.clone(), fixture.registry.clone())
        .with_audio_factory(|| {
            Ok(Box::new(MpvEngine::open(MpvConfig {
                library: std::env::var_os("CUMULUS_LIMITER_PROBE_MPV").map(PathBuf::from),
                null_output: true,
            })?))
        });
    if let Some(library) = library {
        let plan = material_limiter_plan(&json!({
            "aev2": {"limiter": {"on": true, "threshold": -12.0, "attack": 0.005}}
        }))
        .unwrap();
        client
            .with_native_cold_offline_limiter(library, plan)
            .unwrap()
    } else {
        client
    }
}

fn view(value: ColdOfflinePlayerView) -> Value {
    serde_json::to_value(value).unwrap()
}

async fn clock(client: &MusicClient, id: &str, after: f64) -> Value {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let value = view(client.cold_offline_status().await.unwrap());
            assert_eq!(value["playingId"], id);
            if value["position"].as_f64().is_some_and(|at| at >= after) {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cold production output did not reach the requested clock")
}

#[tokio::test]
#[ignore = "requires trusted native libFLAC 1.5.0 and libmpv with null audio"]
async fn limiter_cold_flac_production_controller_opt_in_and_stock_fallback() {
    let sdk = sdk_path();
    let api = Api::open(&sdk).unwrap();
    let fixture = Fixture::new().await;
    let store = fixture.registry.store();
    let mut membership = fixture.membership();
    membership["wall"] = json!(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    );
    store
        .put("1", "membership", "current", &membership)
        .unwrap();
    let queue = json!({"sentinel":"existing queue remains outside cold mode"});
    store.put("1", "playback", "queue", &queue).unwrap();
    let session = fixture.cold.token().unwrap();

    let make = |name: &str, rate, index, encrypted| {
        let file = fixture.dir.path().join(format!("{name}-source.flac"));
        api.encode_fixture_with_index(&file, rate, FLAC_BLOCK, SOURCE_END, index, |frame| {
            input_pair(frame).map(i32::from)
        })
        .unwrap();
        fixture.publish(name, &fs::read(file).unwrap(), encrypted)
    };
    make("indexed", RATE, Some(FLAC_BLOCK), false);
    make("wrong-rate", 48_000, None, false);
    make("unindexed-playing", RATE, None, false);
    let original = make("unindexed-ncm", RATE, None, true);
    fixture.publish(
        "stock-aac",
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tag_audio/seed.m4a"
        )),
        false,
    );

    for sdk_option in [None, Some(fixture.dir.path().join("missing-libFLAC.dll"))] {
        let ordinary = client(&fixture, sdk_option);
        ordinary.cold_offline_open().await.unwrap();
        let loaded = view(ordinary.cold_offline_play("indexed".into()).await.unwrap());
        assert_eq!(loaded["phase"], "playing");
        assert!(loaded.get("limiterActive").is_none());
        assert!(
            clock(&ordinary, "indexed", 0.025)
                .await
                .get("limiterActive")
                .is_none()
        );
        ordinary.cold_offline_close().await.unwrap();
    }

    let client = client(&fixture, Some(sdk));
    client.cold_offline_open().await.unwrap();
    for id in ["stock-aac", "wrong-rate"] {
        let loaded = view(client.cold_offline_play(id.into()).await.unwrap());
        assert_eq!(loaded["phase"], "playing");
        assert_eq!(loaded["playingId"], id);
        assert!(loaded.get("limiterActive").is_none());
        client.cold_offline_stop().await.unwrap();
    }
    let loaded = view(client.cold_offline_play("indexed".into()).await.unwrap());
    assert_eq!(loaded["limiterActive"], true);
    clock(&client, "indexed", 0.030).await;
    assert!(matches!(
        client.cold_offline_pause("other-item".into(), true).await,
        Err(Error::StaleResponse)
    ));
    let paused = view(
        client
            .cold_offline_pause("indexed".into(), true)
            .await
            .unwrap(),
    );
    assert_eq!(paused["phase"], "paused");
    let target = seconds(SEEK_FRAME);
    let seek = view(
        client
            .cold_offline_seek("indexed".into(), target)
            .await
            .unwrap(),
    );
    assert_eq!(seek["phase"], "paused");
    assert_eq!(seek["limiterActive"], true);
    let indexed = seek["position"].as_f64().unwrap();
    assert!((indexed - target).abs() < 0.010);
    assert!(matches!(
        client.cold_offline_seek("indexed".into(), 90.0).await,
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(
        view(client.cold_offline_status().await.unwrap())["limiterActive"],
        true
    );
    client
        .cold_offline_pause("indexed".into(), false)
        .await
        .unwrap();
    clock(&client, "indexed", target + 0.040).await;

    let loaded = view(
        client
            .cold_offline_play("unindexed-playing".into())
            .await
            .unwrap(),
    );
    assert_eq!(loaded["limiterActive"], true);
    clock(&client, "unindexed-playing", 0.030).await;
    let running = view(
        client
            .cold_offline_seek("unindexed-playing".into(), target)
            .await
            .unwrap(),
    );
    assert_eq!(running["phase"], "playing");
    assert_eq!(running["playingId"], "unindexed-playing");
    assert!(running.get("limiterActive").is_none());
    clock(&client, "unindexed-playing", target + 0.040).await;

    let loaded = view(
        client
            .cold_offline_play("unindexed-ncm".into())
            .await
            .unwrap(),
    );
    assert_eq!(loaded["limiterActive"], true);
    clock(&client, "unindexed-ncm", 0.030).await;
    client
        .cold_offline_pause("unindexed-ncm".into(), true)
        .await
        .unwrap();
    replace(&original);
    assert!(
        client
            .cold_offline_play("unindexed-ncm".into())
            .await
            .is_err()
    );
    let seek = view(
        client
            .cold_offline_seek("unindexed-ncm".into(), target)
            .await
            .unwrap(),
    );
    assert_eq!(seek["phase"], "paused");
    assert_eq!(seek["playingId"], "unindexed-ncm");
    assert!(seek.get("limiterActive").is_none());
    let stock = seek["position"].as_f64().unwrap();
    assert!((stock - target).abs() < 0.010);
    client
        .cold_offline_pause("unindexed-ncm".into(), false)
        .await
        .unwrap();
    clock(&client, "unindexed-ncm", target + 0.040).await;
    store.delete("1", "membership", "current").unwrap();
    let stopped = view(client.cold_offline_status().await.unwrap());
    assert_eq!(stopped["phase"], "idle");
    assert_eq!(stopped["stopReason"], "cached_rights_unavailable");
    assert_eq!(store.get("1", "playback", "queue").unwrap(), Some(queue));
    assert_eq!(fixture.cold.token().unwrap(), session);
    assert_eq!(
        fixture.cold.snapshot().unwrap().status,
        SessionStatus::Unknown
    );
    assert_eq!(fixture.service.0.load(Ordering::Acquire), 1);
    client.cold_offline_close().await.unwrap();
    eprintln!(
        "production cold controller: missing SDK, AAC and 48 kHz stayed stock; indexed limiter seek at {indexed:.6}s, replaced unindexed NCM same-reader stock fallback at {stock:.6}s; cached-rights stop without queue/account mutation"
    );
}
