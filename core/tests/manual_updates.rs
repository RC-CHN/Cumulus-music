#[path = "support/manual_updates.rs"]
mod support;
use cumulus_core::updates::{
    LatestReleaseReply, LatestReleaseSource, PackageFailure, ReleaseFuture, UpdateCheck,
    UpdateFailure, check_latest,
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use support::{Fixture, release, synthetic_package};

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
async fn release_candidates_only_offer_newer_stable_versions() {
    let current = "0.2.0-rc.1";
    let previous = check_latest(current, &Json(release("0.1.0")))
        .await
        .unwrap();
    assert_eq!(
        previous.view,
        UpdateCheck::NoNewer {
            current_version: current.into(),
            latest_version: "0.1.0".into(),
        }
    );
    assert!(previous.release.is_none());
    for version in ["0.2.0", "0.3.0"] {
        let stable = check_latest(current, &Json(release(version)))
            .await
            .unwrap();
        assert_eq!(
            stable.view,
            UpdateCheck::Available {
                current_version: current.into(),
                latest_version: version.into(),
            }
        );
        assert!(stable.release.is_some());
    }
    for malformed in [
        "0.2.0-rc.0",
        "0.2.0-rc.01",
        "0.2.0-rc.+1",
        "0.2.0-rc.",
        "0.2.0-alpha.1",
    ] {
        assert!(matches!(
            check_latest(malformed, &Json(release("0.2.0"))).await,
            Err(UpdateFailure::InvalidMetadata)
        ));
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

#[tokio::test]
async fn package_verification_binds_bytes_to_the_checked_version_and_format() {
    let root = tempfile::tempdir().unwrap();
    let result = check_latest("0.2.0", &Json(release("0.2.0")))
        .await
        .unwrap();
    assert!(result.release.is_none());
    let packages = result.packages.unwrap();
    for (suffix, format) in [
        ("linux-x64.deb", "linux_deb"),
        ("linux-x64.tar.gz", "linux_portable"),
        ("windows-x64.zip", "windows_zip"),
        ("windows-x64-setup.exe", "windows_setup"),
    ] {
        let name = format!("Cumulus-0.2.0-{suffix}");
        let path = root.path().join(&name);
        let bytes = synthetic_package(&name);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(
            serde_json::to_value(packages.verify(&path).unwrap()).unwrap(),
            json!({"version":"0.2.0","filename":name,"format":format}),
        );
        let mut changed = bytes.clone();
        *changed.last_mut().unwrap() ^= 1;
        std::fs::write(&path, &changed).unwrap();
        assert!(matches!(
            packages.verify(&path),
            Err(PackageFailure::DigestMismatch)
        ));
        changed[0] ^= 1;
        std::fs::write(&path, &changed).unwrap();
        assert!(matches!(
            packages.verify(&path),
            Err(PackageFailure::WrongFormat)
        ));
        std::fs::write(&path, &changed[..changed.len() - 1]).unwrap();
        assert!(matches!(
            packages.verify(&path),
            Err(PackageFailure::WrongSize)
        ));
    }

    let renamed = root.path().join("Cumulus-0.1.0-linux-x64.deb");
    std::fs::write(&renamed, synthetic_package("Cumulus-0.2.0-linux-x64.deb")).unwrap();
    assert!(matches!(
        packages.verify(&renamed),
        Err(PackageFailure::WrongName)
    ));
    let directory = root.path().join("folder");
    std::fs::create_dir_all(directory.join("Cumulus-0.2.0-linux-x64.deb")).unwrap();
    assert!(matches!(
        packages.verify(&directory.join("Cumulus-0.2.0-linux-x64.deb")),
        Err(PackageFailure::InvalidFile)
    ));

    #[cfg(unix)]
    {
        let link = root.path().join("link");
        std::fs::create_dir(&link).unwrap();
        let path = link.join("Cumulus-0.2.0-linux-x64.deb");
        std::os::unix::fs::symlink(&renamed, &path).unwrap();
        assert!(matches!(
            packages.verify(&path),
            Err(PackageFailure::InvalidFile)
        ));
    }
}
