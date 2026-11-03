#[path = "support/nos.rs"]
mod nos_model;
mod support;

use cumulus_core::{
    Error,
    account::{AccountSession, SessionToken, profile::avatar::*},
    cloud::nos::{NosBackend, NosNode, NosRequest, NosRunOptions, NosStatus},
    protocol::api::ApiFuture,
    storage::StateStore,
};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use serde_json::{Value, json};
use std::{io::Cursor, sync::Arc};
use support::{Reply, ScriptApi, gate};

const DETAIL: &str = "/api/w/v1/user/detail/7";
const ALLOC: &str = "/api/nos/token/alloc";
const BIND: &str = "/api/user/avatar/upload/v1";
const BEFORE: &str = "https://p2.music.126.net/baseline.jpg";
const REQUESTED: &str = "https://p5.music.126.net/synthetic-avatar.jpg";

#[derive(Default)]
struct Backend(nos_model::Backend);
impl NosBackend for Backend {
    fn discover<'a>(&'a self, bucket: &'a str) -> ApiFuture<'a, NosNode> {
        Box::pin(async move {
            assert_eq!(bucket, "yyimgs");
            NosNode::parse("https://nosup-test.127.net")
        })
    }
    fn request(&self, request: NosRequest) -> ApiFuture<'_, Value> {
        self.0.request(request)
    }
}

struct World {
    root: tempfile::TempDir,
    session: Arc<AccountSession>,
    api: Arc<ScriptApi>,
    backend: Arc<Backend>,
    editor: Arc<AvatarEditor>,
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
                json!({"userId":7,"nickname":"Do not overwrite","avatarUrl":BEFORE}),
                &session.token().unwrap(),
            )
            .unwrap();
        let token = session.token().unwrap();
        let backend = Arc::new(Backend::default());
        let editor = Arc::new(
            AvatarEditor::with_backend(session.clone(), backend.clone(), 256 * 1024).unwrap(),
        );
        Self {
            root,
            session,
            api,
            backend,
            editor,
            token,
        }
    }

    async fn prepared(&self) -> String {
        let task = self
            .editor
            .select(&self.token, image(480, 360))
            .await
            .unwrap();
        assert_eq!(task.phase, AvatarPhase::Draft);
        let preview = self.editor.image(&self.token, &task.id).unwrap();
        assert_eq!((preview.width, preview.height), (480, 360));
        assert!(preview.preview.starts_with("data:image/jpeg;base64,"));
        let task = self
            .editor
            .prepare(
                &self.token,
                &task.id,
                AvatarCrop {
                    x: 80,
                    y: 20,
                    size: 330,
                },
            )
            .unwrap();
        assert_eq!(task.phase, AvatarPhase::Prepared);
        task.id
    }

    async fn uploaded(&self) -> String {
        let id = self.prepared().await;
        let view = self
            .editor
            .upload(&self.token, &id, NosRunOptions::default())
            .await
            .unwrap();
        assert_eq!(view.phase, AvatarPhase::Uploaded);
        assert_eq!(view.transfer, Some(NosStatus::Uploaded));
        id
    }

    fn count(&self, path: &str) -> usize {
        self.api
            .calls()
            .iter()
            .filter(|call| call.path == path)
            .count()
    }
}

fn image(width: u32, height: u32) -> Vec<u8> {
    let pixels = RgbImage::from_fn(width, height, |x, y| {
        Rgb([(x % 255) as u8, (y % 255) as u8, 70])
    });
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(pixels)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

fn detail(url: &str) -> Reply {
    json!({"code":200,"profile":{"userId":7,"nickname":"Stale detail nickname","avatarUrl":url}})
        .into()
}

fn allocation() -> Reply {
    json!({"code":200,"data":{"bucket":"yyimgs","objectKey":"synthetic-avatar.jpg",
         "token":"PRIVATE_SCOPED_TOKEN","docId":"9007199254740995"}})
    .into()
}

#[tokio::test]
async fn crop_nos_bind_and_exact_verified_readback_publish_only_avatar() {
    let canonical = "https://p6.music.126.net/server-rewritten.jpg";
    let world = World::new([
        detail(BEFORE),
        allocation(),
        detail(BEFORE),
        json!({"code":200,"url":format!("{canonical}?secret=hidden")}).into(),
        detail(canonical),
    ]);
    let id = world.uploaded().await;
    let calls = world.backend.0.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].complete && !calls[0].query);
    assert_eq!(calls[0].content_type, "image/jpeg");
    assert_eq!(calls[0].token, "PRIVATE_SCOPED_TOKEN");
    let jpeg = image::load_from_memory(&calls[0].body).unwrap();
    assert_eq!((jpeg.width(), jpeg.height()), (330, 330));
    let result = world.editor.bind(&world.token, &id).await.unwrap();
    assert_eq!(result.phase, AvatarPhase::Confirmed);
    assert!(!result.may_leave_object);
    let calls = world.api.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        [DETAIL, ALLOC, DETAIL, BIND, DETAIL]
    );
    assert_eq!(
        calls[1].payload,
        json!({"filename":"avatar.jpg","ext":"jpg","type":"image","bucket":"yyimgs"})
    );
    assert_eq!(
        calls[3].payload,
        json!({"imgid":"9007199254740995","url":REQUESTED})
    );
    let avatar = format!("{canonical}?param=320y320");
    let profile = world.session.snapshot().unwrap().profile.unwrap();
    assert_eq!(profile["avatarUrl"], avatar);
    assert_eq!(profile["nickname"], "Do not overwrite");
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("PRIVATE_SCOPED_TOKEN")
    );
    assert!(result.cleanup_pending);
    assert!(
        !world
            .editor
            .cleanup(&world.token, &id)
            .unwrap()
            .cleanup_pending
    );
    assert!(
        !world
            .root
            .path()
            .join(format!("account-avatar-{id}.jpg"))
            .exists()
    );
    assert_eq!(
        world.editor.bind(&world.token, &id).await.unwrap().phase,
        AvatarPhase::Confirmed
    );
    assert_eq!(world.count(BIND), 1);
}

#[tokio::test]
async fn lost_binding_does_not_replay_and_accept_current_is_read_only() {
    let unrelated = "https://p1.music.126.net/other-avatar.jpg";
    let other_again = "https://p1.music.126.net/another-avatar.jpg";
    let world = World::new([
        detail(BEFORE),
        allocation(),
        detail(BEFORE),
        Reply::Ready(Err(Error::Transport("lost binding".into()))),
        detail(unrelated),
        detail(other_again),
        detail(other_again),
    ]);
    let id = world.uploaded().await;
    let first = world.editor.bind(&world.token, &id).await.unwrap();
    assert_eq!(first.phase, AvatarPhase::Binding);
    assert!(first.observed_revision.is_none());
    let reopened =
        AvatarEditor::with_backend(world.session.clone(), world.backend.clone(), 256 * 1024)
            .unwrap();
    assert_eq!(
        reopened.bind(&world.token, &id).await.unwrap().phase,
        AvatarPhase::Binding
    );
    assert!(matches!(
        reopened.abandon(&world.token, &id),
        Err(Error::MutationUncertain)
    ));
    let checked = reopened.reconcile(&world.token, &id).await.unwrap();
    assert_eq!(checked.phase, AvatarPhase::Binding);
    assert_eq!(
        world.session.snapshot().unwrap().profile.unwrap()["avatarUrl"],
        BEFORE
    );
    let old_revision = checked.observed_revision.unwrap();
    assert!(matches!(
        reopened
            .accept_current(&world.token, &id, &old_revision)
            .await,
        Err(Error::State("account avatar changed"))
    ));
    let new_revision = reopened
        .get(&world.token, &id)
        .unwrap()
        .observed_revision
        .unwrap();
    let accepted = reopened
        .accept_current(&world.token, &id, &new_revision)
        .await
        .unwrap();
    assert_eq!(accepted.phase, AvatarPhase::AcceptedCurrentState);
    assert!(accepted.may_leave_object);
    assert_eq!(world.count(BIND), 1);
}

#[tokio::test]
async fn dropped_bind_future_persists_and_exact_target_recovers_without_ack() {
    let (pending, release) = gate();
    let world = World::new([
        detail(BEFORE),
        allocation(),
        detail(BEFORE),
        pending,
        detail(REQUESTED),
    ]);
    let id = world.uploaded().await;
    let work = {
        let (editor, token, id) = (world.editor.clone(), world.token.clone(), id.clone());
        tokio::spawn(async move { editor.bind(&token, &id).await })
    };
    world.api.wait_for_calls(4).await;
    work.abort();
    assert!(work.await.unwrap_err().is_cancelled());
    assert!(release.send(Ok(json!({"code":200}))).is_err());
    assert_eq!(
        world.editor.get(&world.token, &id).unwrap().phase,
        AvatarPhase::Binding
    );
    assert_eq!(
        world
            .editor
            .reconcile(&world.token, &id)
            .await
            .unwrap()
            .phase,
        AvatarPhase::Confirmed
    );
    assert_eq!(world.count(BIND), 1);
}

#[tokio::test]
async fn lost_allocation_is_not_reallocated_and_invalid_picker_preserves_draft() {
    let world = World::new([
        detail(BEFORE),
        Reply::Ready(Err(Error::Transport("lost allocation".into()))),
    ]);
    let id = world.prepared().await;
    assert!(
        world
            .editor
            .select(&world.token, image(319, 320))
            .await
            .is_err()
    );
    assert_eq!(
        world.editor.get(&world.token, &id).unwrap().phase,
        AvatarPhase::Prepared
    );
    for _ in 0..2 {
        let result = world
            .editor
            .upload(&world.token, &id, NosRunOptions::default())
            .await
            .unwrap();
        assert_eq!(result.phase, AvatarPhase::Allocating);
        assert!(result.may_leave_object);
    }
    assert_eq!(world.count(ALLOC), 1);
    assert!(world.backend.0.calls().is_empty());
    assert_eq!(world.count(BIND), 0);
}

#[tokio::test]
async fn decoded_avatar_rejections_settle_and_an_external_change_blocks_binding() {
    for (code, issue) in [
        (405, AvatarIssue::RateLimited),
        (533, AvatarIssue::PhoneRequired),
    ] {
        let world = World::new([
            detail(BEFORE),
            allocation(),
            detail(BEFORE),
            json!({"code":code}).into(),
        ]);
        let id = world.uploaded().await;
        let view = world.editor.bind(&world.token, &id).await.unwrap();
        assert_eq!(view.phase, AvatarPhase::Rejected);
        assert_eq!(view.error, Some(issue));
        assert_eq!(world.count(BIND), 1);
    }
    let world = World::new([
        detail(BEFORE),
        allocation(),
        detail("https://p4.music.126.net/other.jpg"),
    ]);
    let id = world.uploaded().await;
    let view = world.editor.bind(&world.token, &id).await.unwrap();
    assert_eq!(view.phase, AvatarPhase::Uploaded);
    assert_eq!(view.error, Some(AvatarIssue::Changed));
    assert_eq!(world.count(BIND), 0);
}

#[tokio::test]
async fn switching_account_during_bind_preserves_original_responsibility() {
    let (pending, release) = gate();
    let world = World::new([detail(BEFORE), allocation(), detail(BEFORE), pending]);
    let id = world.uploaded().await;
    let work = {
        let (editor, token, id) = (world.editor.clone(), world.token.clone(), id.clone());
        tokio::spawn(async move { editor.bind(&token, &id).await })
    };
    world.api.wait_for_calls(4).await;
    let replacement = Arc::new(ScriptApi::new([]));
    world
        .session
        .adopt(
            replacement.clone(),
            json!({"userId":8,"nickname":"Other account"}),
            &world.session.token().unwrap(),
        )
        .unwrap();
    release
        .send(Ok(json!({"code":200,"url":REQUESTED})))
        .unwrap();
    assert!(matches!(work.await.unwrap(), Err(Error::StaleResponse)));
    assert!(replacement.calls().is_empty());
    let snapshot = world.session.snapshot().unwrap();
    assert_eq!(snapshot.profile.unwrap()["nickname"], "Other account");
    let store = StateStore::open(world.root.path()).unwrap();
    let original = store.get("7", "account-avatar-v1", &id).unwrap().unwrap();
    assert_eq!(original["phase"], "binding");
    assert!(store.get("8", "account-avatar-v1", &id).unwrap().is_none());
}
