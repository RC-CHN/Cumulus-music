mod support;
use cumulus_core::{
    account::AccountSession,
    download::{
        completed::{CompletedDownloads, FileAction},
        offline::OfflineLibrary,
        service::{
            DownloadDestination, DownloadOptions, DownloadOrigin, DownloadRunOptions,
            DownloadService, DownloadStatus, OutputContainer,
        },
    },
    media::tags::{decode_comment, voice_plan},
    podcast::Episode,
    storage::StateStore,
};
use serde_json::{Value, json};
use std::sync::Arc;
use support::{Reply, ScriptApi};
const AUDIO: &[u8] = include_bytes!("fixtures/tag_audio/seed.mp3");
fn program() -> Value {
    json!({"id":81,"name":"Episode","mainSong":{"id":91,"duration":1000},"radio":{"id":71,"name":"Radio"},"dj":{"userId":7,"nickname":"Host"},"coverUrl":"https://p.test.music.126.net/cover"})
}
fn detail() -> Reply {
    json!({"code":200,"program":program()}).into()
}
fn permission(allowed: bool) -> Reply {
    json!({"code":200,"data":{"allowDownloadVoiceIds":if allowed{vec![81]}else{vec![]},"forbidDownloadVoiceIds":if allowed{vec![]}else{vec![81]},"downloadCheckResultList":[{"voiceId":81,"canDownload":allowed}]}}).into()
}
fn resource() -> Reply {
    use md5::{Digest, Md5};
    json!({"code":200,"data":{"id":91,"code":200,"url":"https://m.test.music.126.net/media?auth=fixture","md5":hex::encode(Md5::digest(AUDIO)),"size":AUDIO.len(),"type":"mp3","level":"standard","flag":0,"payed":0}}).into()
}
struct Fixture {
    root: tempfile::TempDir,
    api: Arc<ScriptApi>,
    session: Arc<AccountSession>,
    media: Arc<support::download::Backend>,
    service: DownloadService,
}
impl Fixture {
    fn new(replies: Vec<Reply>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let api = Arc::new(ScriptApi::new(replies));
        let session = Arc::new(AccountSession::new(
            api.clone(),
            Some(Arc::new(
                StateStore::open(root.path().join("state")).unwrap(),
            )),
        ));
        session
            .adopt(api.clone(), json!({"userId":1}), &session.token().unwrap())
            .unwrap();
        let media = Arc::new(support::download::Backend::new(AUDIO.into()));
        let service =
            DownloadService::with_backend(session.clone(), media.clone(), 1024 * 1024).unwrap();
        Self {
            root,
            api,
            session,
            media,
            service,
        }
    }
    fn destination(&self) -> DownloadDestination {
        DownloadDestination::Named {
            root: self.root.path().join("media"),
            path_type: 0,
            file_type: 0,
        }
    }
}
#[tokio::test]
async fn program_download_renews_rights_after_reopen_tags_correct_identity_and_manages_files() {
    let f = Fixture::new(vec![
        detail(),
        detail(),
        permission(true),
        resource(),
        detail(),
        permission(true),
        resource(),
    ]);
    let task = f
        .service
        .create_voice(&json!(81), f.destination(), DownloadOptions::default())
        .await
        .unwrap();
    assert_eq!(task.song_id, "91");
    assert_eq!(
        task.origin,
        DownloadOrigin::Voice {
            voice_id: "81".into()
        }
    );
    let paused = f
        .service
        .run(
            &task.id,
            DownloadRunOptions {
                byte_budget: Some(123),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused);
    assert_eq!(paused.bytes, 123);
    let reopened_session = Arc::new(AccountSession::new(
        f.api.clone(),
        Some(Arc::new(
            StateStore::open(f.root.path().join("state")).unwrap(),
        )),
    ));
    reopened_session
        .adopt(
            f.api.clone(),
            json!({"userId":1}),
            &reopened_session.token().unwrap(),
        )
        .unwrap();
    let reopened =
        DownloadService::with_backend(reopened_session.clone(), f.media.clone(), 1024 * 1024)
            .unwrap();
    let done = reopened
        .run(&task.id, DownloadRunOptions::default())
        .await
        .unwrap();
    assert_eq!(done.status, DownloadStatus::Complete);
    assert_eq!(done.authorization_count, 2);
    assert_eq!(f.media.audio.calls()[1].offset, 123);
    let path = done.target.as_ref().unwrap();
    assert!(path.ends_with("电台节目/Host - Episode.mp3"));
    use lofty::{
        file::TaggedFileExt,
        prelude::{Accessor, ItemKey},
    };
    let tagged = lofty::read_from_path(path).unwrap();
    let tags = tagged.primary_tag().unwrap();
    assert_eq!(tags.title().as_deref(), Some("Episode"));
    assert_eq!(tags.artist().as_deref(), Some("Host"));
    assert_eq!(tags.album().as_deref(), Some("Radio"));
    let comm = tags
        .get_strings(ItemKey::Comment)
        .find(|s| s.starts_with("163 key"))
        .unwrap();
    let decoded = decode_comment(comm).unwrap();
    assert!(decoded.starts_with("dj:"));
    let value: Value = serde_json::from_str(&decoded[3..]).unwrap();
    assert_eq!(value["programId"], "81");
    assert_eq!(value["radioId"], "71");
    assert_eq!(value["mainMusic"]["mainTrackId"], "91");
    let offline = OfflineLibrary::new(reopened_session.clone()).unwrap();
    let checked = offline
        .check(&reopened_session.token().unwrap(), path, true)
        .await
        .unwrap();
    assert!(checked.access.can_play && checked.integrity_verified);
    assert_eq!(checked.record.song_id, "91");
    let files = CompletedDownloads::new(reopened_session).unwrap();
    let op = files.rename(&task.id, "renamed.mp3").unwrap();
    files.resume(&op.id).unwrap();
    let target = f.root.path().join("moved.mp3");
    let op = files
        .start(&task.id, FileAction::Move, Some(target.clone()))
        .unwrap();
    files.resume(&op.id).unwrap();
    assert!(target.exists());
    let op = files.start(&task.id, FileAction::Delete, None).unwrap();
    files.resume(&op.id).unwrap();
    assert!(!target.exists());
    assert_eq!(
        f.api
            .calls()
            .iter()
            .filter(|c| c.path == "/api/dj/voices/download/check")
            .count(),
        2
    );
    assert!(f.api.calls().iter().all(|c|c.path!="/api/v3/song/detail"&&c.path!="/api/song/enhance/player/url/v1"));
}
#[tokio::test]
async fn revoked_program_download_does_not_continue_partial_media() {
    let f = Fixture::new(vec![
        detail(),
        detail(),
        permission(true),
        resource(),
        detail(),
        permission(false),
    ]);
    let task = f
        .service
        .create_voice(
            &json!(81),
            f.destination(),
            DownloadOptions {
                tags: false,
                cover: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    f.service
        .run(
            &task.id,
            DownloadRunOptions {
                byte_budget: Some(123),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        f.service
            .run(&task.id, DownloadRunOptions::default())
            .await
            .is_err()
    );
    assert_eq!(f.media.audio.calls().len(), 1);
    assert_eq!(
        f.service.get(&task.id).unwrap().status,
        DownloadStatus::Error
    );
    assert!(!f.service.get(&task.id).unwrap().target.unwrap().exists());
}
#[tokio::test]
async fn changed_main_song_or_permission_refusal_never_contacts_media_backend() {
    for changed in [false, true] {
        let mut new = program();
        new["mainSong"]["id"] = json!(92);
        let mut replies = vec![detail()];
        if changed {
            replies.push(json!({"code":200,"program":new}).into());
        } else {
            replies.extend([detail(), permission(false)]);
        }
        let f = Fixture::new(replies);
        let task = f
            .service
            .create_voice(&json!(81), f.destination(), DownloadOptions::default())
            .await
            .unwrap();
        assert!(
            f.service
                .run(&task.id, DownloadRunOptions::default())
                .await
                .is_err()
        );
        assert!(f.media.audio.calls().is_empty());
    }
}
#[tokio::test]
async fn unsupported_transcript_and_ncm_contracts_reject_before_creating_a_task() {
    let f = Fixture::new(vec![]);
    for options in [
        DownloadOptions {
            lyrics: true,
            ..Default::default()
        },
        DownloadOptions {
            container: OutputContainer::Ncm,
            ..Default::default()
        },
    ] {
        assert!(
            f.service
                .create_voice(&json!(81), f.destination(), options)
                .await
                .is_err()
        );
    }
    assert!(f.service.list().unwrap().is_empty());
    assert!(f.api.calls().is_empty());
    assert!(
        OfflineLibrary::new(f.session.clone())
            .unwrap()
            .records(&f.session.token().unwrap())
            .unwrap()
            .is_empty()
    );
}
#[test]
fn voice_tag_plan_is_dj_metadata_and_matches_all_supported_plain_audio_containers() {
    let item = Episode::from_official(&program()).unwrap();
    for ext in ["mp3", "flac", "m4a"] {
        let plan = voice_plan(&item.normalized(), &json!({"type":ext,"gain":1.25}), false).unwrap();
        assert_eq!(plan.tags.tit2, "Episode");
        assert_eq!(plan.tags.tpe1, "Host");
        assert_eq!(plan.tags.talb, "Radio");
        assert!(plan.comment_plain.starts_with("dj:"));
    }
}
