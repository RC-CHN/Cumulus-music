//! Native search experiment assignment and last successful tab order.
use crate::catalog::search::results::SearchCategory;
use crate::{
    Result,
    account::{AccountSession, SessionToken, login_policy::AB_PATH},
    client::MusicClient,
    protocol::api::Api,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const EXPERIMENTS: [&str; 3] = [
    "PH-PC-searchHot",
    "PH-PC-searchMore2",
    "PH-PC-searchPageEfficiency",
];
const NAMESPACE: &str = "search-experiment-v1";
const ASSIGNMENT_KEY: &str = "assignment-v2";
const REFRESH_SECONDS: u64 = 30 * 60;
const FOOTER_EXPERIMENT: &str = "PH-PC-search-result";

#[derive(Default)]
pub(crate) struct SearchHost {
    pub(crate) enabled: bool,
    pub(super) podcast_playbacks: super::podcast_playback::SearchPodcastPlaybacks,
    pub(super) voice_menu_pages: super::voice_menu::SearchVoiceMenuPages,
    pub(super) queue_drops: super::queue_drop::SearchDropPages,
    pub(super) audio_toplist: super::audio_toplist::AudioToplist,
    pub(super) privilege_pages: super::privilege::SearchPrivilegePages,
    pub(super) default_contexts: super::default_context::DefaultContexts,
    pub(super) guest_default_contexts: super::default_context::DefaultContexts,
    pub(super) preview_config: Option<Arc<crate::protocol::mconfig::PcConfig>>,
    pub(super) presentation: super::presentation::SearchPresentationPages,
    gate: tokio::sync::Mutex<()>,
    native: Mutex<Option<NativeAssignment>>,
}

#[derive(Clone)]
struct NativeAssignment {
    token: SessionToken,
    api: Weak<dyn Api>,
    credentials: String,
    fetched: tokio::time::Instant,
    flags: SearchFlags,
    footer: bool,
    optional_read_failed: bool,
}

impl NativeAssignment {
    fn matches(&self, token: &SessionToken, api: &Arc<dyn Api>, credentials: &str) -> bool {
        self.token == *token
            && self.api.upgrade().is_some_and(|old| Arc::ptr_eq(&old, api))
            && self.credentials == credentials
    }
}

pub(super) struct SearchAssignment {
    pub(super) flags: SearchFlags,
    // Never persisted or supplied through IPC. An ordinary Api response cannot
    // grant the independently signed TrackCard footer experiment.
    pub(super) footer_revision: Option<String>,
    pub(super) optional_read_failed: bool,
}

#[derive(Clone, Copy, Deserialize, Serialize, Default)]
#[serde(default)]
pub(super) struct SearchFlags {
    pub(super) v3: bool,
    pub(super) expanded: bool,
    pub(super) dense: bool,
}

#[derive(Deserialize, Serialize, Default)]
struct Assignment {
    attempted_at: u64,
    #[serde(flatten)]
    flags: SearchFlags,
}

fn previous_assignment(session: &AccountSession, token: &SessionToken) -> Result<Assignment> {
    let store = session.persistent_store()?;
    session.with_token(token, |owner| {
        let current = store
            .get(owner, NAMESPACE, ASSIGNMENT_KEY)?
            .and_then(|value| serde_json::from_value(value).ok());
        if let Some(current) = current {
            return Ok(current);
        }
        let previous = store.get(owner, NAMESPACE, "assignment")?;
        Ok(Assignment {
            flags: SearchFlags {
                v3: previous.as_ref().is_some_and(|value| value["v3"] == true),
                ..SearchFlags::default()
            },
            ..Assignment::default()
        })
    })
}

fn save_assignment(
    session: &AccountSession,
    token: &SessionToken,
    updated: &Assignment,
) -> Result<()> {
    let store = session.persistent_store()?;
    session.with_token(token, |owner| {
        store.put(
            owner,
            NAMESPACE,
            ASSIGNMENT_KEY,
            &serde_json::to_value(updated)?,
        )
    })
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

impl MusicClient {
    pub(super) async fn search_assignment_for(
        &self,
        token: &SessionToken,
        api: &Arc<dyn Api>,
    ) -> Result<SearchAssignment> {
        if !self.search.enabled {
            return Ok(SearchAssignment {
                flags: SearchFlags::default(),
                footer_revision: None,
                optional_read_failed: false,
            });
        }
        if api
            .verified_pc_experiment_revision()
            .ok()
            .flatten()
            .is_none()
        {
            return self
                .search
                .assignment_for(&self.session, token, api.as_ref())
                .await
                .map(|flags| SearchAssignment {
                    flags,
                    footer_revision: None,
                    optional_read_failed: false,
                });
        }

        let _gate = self.search.gate.lock().await;
        self.session.require_api(token, api)?;
        let old = previous_assignment(&self.session, token)?;
        if self.login_pending()?
            || self
                .session
                .with_verified_profile(token, api, |_, _| Ok(()))
                .is_err()
        {
            return Ok(SearchAssignment {
                flags: old.flags,
                footer_revision: None,
                optional_read_failed: true,
            });
        }
        let Some(credentials) = api.verified_pc_experiment_revision().ok().flatten() else {
            return Ok(SearchAssignment {
                flags: old.flags,
                footer_revision: None,
                optional_read_failed: true,
            });
        };
        let previous = self
            .search
            .native
            .lock()
            .map_err(|_| crate::Error::LockPoisoned)?
            .as_ref()
            .filter(|current| current.matches(token, api, &credentials))
            .cloned();
        if let Some(current) = &previous
            && current.optional_read_failed
            && current.fetched.elapsed() < Duration::from_secs(REFRESH_SECONDS)
            && self
                .pc_experiments
                .observed_groups(token, api, &credentials)?
                .is_none()
        {
            return Ok(SearchAssignment {
                flags: current.flags,
                footer_revision: current.footer.then_some(credentials),
                optional_read_failed: current.optional_read_failed,
            });
        }

        // M147:2890–2918 sends open web keys plus the native vector once.
        // The original three search flags and the footer share that signed
        // batch; never issue a second short query if this optional read fails.
        let refreshed = self.verified_pc_experiment_groups_for(token, api).await;
        self.session.require_api(token, api)?;
        if api
            .verified_pc_experiment_revision()
            .ok()
            .flatten()
            .as_deref()
            != Some(credentials.as_str())
        {
            return Ok(SearchAssignment {
                flags: old.flags,
                footer_revision: None,
                optional_read_failed: true,
            });
        }
        let (flags, footer, optional_read_failed) = match refreshed {
            Ok((revision, groups)) if revision == credentials => (
                SearchFlags {
                    v3: groups.group(EXPERIMENTS[0]) == Some("t1"),
                    expanded: groups.group(EXPERIMENTS[1]) == Some("t1"),
                    dense: groups.group(EXPERIMENTS[2]) == Some("t1"),
                },
                groups.group(FOOTER_EXPERIMENT) == Some("t1"),
                false,
            ),
            // M147 retains the latest global web assignment, not the last
            // value this search page happened to consume. Another native
            // consumer may have replaced t1 with c before that batch expired.
            // This presentation-only catch does not make that batch fresh.
            _ => {
                let (flags, footer) = self
                    .pc_experiments
                    .search_catch_groups(token, api, &credentials)?
                    .map(|groups| {
                        (
                            SearchFlags {
                                v3: groups.group(EXPERIMENTS[0]) == Some("t1"),
                                expanded: groups.group(EXPERIMENTS[1]) == Some("t1"),
                                dense: groups.group(EXPERIMENTS[2]) == Some("t1"),
                            },
                            groups.group(FOOTER_EXPERIMENT) == Some("t1"),
                        )
                    })
                    .or_else(|| {
                        previous
                            .as_ref()
                            .map(|current| (current.flags, current.footer))
                    })
                    .unwrap_or((old.flags, false));
                (flags, footer, true)
            }
        };
        save_assignment(
            &self.session,
            token,
            &Assignment {
                attempted_at: now(),
                flags,
            },
        )?;
        *self
            .search
            .native
            .lock()
            .map_err(|_| crate::Error::LockPoisoned)? = Some(NativeAssignment {
            token: token.clone(),
            api: Arc::downgrade(api),
            credentials: credentials.clone(),
            fetched: tokio::time::Instant::now(),
            flags,
            footer,
            optional_read_failed,
        });
        Ok(SearchAssignment {
            flags,
            footer_revision: footer.then_some(credentials),
            optional_read_failed,
        })
    }
}

fn assignment(value: &Value) -> Option<SearchFlags> {
    let rows = value["data"].as_array().filter(|rows| rows.len() <= 4096)?;
    let enabled = |experiment: &str| {
        rows.iter()
            .find(|row| row["expName"] == experiment)
            .is_some_and(|row| row["expGroupName"] == "t1")
    };
    Some(SearchFlags {
        v3: enabled(EXPERIMENTS[0]),
        expanded: enabled(EXPERIMENTS[1]),
        dense: enabled(EXPERIMENTS[2]),
    })
}

fn parse_tabs(value: &Value) -> Option<Vec<SearchCategory>> {
    let data = value["data"].as_object()?;
    let rows = data
        .get("algTabs")
        .filter(|v| !v.is_null())
        .or_else(|| data.get("defaultTabs"))?
        .as_array()?;
    let mut tabs = Vec::new();
    for row in rows.iter().take(64) {
        if let Some(kind) = row["title"].as_str().and_then(SearchCategory::from_title)
            && !tabs.contains(&kind)
        {
            tabs.push(kind);
        }
    }
    (!tabs.is_empty()).then_some(tabs)
}

impl SearchHost {
    pub(super) async fn assignment_for(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        api: &dyn Api,
    ) -> Result<SearchFlags> {
        if !self.enabled {
            return Ok(SearchFlags::default());
        }
        let _lock = self.gate.lock().await;
        let old = previous_assignment(session, token)?;
        let now = now();
        if now >= old.attempted_at && now - old.attempted_at < REFRESH_SECONDS {
            return Ok(old.flags);
        }
        let response = api
            .call(
                AB_PATH,
                json!({"expNames":EXPERIMENTS.join(",")}),
                false,
                &[200],
            )
            .await;
        let updated = Assignment {
            attempted_at: now,
            flags: response
                .as_ref()
                .ok()
                .and_then(assignment)
                .unwrap_or(old.flags),
        };
        save_assignment(session, token, &updated)?;
        Ok(updated.flags)
    }

    pub(super) fn selected_tabs(
        &self,
        session: &AccountSession,
        token: &SessionToken,
        response: Result<Value>,
    ) -> Result<Vec<SearchCategory>> {
        let store = session.persistent_store()?;
        session.with_token(token, |owner| {
            if let Some(tabs) = response.as_ref().ok().and_then(parse_tabs) {
                store.put(owner, NAMESPACE, "tabs", &serde_json::to_value(&tabs)?)?;
                return Ok(tabs);
            }
            Ok(store
                .get(owner, NAMESPACE, "tabs")?
                .and_then(|value| serde_json::from_value::<Vec<SearchCategory>>(value).ok())
                .filter(|tabs| !tabs.is_empty() && tabs.len() <= SearchCategory::ALL.len())
                .unwrap_or_else(|| SearchCategory::ALL.to_vec()))
        })
    }
}
