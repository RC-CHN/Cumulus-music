//! M147's recoverable web presentation state, separate from live TTL grants.
use super::{MusicClient, ScopeContext, store};
use crate::{
    Error, Result,
    account::SessionToken,
    playback::controller::SongSpeedObservation,
    protocol::{
        api::Api,
        http::MusicHttpClient,
        pc_experiments::{PcEffectMinibarEntry, PcExperimentGroups},
    },
};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::watch;

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Flags {
    pub(super) search_v3: bool,
    pub(super) search_footer: bool,
    pub(super) search_expanded: bool,
    pub(super) search_dense: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) song_speed: Option<bool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) effect_catalogue_new: bool,
    // Older presentation caches have no independent minibar decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) effect_minibar_entry: Option<PcEffectMinibarEntry>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) podcast_purchases_compact: bool,
    // A previous profile cache is not evidence for this newly bound action.
    #[serde(skip)]
    pub(super) podcast_resume: bool,
}

impl Flags {
    pub(super) fn project(groups: &PcExperimentGroups) -> Self {
        let minibar_entry = groups.effect_minibar_entry();
        Self {
            search_v3: groups.group("PH-PC-searchHot") == Some("t1"),
            search_footer: groups.group("PH-PC-search-result") == Some("t1"),
            search_expanded: groups.group("PH-PC-searchMore2") == Some("t1"),
            search_dense: groups.group("PH-PC-searchPageEfficiency") == Some("t1"),
            song_speed: song_speed_cohort(groups),
            effect_catalogue_new: groups.effect_catalogue_new_assigned(),
            effect_minibar_entry: (minibar_entry != PcEffectMinibarEntry::Legacy)
                .then_some(minibar_entry),
            podcast_purchases_compact: groups.group("PH-PC-listPage-showMore") == Some("t1"),
            podcast_resume: groups.group(crate::podcast::resume::EXPERIMENT) == Some("t1"),
        }
    }
}

fn song_speed_cohort(groups: &PcExperimentGroups) -> Option<bool> {
    let speed = groups
        .group("PH-PC-playSpeed")
        .filter(|group| !group.is_empty());
    let minibar = groups
        .group("PH-PC-minibarNew")
        .filter(|group| !group.is_empty());
    let legacy = groups
        .group("PH-PC-minibarNewLike")
        .filter(|group| !group.is_empty());
    match (speed, minibar, legacy) {
        (Some("t1"), Some("t1"), _) | (Some("t1"), Some("c"), Some("t1")) => Some(true),
        (Some(group), _, _) if group != "t1" => Some(false),
        (_, Some("c"), Some(group)) if group != "t1" => Some(false),
        (_, Some(group), _) if group != "t1" && group != "c" => Some(false),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PcPresentationView {
    pub epoch: String,
    pub revision: String,
    pub search_footer: bool,
    pub search_expanded: bool,
    pub search_dense: bool,
    pub guest_search_ready: bool,
    pub effect_catalogue_revision: String,
    pub effect_catalogue_new: bool,
    pub effect_minibar_entry: PcEffectMinibarEntry,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub podcast_purchases_compact: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub podcast_resume: bool,
}

impl PcPresentationView {
    fn from_flags(
        token: &SessionToken,
        revision: String,
        flags: Flags,
        guest_search_ready: bool,
    ) -> Self {
        Self {
            epoch: token.generation.to_string(),
            effect_catalogue_revision: revision.clone(),
            revision,
            search_footer: flags.search_footer,
            search_expanded: flags.search_expanded,
            search_dense: flags.search_dense,
            guest_search_ready,
            effect_catalogue_new: flags.effect_catalogue_new,
            effect_minibar_entry: flags.effect_minibar_entry.unwrap_or(
                if flags.effect_catalogue_new {
                    // An old cached modern catalogue implies the modern minibar
                    // but does not establish a yxrk cohort.
                    PcEffectMinibarEntry::Hidden
                } else {
                    PcEffectMinibarEntry::Legacy
                },
            ),
            podcast_purchases_compact: token.uid.is_some() && flags.podcast_purchases_compact,
            podcast_resume: token.uid.is_some() && flags.podcast_resume,
        }
    }
}

/// Internal endpoint choice is not a renderer-controlled experiment input.
#[derive(Clone)]
pub(in crate::client) struct PcPresentationSnapshot {
    pub(in crate::client) view: PcPresentationView,
    pub(in crate::client) search_v3: bool,
}

struct Current {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: Option<String>,
    owner: String,
    flags: Flags,
    view: PcPresentationView,
}

impl Current {
    fn same_scope(&self, token: &SessionToken, api: &Arc<dyn Api>, owner: &str) -> bool {
        self.token == *token
            && self.api.upgrade().is_some_and(|old| Arc::ptr_eq(&old, api))
            && self.owner == owner
    }

    fn matches(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        credentials: &Option<String>,
        owner: &str,
    ) -> bool {
        self.same_scope(token, api, owner) && self.credentials == *credentials
    }
    fn snapshot(&self) -> PcPresentationSnapshot {
        PcPresentationSnapshot {
            view: self.view.clone(),
            search_v3: self.flags.search_v3,
        }
    }
}

pub(super) struct PresentationHost {
    state: Mutex<Option<Current>>,
    changes: watch::Sender<PcPresentationView>,
    pub(super) song_speed: watch::Sender<SongSpeedObservation>,
    pub(super) running: Arc<AtomicBool>,
    pub(super) cache_write_failed: AtomicBool,
    revision: AtomicU64,
}

impl Default for PresentationHost {
    fn default() -> Self {
        let view = PcPresentationView {
            epoch: "0".into(),
            revision: "0".into(),
            search_footer: false,
            search_expanded: false,
            search_dense: false,
            guest_search_ready: false,
            effect_catalogue_revision: "0".into(),
            effect_catalogue_new: false,
            effect_minibar_entry: PcEffectMinibarEntry::Legacy,
            podcast_purchases_compact: false,
            podcast_resume: false,
        };
        Self {
            state: Mutex::new(None),
            changes: watch::channel(view).0,
            song_speed: watch::channel(SongSpeedObservation::default()).0,
            running: Arc::new(AtomicBool::new(false)),
            cache_write_failed: AtomicBool::new(false),
            revision: AtomicU64::new(0),
        }
    }
}

impl PresentationHost {
    fn publish(
        &self,
        token: &SessionToken,
        view: PcPresentationView,
        song_speed: Option<bool>,
        scope_changed: bool,
    ) {
        // Anonymous presentation can choose a catalog layout, but it does not
        // grant the signed-in song speed control to the player.
        let song_speed = token.uid.as_ref().and(song_speed);
        self.song_speed.send_if_modified(|previous| {
            if !scope_changed
                && previous.epoch == token.generation
                && previous.enabled == song_speed
            {
                return false;
            }
            *previous = SongSpeedObservation {
                epoch: token.generation,
                revision: view.revision.parse().expect("native decimal revision"),
                enabled: song_speed,
            };
            true
        });
        self.changes.send_replace(view);
    }

    fn next_revision(&self) -> Result<String> {
        self.revision
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map(|previous| (previous + 1).to_string())
            .map_err(|_| Error::State("PC presentation revision exhausted"))
    }
}

impl MusicClient {
    pub(in crate::client) fn effect_catalogue_presentation(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<(String, bool)> {
        let view = self.pc_presentation_context().snapshot(token, api)?.view;
        Ok((view.effect_catalogue_revision, view.effect_catalogue_new))
    }

    /// Pure native bootstrap/current snapshot. No request, timer or player lock.
    pub fn pc_presentation(&self) -> Result<PcPresentationView> {
        let token = self.session.token()?;
        let api = self.session.native_api(&token)?;
        self.pc_presentation_context()
            .snapshot(&token, &api)
            .map(|snapshot| snapshot.view)
    }

    pub fn subscribe_pc_presentation(&self) -> Result<watch::Receiver<PcPresentationView>> {
        self.pc_presentation()?;
        Ok(self.pc_experiments.presentation.changes.subscribe())
    }

    pub(in crate::client) fn pc_presentation_snapshot(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<super::PcPresentationSnapshot> {
        if api.pc_presentation_credential_revision()?.is_none() {
            return Err(Error::State(
                "PC presentation snapshot has no native transport",
            ));
        }
        self.pc_presentation_context().snapshot(token, api)
    }

    pub(in crate::client) fn pc_presentation_is_current(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        expected_revision: &str,
    ) -> Result<bool> {
        let snapshot = self.pc_presentation_snapshot(token, api)?;
        Ok(snapshot.view.revision == expected_revision)
    }
}

impl ScopeContext {
    pub(super) fn snapshot(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<PcPresentationSnapshot> {
        self.with_current(token, api, None, |_, current| Ok(current.snapshot()))
    }

    fn with_current<T>(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        expected_revision: Option<&str>,
        operation: impl FnOnce(&PresentationHost, &mut Current) -> Result<T>,
    ) -> Result<T> {
        self.session
            .with_pc_presentation_identity(token, api, |uid, no_uid, guest_ready| {
                let host = &self.host.presentation;
                let mut state = host.state.lock().map_err(|_| Error::LockPoisoned)?;
                // Sample inside the publication scope, so an older thread cannot
                // rebind the watch after another thread installed the latest scope.
                let credentials = match expected_revision {
                    Some(expected) => {
                        let value = api.pc_presentation_credential_revision()?;
                        if value.as_deref() != Some(expected) {
                            return Err(Error::StaleResponse);
                        }
                        value
                    }
                    None => api.pc_presentation_credential_revision().ok().flatten(),
                };
                let domain = if no_uid {
                    match api.pc_anonymous_state() {
                        Ok(crate::protocol::pc_experiments::PcAnonymousState::Registered {
                            uid,
                            revision,
                        }) => {
                            if credentials.as_deref() != Some(revision.as_str()) {
                                return Err(Error::StaleResponse);
                            }
                            Some(format!("anonymous:{uid}"))
                        }
                        _ => None,
                    }
                } else {
                    uid.map(str::to_owned)
                };
                let owner = domain.clone().unwrap_or_else(|| "local-guest".into());
                let guest_search_ready = guest_ready
                    && (api.as_ref() as &dyn std::any::Any)
                        .downcast_ref::<MusicHttpClient>()
                        .and_then(|transport| transport.guest_search_authority().ok().flatten())
                        .is_some_and(|authority| {
                            credentials.as_deref() == Some(&authority.revision)
                        });
                if expected_revision.is_some() && domain.is_none() {
                    return Err(Error::StaleResponse);
                }
                if state
                    .as_ref()
                    .is_none_or(|value| !value.matches(token, api, &credentials, &owner))
                {
                    let credential_changed = state.as_ref().is_some_and(|current| {
                        current.same_scope(token, api, &owner) && current.credentials != credentials
                    });
                    let flags = if credential_changed {
                        // Bootstrap is for a newly observed owner/context. Once this live
                        // context has actually rotated credentials, reloading its old
                        // display cache would resurrect a rejected previous-revision t1.
                        // Only a new successful read may repopulate this context.
                        Flags::default()
                    } else if domain.is_some() && credentials.is_some() {
                        let loaded = store::load(&self.store, &owner);
                        match loaded {
                            Ok(flags) => flags,
                            Err(_) => {
                                host.cache_write_failed.store(true, Ordering::Release);
                                Flags::default()
                            }
                        }
                    } else {
                        // A blank MUSIC_U is not itself a successful anonymous receipt.
                        Flags::default()
                    };
                    let view = PcPresentationView::from_flags(
                        token,
                        host.next_revision()?,
                        flags,
                        guest_search_ready,
                    );
                    *state = Some(Current {
                        token: token.clone(),
                        api: Arc::downgrade(api),
                        credentials,
                        owner,
                        flags,
                        view: view.clone(),
                    });
                    host.publish(token, view, flags.song_speed, true);
                } else if let Some(current) = state.as_mut()
                    && current.view.guest_search_ready != guest_search_ready
                {
                    current.view.revision = host.next_revision()?;
                    current.view.guest_search_ready = guest_search_ready;
                    // Readiness-only revisions do not invalidate a player observation.
                    host.changes.send_replace(current.view.clone());
                }
                operation(host, state.as_mut().expect("presentation initialized"))
            })
    }

    /// Only a successful live signed read reaches this hook. Persistence is a
    /// display bootstrap and is never copied back into `Assigned`/fresh grants.
    pub(super) fn commit_presentation(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
        revision: &str,
        groups: &PcExperimentGroups,
    ) -> Result<()> {
        self.session.require_api(token, api)?;
        if api.pc_presentation_credential_revision()?.as_deref() != Some(revision) {
            return Err(Error::StaleResponse);
        }
        let flags = Flags::project(groups);
        self.with_current(token, api, Some(revision), |host, current| {
            let persisted = store::save(&self.store, &current.owner, flags);
            if api.pc_presentation_credential_revision()?.as_deref() != Some(revision) {
                return Err(Error::StaleResponse);
            }
            host.cache_write_failed
                .store(persisted.is_err(), Ordering::Release);
            if current.flags != flags {
                let old_effect_revision = (current.flags.effect_catalogue_new
                    == flags.effect_catalogue_new)
                    .then(|| current.view.effect_catalogue_revision.clone());
                current.flags = flags;
                current.view = PcPresentationView::from_flags(
                    token,
                    host.next_revision()?,
                    flags,
                    current.view.guest_search_ready,
                );
                if let Some(revision) = old_effect_revision {
                    current.view.effect_catalogue_revision = revision;
                }
                host.publish(token, current.view.clone(), flags.song_speed, false);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::{AccountSession, profiles::ProfileRegistry},
        client::pc_experiments::OrdinarySkipAssignment,
        protocol::{api::ApiFuture, http::ApiOrigin},
    };
    use serde_json::{Value, json};
    use std::sync::atomic::AtomicUsize;

    struct NativeRead {
        calls: AtomicUsize,
        accepted: bool,
    }

    impl Api for NativeRead {
        fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async { panic!("presentation bootstrap must not fall back to ordinary API") })
        }

        fn verified_pc_experiment_revision(&self) -> Result<Option<String>> {
            Ok(Some("synthetic-current-revision".into()))
        }

        fn verified_pc_experiment_groups<'a>(
            &'a self,
            revision: &'a str,
        ) -> ApiFuture<'a, PcExperimentGroups> {
            Box::pin(async move {
                assert_eq!(revision, "synthetic-current-revision");
                self.calls.fetch_add(1, Ordering::Relaxed);
                if !self.accepted {
                    return Err(Error::service("synthetic", json!(403), Value::Null));
                }
                PcExperimentGroups::refreshed(&json!({"data":[
                    {"expName":"PH-PC-search-result","expGroupName":"t1"},
                    {"expName":"PH-PC-skipSongs2","expGroupName":"t1"}
                ]}))
            })
        }
    }

    #[tokio::test]
    async fn only_a_live_response_persists_and_bootstrap_never_grants_fresh_permission() {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let store = registry.store();
        let first = Arc::new(NativeRead {
            calls: AtomicUsize::new(0),
            accepted: true,
        });
        let session = Arc::new(AccountSession::new(first.clone(), Some(store.clone())));
        session
            .adopt(
                first.clone(),
                json!({"userId":71}),
                &session.token().unwrap(),
            )
            .unwrap();
        let client = MusicClient::with_session(session.clone(), registry);
        let mut changes = client.subscribe_pc_presentation().unwrap();
        assert!(!changes.borrow_and_update().search_footer);
        let before = store.change_stamp().unwrap();
        let live = client.refresh_pc_presentation().await.unwrap();
        assert!(live.search_footer);
        assert_eq!(*changes.borrow_and_update(), live);
        let saved = store.change_stamp().unwrap();
        assert_ne!(saved, before);
        assert_eq!(
            client
                .official_pc_ordinary_skip_assignment(&live.epoch)
                .await
                .unwrap(),
            OrdinarySkipAssignment::Enabled
        );
        assert_eq!(first.calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            store.change_stamp().unwrap(),
            saved,
            "a native fresh cache hit must not write display state again"
        );
        assert!(!changes.has_changed().unwrap());

        let second = Arc::new(NativeRead {
            calls: AtomicUsize::new(0),
            accepted: false,
        });
        session
            .adopt(
                second.clone(),
                json!({"userId":72}),
                &session.token().unwrap(),
            )
            .unwrap();
        let other = client.pc_presentation().unwrap();
        assert!(!other.search_footer);
        assert_ne!(other.epoch, live.epoch);
        session
            .adopt(
                second.clone(),
                json!({"userId":71}),
                &session.token().unwrap(),
            )
            .unwrap();
        let restored = client.pc_presentation().unwrap();
        assert!(restored.search_footer);
        assert_ne!(restored.epoch, live.epoch);
        assert_eq!(second.calls.load(Ordering::Relaxed), 0);
        assert!(
            client
                .official_pc_ordinary_skip_assignment(&restored.epoch)
                .await
                .unwrap_err()
                .is_service_code(403)
        );
        assert_eq!(client.pc_presentation().unwrap(), restored);
        assert_eq!(second.calls.load(Ordering::Relaxed), 1);
    }

    struct GuestSample {
        new_revision: AtomicBool,
        rotate_during_owner_read: AtomicBool,
    }

    impl GuestSample {
        fn revision(&self) -> String {
            if self.new_revision.load(Ordering::Relaxed) {
                "r2"
            } else {
                "r1"
            }
            .into()
        }
    }

    impl Api for GuestSample {
        fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
            Box::pin(async { panic!("an anonymous display snapshot is strictly local") })
        }

        fn pc_presentation_credential_revision(&self) -> Result<Option<String>> {
            Ok(Some(self.revision()))
        }

        fn pc_anonymous_state(&self) -> Result<crate::protocol::pc_experiments::PcAnonymousState> {
            if self.rotate_during_owner_read.swap(false, Ordering::Relaxed) {
                self.new_revision.store(true, Ordering::Relaxed);
            }
            Ok(
                crate::protocol::pc_experiments::PcAnonymousState::Registered {
                    uid: "9001".into(),
                    revision: self.revision(),
                },
            )
        }
    }

    #[test]
    fn song_speed_requires_the_official_signed_minibar_and_speed_groups() {
        use serde_json::json;
        let lanes = [
            "PH-PC-playSpeed",
            "PH-PC-minibarNew",
            "PH-PC-minibarNewLike",
        ];
        for (reply, expected) in [
            ([None, None, None], None),
            ([Some("t1"), Some("t1"), None], Some(true)),
            ([Some("t1"), Some("c"), Some("t1")], Some(true)),
            ([Some("c"), Some("t1"), None], Some(false)),
            ([Some("t1"), Some("c"), Some("c")], Some(false)),
            ([Some("t1"), Some("c"), None], None),
            ([None, Some("t1"), Some("t1")], None),
            ([Some("t1"), Some("future"), Some("t1")], Some(false)),
        ] {
            let rows: Vec<_> = lanes
                .iter()
                .zip(reply)
                .filter_map(|(name, group)| Some(json!({"expName":name,"expGroupName":group?})))
                .collect();
            let groups = PcExperimentGroups::refreshed(&json!({"data":rows})).unwrap();
            assert_eq!(Flags::project(&groups).song_speed, expected, "{reply:?}");
        }
    }

    #[test]
    fn anonymous_speed_assignment_cannot_enable_or_leak_from_a_signed_player_scope() {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let saved = Flags {
            search_footer: true,
            song_speed: Some(true),
            ..Flags::default()
        };
        store::save(&registry.store(), "anonymous:9001", saved).unwrap();
        store::save(&registry.store(), "71", saved).unwrap();
        let api: Arc<dyn Api> = Arc::new(GuestSample {
            new_revision: AtomicBool::new(false),
            rotate_during_owner_read: AtomicBool::new(false),
        });
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .clear_current(&session.token().unwrap(), &api, || Ok(()))
            .unwrap();
        let client = MusicClient::with_session(session.clone(), registry);
        let mut speed = client.pc_experiments.song_speed();

        let guest = client.pc_presentation().unwrap();
        assert!(guest.search_footer);
        assert_eq!(speed.borrow_and_update().enabled, None);
        let assigned = PcExperimentGroups::refreshed(&json!({"data":[
            {"expName":"PH-PC-playSpeed","expGroupName":"t1"},
            {"expName":"PH-PC-minibarNew","expGroupName":"t1"}
        ]}))
        .unwrap();
        client
            .pc_presentation_context()
            .commit_presentation(&session.token().unwrap(), &api, "r1", &assigned)
            .unwrap();
        assert!(!client.pc_presentation().unwrap().search_footer);
        assert!(!speed.has_changed().unwrap());

        session
            .adopt(api.clone(), json!({"userId":71}), &session.token().unwrap())
            .unwrap();
        let signed = client.pc_presentation().unwrap();
        assert_ne!(signed.epoch, guest.epoch);
        let observed = *speed.borrow_and_update();
        assert_eq!(observed.enabled, Some(true));
        assert_eq!(observed.epoch.to_string(), signed.epoch);

        session
            .clear_current(&session.token().unwrap(), &api, || Ok(()))
            .unwrap();
        let signed_out = client.pc_presentation().unwrap();
        let observed = *speed.borrow_and_update();
        assert_ne!(signed_out.epoch, signed.epoch);
        assert_eq!(observed.enabled, None);
        assert_eq!(observed.epoch.to_string(), signed_out.epoch);
    }

    #[test]
    fn mixed_anonymous_revision_sample_cannot_detour_through_local_guest_and_restore_old_flags() {
        let root = tempfile::tempdir().unwrap();
        let registry = ProfileRegistry::open(root.path(), ApiOrigin::Pc).unwrap();
        let saved = Flags {
            search_footer: true,
            ..Flags::default()
        };
        store::save(&registry.store(), "anonymous:9001", saved).unwrap();
        let fixture = Arc::new(GuestSample {
            new_revision: AtomicBool::new(false),
            rotate_during_owner_read: AtomicBool::new(false),
        });
        let api: Arc<dyn Api> = fixture.clone();
        let session = Arc::new(AccountSession::new(api.clone(), Some(registry.store())));
        session
            .clear_current(&session.token().unwrap(), &api, || Ok(()))
            .unwrap();
        let client = MusicClient::with_session(session.clone(), registry);
        let mut changes = client.subscribe_pc_presentation().unwrap();
        assert!(changes.borrow_and_update().search_footer);
        let epoch = session.token().unwrap().generation.to_string();

        fixture
            .rotate_during_owner_read
            .store(true, Ordering::Relaxed);
        assert!(matches!(
            client.pc_presentation(),
            Err(Error::StaleResponse)
        ));
        assert!(!changes.has_changed().unwrap());
        let current = client.pc_presentation().unwrap();
        assert_eq!(current.epoch, epoch);
        assert!(!current.search_footer);
        assert_eq!(*changes.borrow_and_update(), current);
        assert_eq!(session.token().unwrap().uid, None);
    }
}
