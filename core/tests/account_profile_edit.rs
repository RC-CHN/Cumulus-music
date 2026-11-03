mod support;

use cumulus_core::{
    Error,
    account::{
        AccountSession, SessionToken,
        profile::edit::{
            ProfileBirthday, ProfileEditPhase, ProfileEditor, ProfilePatch, ProfileRegion,
        },
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi, gate};

const UPDATE: &str = "/api/user/profile/update";
const DETAIL: &str = "/api/w/v1/user/detail/7";
const DUPLICATED: &str = "/api/nickname/duplicated";

struct World {
    root: tempfile::TempDir,
    api: Arc<ScriptApi>,
    session: Arc<AccountSession>,
    editor: Arc<ProfileEditor>,
    token: SessionToken,
}

impl World {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(StateStore::open(root.path()).unwrap());
        let api = Arc::new(ScriptApi::new(replies));
        let session = Arc::new(AccountSession::new(api.clone(), Some(store)));
        session
            .adopt(
                api.clone(),
                json!({
                    "userId":7,"nickname":"Baseline","accountOnly":"preserve-this",
                    "avatarUrl":"https://p5.music.126.net/current-avatar.jpg"
                }),
                &session.token().unwrap(),
            )
            .unwrap();
        let token = session.token().unwrap();
        let editor = Arc::new(ProfileEditor::new(session.clone()).unwrap());
        Self {
            root,
            api,
            session,
            editor,
            token,
        }
    }

    async fn baseline(&self) -> String {
        self.editor
            .refresh_for(&self.token)
            .await
            .unwrap()
            .snapshot
            .unwrap()
            .revision
    }

    fn writes(&self) -> usize {
        self.api
            .calls()
            .iter()
            .filter(|call| call.path == UPDATE)
            .count()
    }
}

fn detail(nickname: &str, signature: &str) -> Value {
    json!({"code":200,"level":8,"profile":{
        "userId":7,"nickname":nickname,"signature":signature,"gender":1,"province":110000,"city":110100,
        "avatarUrl":"https://p1.music.126.net/proof/avatar.jpg?private=test-secret","unknown":"ignored-secret"
    }})
}

fn patch_signature(signature: &str) -> ProfilePatch {
    ProfilePatch {
        signature: Some(signature.into()),
        ..Default::default()
    }
}

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[tokio::test]
async fn exact_changed_fields_and_verified_same_uid_metadata() {
    let before = detail("Baseline", "old");
    let mut fresh_before = before.clone();
    fresh_before["level"] = json!(9); // A non-editable change doesn't invalidate the draft.
    let mut after = detail("新昵称", "");
    after["profile"]["birthday"] = json!(951_753_600_000_i64);
    after["profile"]["province"] = json!(130000);
    after["profile"]["city"] = json!(130100);
    after["profile"]["gender"] = json!(2); // Not submitted; irrelevant to confirmation.
    let world = World::new([
        before.into(),
        fresh_before.into(),
        json!({"code":200,"duplicated":false}).into(),
        json!({"code":"200","profile":{"nickname":"ignore-ack-profile"}}).into(),
        after.into(),
    ]);
    let revision = world.baseline().await;
    let id = id();
    let patch = ProfilePatch {
        nickname: Some("  新昵称  ".into()),
        signature: Some(String::new()),
        gender: Some(1),
        birthday: Some(ProfileBirthday {
            year: 2000,
            month: 2,
            day: 29,
            utc_offset_minutes: 480,
        }),
        region: Some(ProfileRegion {
            province: 130000,
            city: 130100,
        }),
    };
    let op = world
        .editor
        .change_for(&world.token, &id, &revision, patch.clone())
        .await
        .unwrap();
    assert_eq!(op.phase, ProfileEditPhase::Confirmed);
    let calls = world.api.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [DETAIL, DETAIL, DUPLICATED, UPDATE, DETAIL]
    );
    assert_eq!(calls[2].payload, json!({"nickname":"新昵称"}));
    assert_eq!(
        calls[3].payload,
        json!({"nickname":"新昵称","signature":"","birthday":951_753_600_000_i64,"province":130000,"city":130100})
    );
    assert!(calls.iter().all(|call| !call.cache));
    assert_eq!(
        world
            .editor
            .change_for(&world.token, &id, &revision, patch)
            .await
            .unwrap(),
        op
    );
    assert_eq!(world.writes(), 1);
    let snapshot = world.session.snapshot().unwrap();
    assert_eq!(snapshot.token, world.token);
    let profile = snapshot.profile.unwrap();
    assert_eq!(profile["nickname"], "新昵称");
    assert_eq!(profile["accountOnly"], "preserve-this");
    assert_eq!(
        profile["avatarUrl"],
        "https://p5.music.126.net/current-avatar.jpg"
    );
    assert!(!serde_json::to_string(&op).unwrap().contains("secret"));
    let reopened = StateStore::open(world.root.path()).unwrap();
    assert_eq!(
        reopened.get("7", "account", "profile").unwrap(),
        Some(profile)
    );
}

#[tokio::test]
async fn stale_baseline_and_unchanged_request_do_not_dispatch_writes() {
    let world = World::new([
        detail("Baseline", "old").into(),
        detail("Baseline", "externally changed").into(),
        detail("Baseline", "externally changed").into(),
    ]);
    let stale = world.baseline().await;
    assert!(matches!(
        world
            .editor
            .change_for(&world.token, &id(), &stale, patch_signature("new"))
            .await,
        Err(Error::State("account profile changed"))
    ));
    let current = world
        .editor
        .page_for(&world.token)
        .unwrap()
        .snapshot
        .unwrap()
        .revision;
    let op = world
        .editor
        .change_for(
            &world.token,
            &id(),
            &current,
            patch_signature("externally changed"),
        )
        .await
        .unwrap();
    assert_eq!(op.phase, ProfileEditPhase::Unchanged);
    assert_eq!(world.writes(), 0);
}

#[tokio::test]
async fn partial_or_lost_write_requires_readonly_reconciliation_and_fresh_acceptance() {
    let before = detail("Baseline", "old");
    let partial = detail("Changed", "old");
    let changed_again = detail("Changed", "different again");
    let world = World::new([
        before.clone().into(),
        before.into(),
        json!({"code":200,"duplicated":false}).into(),
        Reply::Ready(Err(Error::Transport("lost mutation response".into()))),
        partial.into(),
        changed_again.clone().into(),
        changed_again.into(),
    ]);
    let revision = world.baseline().await;
    let id = id();
    let patch = ProfilePatch {
        nickname: Some("Changed".into()),
        signature: Some("desired".into()),
        ..Default::default()
    };
    let unknown = world
        .editor
        .change_for(&world.token, &id, &revision, patch.clone())
        .await
        .unwrap();
    assert_eq!(unknown.phase, ProfileEditPhase::Uncertain);
    assert!(unknown.observed.is_none());
    assert!(matches!(
        world
            .editor
            .change_for(
                &world.token,
                &uuid::Uuid::new_v4().to_string(),
                &revision,
                patch.clone()
            )
            .await,
        Err(Error::MutationUncertain)
    ));
    assert_eq!(
        world
            .editor
            .change_for(&world.token, &id, &revision, patch)
            .await
            .unwrap(),
        unknown
    );
    let checked = world.editor.reconcile_for(&world.token, &id).await.unwrap();
    assert_eq!(checked.phase, ProfileEditPhase::Uncertain);
    assert_eq!(
        world.session.snapshot().unwrap().profile.unwrap()["nickname"],
        "Baseline"
    );
    let observed = checked.observed.unwrap().revision;
    assert!(matches!(
        world
            .editor
            .accept_current_for(&world.token, &id, &observed)
            .await,
        Err(Error::State("account profile changed"))
    ));
    let current = world.editor.page_for(&world.token).unwrap().operations[0]
        .observed
        .as_ref()
        .unwrap()
        .revision
        .clone();
    let accepted = world
        .editor
        .accept_current_for(&world.token, &id, &current)
        .await
        .unwrap();
    assert_eq!(accepted.phase, ProfileEditPhase::AcceptedCurrentState);
    assert_eq!(
        world.session.snapshot().unwrap().profile.unwrap()["nickname"],
        "Changed"
    );
    assert_eq!(world.writes(), 1);
}

#[tokio::test]
async fn dropped_dispatched_intent_persists_and_is_never_replayed() {
    let (pending, release) = gate();
    let before = detail("Baseline", "old");
    let world = World::new([
        before.clone().into(),
        before.into(),
        pending,
        detail("Baseline", "new").into(),
    ]);
    let revision = world.baseline().await;
    let id = id();
    let task = {
        let (editor, token, id, revision) = (
            world.editor.clone(),
            world.token.clone(),
            id.clone(),
            revision.clone(),
        );
        tokio::spawn(async move {
            editor
                .change_for(&token, &id, &revision, patch_signature("new"))
                .await
        })
    };
    world.api.wait_for_calls(3).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(release.send(Ok(json!({"code":200}))).is_err());
    let editor = ProfileEditor::new(world.session.clone()).unwrap();
    let op = editor
        .change_for(&world.token, &id, &revision, patch_signature("new"))
        .await
        .unwrap();
    assert_eq!(op.phase, ProfileEditPhase::Pending);
    assert_eq!(
        editor.reconcile_for(&world.token, &id).await.unwrap().phase,
        ProfileEditPhase::Confirmed
    );
    assert_eq!(world.writes(), 1);
}

#[tokio::test]
async fn malformed_patches_and_prohibited_nickname_never_reach_update() {
    let before = detail("Baseline", "old");
    let world = World::new([
        before.clone().into(),
        before.into(),
        json!({"code":407}).into(),
    ]);
    let revision = world.baseline().await;
    let invalid = [
        ProfilePatch {
            nickname: Some("a".into()),
            ..Default::default()
        },
        ProfilePatch {
            birthday: Some(ProfileBirthday {
                year: 2001,
                month: 2,
                day: 29,
                utc_offset_minutes: 480,
            }),
            ..Default::default()
        },
        ProfilePatch {
            region: Some(ProfileRegion {
                province: 130000,
                city: 110100,
            }),
            ..Default::default()
        },
        ProfilePatch {
            gender: Some(0),
            ..Default::default()
        },
        patch_signature(&"😀".repeat(76)),
    ];
    for patch in invalid {
        assert!(matches!(
            world
                .editor
                .change_for(&world.token, &id(), &revision, patch)
                .await,
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(world.api.calls().len(), 1);
    let prohibited = ProfilePatch {
        nickname: Some("Forbidden".into()),
        ..Default::default()
    };
    assert!(matches!(
        world
            .editor
            .change_for(&world.token, &id(), &revision, prohibited)
            .await,
        Err(Error::InvalidInput("profile nickname is prohibited"))
    ));
    assert_eq!(world.writes(), 0);
    assert!(
        world
            .editor
            .page_for(&world.token)
            .unwrap()
            .operations
            .is_empty()
    );
}

#[tokio::test]
async fn preflight_transport_failure_is_known_unsent_without_journal_entry() {
    let before = detail("Baseline", "old");
    let world = World::new([
        before.clone().into(),
        Reply::Ready(Err(Error::Transport("detail unavailable".into()))),
        before.into(),
        Reply::Ready(Err(Error::Transport("duplicate check unavailable".into()))),
    ]);
    let revision = world.baseline().await;
    for patch in [
        patch_signature("new"),
        ProfilePatch {
            nickname: Some("Changed".into()),
            ..Default::default()
        },
    ] {
        assert!(matches!(
            world
                .editor
                .change_for(&world.token, &id(), &revision, patch)
                .await,
            Err(Error::RequestNotSent(_))
        ));
    }
    assert_eq!(world.writes(), 0);
    assert!(
        world
            .editor
            .page_for(&world.token)
            .unwrap()
            .operations
            .is_empty()
    );
}
