#[path = "support/manual_updates.rs"]
mod support;
use cumulus_core::updates::{
    LatestReleaseReply, LatestReleaseSource, ReleaseFuture, UpdateCheck, UpdateFailure,
    check_latest,
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use support::{Fixture, release};

struct Json(Value);
impl LatestReleaseSource for Json {
    fn latest(&self) -> ReleaseFuture<'_> {
        Box::pin(async {
            Ok(LatestReleaseReply::Published(
                serde_json::to_vec(&self.0).unwrap(),
            ))
        })
    }
}

#[tokio::test]
async fn stable_versions_are_compared_numerically_and_only_newer_offers_have_a_page() {
    let answer = check_latest("0.9.99", &Json(release("0.10.0")))
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(answer.view).unwrap(),
        json!({
            "state":"available","currentVersion":"0.9.99","latestVersion":"0.10.0"
        })
    );
    assert_eq!(
        answer.release.unwrap().url(),
        "https://github.com/RC-CHN/Cumulus-music/releases/tag/v0.10.0"
    );
    for version in ["0.9.98", "0.9.99"] {
        let answer = check_latest("0.9.99", &Json(release(version)))
            .await
            .unwrap();
        assert!(matches!(answer.view, UpdateCheck::NoNewer { .. }));
        assert!(answer.release.is_none());
    }
}

#[tokio::test]
async fn nonstable_unpublished_wrong_repository_and_incomplete_assets_fail_closed() {
    for (field, value) in [
        ("draft", json!(true)),
        ("prerelease", json!(true)),
        ("published_at", Value::Null),
        ("published_at", json!("invalid")),
        ("tag_name", json!("v0.02.0")),
        ("tag_name", json!("v0.2.0-rc.1")),
        ("tag_name", json!("v0.2.0/evil")),
        (
            "html_url",
            json!("https://github.com/attacker/Cumulus-music/releases/tag/v0.2.0"),
        ),
        (
            "html_url",
            json!("https://github.com/RC-CHN/Cumulus-music/releases/tag/v0.2.0?redirect=evil"),
        ),
        (
            "html_url",
            json!("https://github.com/RC-CHN/Cumulus-music/releases/tag/v0.3.0"),
        ),
    ] {
        let mut row = release("0.2.0");
        row[field] = value;
        assert!(
            matches!(
                check_latest("0.1.0", &Json(row)).await,
                Err(UpdateFailure::InvalidMetadata)
            ),
            "{field}"
        );
    }
    for bad in [
        json!({"name":"SHA256SUMS","state":"uploaded","size":8,"digest":null}),
        json!({"name":"SHA256SUMS","state":"uploaded","size":0,"digest":format!("sha256:{}","a".repeat(64))}),
        json!({"name":"SHA256SUMS","state":"starter","size":9,"digest":format!("sha256:{}","a".repeat(64))}),
    ] {
        let mut row = release("0.2.0");
        row["assets"][6] = bad;
        assert!(matches!(
            check_latest("0.1.0", &Json(row)).await,
            Err(UpdateFailure::InvalidMetadata)
        ));
    }
    let mut duplicate = release("0.2.0");
    let checksum = duplicate["assets"][6].clone();
    duplicate["assets"].as_array_mut().unwrap().push(checksum);
    assert!(matches!(
        check_latest("0.1.0", &Json(duplicate)).await,
        Err(UpdateFailure::InvalidMetadata)
    ));
}

#[tokio::test]
async fn absence_rate_limit_and_network_failure_do_not_make_an_update_offer() {
    let fixture = Fixture {
        mode: Default::default(),
        reads: Default::default(),
        installed: "0.1.0",
    };
    fixture.mode.store(4, Ordering::SeqCst);
    let report = check_latest("0.1.0", &fixture).await.unwrap();
    assert!(matches!(
        report.view,
        UpdateCheck::NoPublishedRelease { .. }
    ));
    assert!(report.release.is_none());
    for (mode, error) in [
        (2, UpdateFailure::Network),
        (3, UpdateFailure::RateLimited),
        (5, UpdateFailure::InvalidMetadata),
    ] {
        fixture.mode.store(mode, Ordering::SeqCst);
        assert!(matches!(check_latest("0.1.0", &fixture).await, Err(actual) if actual == error));
    }
    assert_eq!(fixture.reads.load(Ordering::SeqCst), 4);
}
