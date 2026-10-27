//! Explicit live verification: add only resources absent from the baseline,
//! undo those additions, and compare complete IDs/settings. Never auto-run.
use cumulus_core::{
    Error, Result,
    account::{
        AccountSession,
        credentials::Credentials,
        resources::{AccountResources, MutationPhase, ResourceKind},
    },
    catalog::{CatalogApi, SearchOptions},
    protocol::{
        api::identifier,
        http::{ApiOrigin, ClientProfile, MusicHttpClient},
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{collections::HashSet, path::PathBuf, sync::Arc};

fn ids(kind: ResourceKind, snapshot: &Value) -> Result<Vec<String>> {
    snapshot
        .as_array()
        .ok_or(Error::Response("missing baseline resource array"))?
        .iter()
        .map(|row| {
            identifier(if kind == ResourceKind::Liked {
                row
            } else {
                &row["id"]
            })
        })
        .collect()
}
async fn set(
    resources: &AccountResources,
    kind: ResourceKind,
    key: &str,
    value: Value,
) -> Result<()> {
    let record = if kind == ResourceKind::Settings {
        resources.set_setting(key, value).await?
    } else {
        resources
            .set_membership(
                kind,
                &json!(key),
                value
                    .as_bool()
                    .ok_or(Error::InvalidInput("membership must be boolean"))?,
            )
            .await?
    };
    if !matches!(
        record.phase,
        MutationPhase::Confirmed | MutationPhase::Unchanged
    ) {
        return Err(Error::MutationUncertain);
    }
    Ok(())
}
async fn reconcile(resources: &AccountResources, kind: ResourceKind, key: &str) -> Result<()> {
    for attempt in 0..3 {
        match resources.reconcile(kind, key).await {
            Ok(record)
                if matches!(
                    record.phase,
                    MutationPhase::Confirmed | MutationPhase::Unchanged
                ) =>
            {
                return Ok(());
            }
            Ok(_) => return Err(Error::MutationUncertain),
            Err(Error::MutationUncertain) if attempt < 2 => {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await
            }
            Err(error) => return Err(error),
        }
    }
    Err(Error::MutationUncertain)
}

async fn run() -> Result<Value> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(Error::InvalidInput(
            "usage: resource_roundtrip <private-profile-directory> <pc|interface>",
        ));
    }
    let origin = match args[1].as_str() {
        "pc" => ApiOrigin::Pc,
        "interface" => ApiOrigin::Interface,
        _ => return Err(Error::InvalidInput("unsupported origin")),
    };
    let directory = PathBuf::from(&args[0]);
    if !directory.join("core.sqlite3").is_file() {
        return Err(Error::InvalidInput(
            "explicit existing private profile required",
        ));
    }
    let store = Arc::new(StateStore::open(directory)?);
    let credentials = Arc::new(Credentials::open(store.clone(), origin)?);
    let client = Arc::new(MusicHttpClient::new(credentials, ClientProfile::default())?);
    let session = Arc::new(AccountSession::new(client, Some(store.clone())));
    session.refresh().await?;
    let resources = AccountResources::new(session.clone())?;
    let kinds = [
        ResourceKind::Liked,
        ResourceKind::Albums,
        ResourceKind::Artists,
        ResourceKind::Settings,
    ];
    let mut baseline = json!({});
    for kind in kinds {
        baseline[kind.as_str()] = resources.refresh(kind).await?;
    }
    let (_, songs) = session
        .run(|api, _| async move {
            CatalogApi::new(api)
                .search(
                    "音乐",
                    SearchOptions {
                        limit: 30,
                        ..Default::default()
                    },
                )
                .await
        })
        .await?;
    let songs = songs["result"]["songs"]
        .as_array()
        .ok_or(Error::Response("search did not return songs"))?;
    let mut choices = vec![];
    for kind in &kinds[..3] {
        let before: HashSet<_> = ids(*kind, &baseline[kind.as_str()])?.into_iter().collect();
        let candidates: Vec<_> = songs
            .iter()
            .flat_map(|song| match kind {
                ResourceKind::Liked => vec![song["id"].clone()],
                ResourceKind::Albums => vec![song["al"]["id"].clone()],
                ResourceKind::Artists => song["ar"]
                    .as_array()
                    .map(|rows| rows.iter().map(|r| r["id"].clone()).collect())
                    .unwrap_or_default(),
                ResourceKind::Settings | ResourceKind::Mvs => vec![],
            })
            .filter_map(|id| identifier(&id).ok())
            .collect();
        let key = candidates
            .into_iter()
            .find(|id| !before.contains(id))
            .ok_or(Error::Response(
                "no isolated candidate absent from baseline",
            ))?;
        choices.push((*kind, key, json!(true), json!(false)));
    }
    let original = baseline["settings"]["allowLikedNotify"]
        .as_bool()
        .ok_or(Error::Response("notification baseline is not boolean"))?;
    choices.push((
        ResourceKind::Settings,
        "allowLikedNotify".into(),
        json!(!original),
        json!(original),
    ));
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let token = session.token()?;
    let mut private = json!({"baseline":baseline,"choices":choices.iter().map(|(kind,key,desired,restore)|json!({"kind":kind,"key":key,"desired":desired,"restore":restore})).collect::<Vec<_>>(),"complete":false,"report":null});
    // Persist recovery inputs before touching a remote resource. These include
    // account data and stay in the existing private SQLite profile.
    session.save_result(&token, "audit-resource-roundtrip", &run_id, &private)?;
    let mut report = json!({"schema":1,"implementation":"rust-core","origin":origin,"checks":{}});
    for (kind, key, desired, restore) in choices {
        let mut errors = vec![];
        let applied = match set(&resources, kind, &key, desired).await {
            Ok(()) => true,
            Err(error) => {
                errors.push(error.to_string());
                reconcile(&resources, kind, &key).await.is_ok()
            }
        };
        let undo_confirmed = if applied {
            match set(&resources, kind, &key, restore).await {
                Ok(()) => true,
                Err(error) => {
                    errors.push(error.to_string());
                    reconcile(&resources, kind, &key).await.is_ok()
                }
            }
        } else {
            false
        };
        let baseline_restored = match resources.refresh(kind).await {
            Ok(after) => {
                if kind == ResourceKind::Settings {
                    after == baseline[kind.as_str()]
                } else {
                    ids(kind, &after)? == ids(kind, &baseline[kind.as_str()])?
                }
            }
            Err(error) => {
                errors.push(error.to_string());
                false
            }
        };
        report["checks"][kind.as_str()] = json!({"applied_confirmed":applied,"undo_confirmed":undo_confirmed,"baseline_restored":baseline_restored,"errors":errors,"ok":applied&&undo_confirmed&&baseline_restored&&errors.is_empty()});
        private["report"] = report.clone();
        // Save cleanup responsibility to the original account even if an
        // external account switch invalidated the session during the audit.
        store.put(
            token
                .uid
                .as_deref()
                .ok_or(Error::State("missing audit account"))?,
            "audit-resource-roundtrip",
            &run_id,
            &private,
        )?;
        if !baseline_restored {
            break;
        }
    }
    let cleanup_complete = report["checks"]
        .as_object()
        .is_some_and(|checks| checks.values().all(|v| v["baseline_restored"] == true));
    report["cleanup_complete"] = json!(cleanup_complete);
    private["complete"] = json!(cleanup_complete);
    private["report"] = report.clone();
    store.put(
        token
            .uid
            .as_deref()
            .ok_or(Error::State("missing audit account"))?,
        "audit-resource-roundtrip",
        &run_id,
        &private,
    )?;
    Ok(report)
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(report) => {
            println!("{}", serde_json::to_string_pretty(&report).expect("report"));
            if !report["checks"]
                .as_object()
                .is_some_and(|checks| checks.len() == 4 && checks.values().all(|v| v["ok"] == true))
            {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
