//! Linux research only: admit a fixed set of prepared PC reads before any I/O.
//! The caller supplies an independently selected UID and a nonredirecting,
//! egress-fenced transport. This module never constructs a network client.

#[path = "signed_window_read_gate/media.rs"]
mod media;
#[cfg(feature = "signed-window-read-proof")]
pub use media::MediaArm;

use cumulus_core::{
    Error, Result,
    account::credentials::Credentials,
    download::transfer::MediaUrl,
    protocol::{
        api::{ApiFuture, identifier, packed_ids},
        http::{
            APP_VERSION, ApiOrigin, BodyStatus, ClientProfile, HttpBackend, HttpReply,
            OFFICIAL_WINDOWS_USER_AGENT, RequestInput, RequestOptions, RequestPlan, WireRoute,
            build_request,
        },
        mconfig::Snapshot,
        pc_experiments::{ASSIGNMENT_PATH, PcExperimentGroups, assignment_arguments},
        wire::{eapi, serialization::trim_js},
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const ACCOUNT: &str = "/api/w/nuser/account/get";
const PLAYLISTS: &str = "/api/user/playlist";
const LIKES: &str = "/api/song/like/get";
const DEFAULT_WORD: &str = "/api/search/default/keyword/get";
const TABS: &str = "/api/search/pc/result/tab";
const V1: &str = "/api/search/pc/complex/page";
const V3: &str = "/api/search/pc/complex/page/v3";
const SONGS: &str = "/api/search/song/list/page";
const DETAIL: &str = "/api/v3/song/detail";
const SOURCE: &str = "/api/song/enhance/player/url/v1";
const MAX_API_BYTES: usize = 2 * 1024 * 1024;
const PAGE_SIZE: usize = 200;
const PAGE_LIMIT: usize = 16;
const SONG_PAGE_SIZE: usize = 10;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MEDIA_ARM_TIMEOUT: Duration = Duration::from_secs(12);
pub const MAX_MEDIA_BYTES: u64 = 16 * 1024 * 1024;

fn refused() -> Error {
    Error::RequestNotSent(Box::new(Error::State(
        "signed window read is outside scope",
    )))
}

fn bad_reply() -> Error {
    Error::Response("signed window read response refused")
}

fn canonical_id(value: &str) -> bool {
    (1..=20).contains(&value.len())
        && !value.starts_with('0')
        && value.bytes().all(|b| b.is_ascii_digit())
}

fn id(value: &Value) -> Result<String> {
    let result = identifier(value).map_err(|_| bad_reply())?;
    canonical_id(&result)
        .then_some(result)
        .ok_or_else(bad_reply)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Read {
    Account,
    Playlists,
    Likes,
    DefaultWord,
    Assignment,
    Tabs,
    Comprehensive,
    Songs,
    Detail,
    Source,
}

#[derive(Default)]
struct Progress {
    account: bool,
    playlists: HashSet<String>,
    playlists_done: bool,
    likes: Option<usize>,
    v3: Option<bool>,
    tab: bool,
    comprehensive: bool,
    selected_song_found: bool,
    selected_from_preview: bool,
    detail: bool,
    receipt: Option<(MediaUrl, MediaReceipt, Instant)>,
    attempts: [u8; 10],
    denied: usize,
    first_plan_rejection: Option<&'static str>,
    unfinished_read: Option<Read>,
    closed: bool,
    media_consumed: bool,
}

/// Does not retain or expose the URL, account, song ID, or original reply.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub expected_account_verified: bool,
    pub playlist_pages: u8,
    pub playlist_count: Option<usize>,
    pub liked_count: Option<usize>,
    pub search_v3: Option<bool>,
    pub search_tab_read: bool,
    pub comprehensive_read: bool,
    pub selected_song_found: bool,
    pub selected_from_preview: bool,
    pub song_detail_read: bool,
    pub media_authorized: bool,
    pub media_consumed: bool,
    pub attempted_reads: usize,
    pub denied_before_dispatch: usize,
    pub first_plan_rejection: Option<&'static str>,
    pub unfinished_read: Option<&'static str>,
    pub closed_after_failure: bool,
}

/// Private native data for the single credential-free CDN GET. Never serialize.
pub struct MediaReceipt {
    pub bytes: u64,
    pub md5: String,
    expires: Instant,
}

pub struct Gate {
    inner: Arc<dyn HttpBackend>,
    credentials: Arc<Credentials>,
    account_revision: String,
    account_device: String,
    account_token: String,
    profile: ClientProfile,
    mconfig_header: String,
    owner: String,
    keyword: String,
    song: String,
    preview_only: bool,
    progress: Mutex<Progress>,
}

fn unique_music_u(cookies: &[(String, String)]) -> Option<&str> {
    let mut tokens = cookies
        .iter()
        .filter_map(|(name, value)| name.eq_ignore_ascii_case("MUSIC_U").then_some(value));
    let token = tokens.next().filter(|value| !value.is_empty())?;
    tokens.next().is_none().then_some(token)
}

fn grouped_cookie_header(header: &str) -> Option<BTreeMap<&str, Vec<&str>>> {
    let mut cookies = BTreeMap::<_, Vec<_>>::new();
    for pair in header.split("; ") {
        let (name, value) = pair.split_once('=')?;
        if name.is_empty() {
            return None;
        }
        cookies.entry(name).or_default().push(value);
    }
    Some(cookies)
}

fn same_headers(expected: &BTreeMap<String, String>, actual: &BTreeMap<String, String>) -> bool {
    expected.len() == actual.len()
        && expected.iter().all(|(name, value)| {
            actual.get(name).is_some_and(|received| {
                if name == "Cookie" {
                    // CookieStore's hash iteration may reorder distinct
                    // names at equal path length between independent reads.
                    // Keep every value and same-name precedence exact.
                    grouped_cookie_header(value)
                        .zip(grouped_cookie_header(received))
                        .is_some_and(|(left, right)| left == right)
                } else {
                    value == received
                }
            })
        })
}

impl Read {
    fn label(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Playlists => "playlists",
            Self::Likes => "likes",
            Self::DefaultWord => "default_word",
            Self::Assignment => "assignment",
            Self::Tabs => "tabs",
            Self::Comprehensive => "comprehensive",
            Self::Songs => "songs",
            Self::Detail => "detail",
            Self::Source => "source",
        }
    }
}

impl Gate {
    pub fn new(
        inner: Arc<dyn HttpBackend>,
        credentials: Arc<Credentials>,
        profile: ClientProfile,
        snapshot: &Snapshot,
        owner: String,
        keyword: String,
        song: String,
    ) -> Result<Self> {
        if !canonical_id(&owner)
            || !canonical_id(&song)
            || keyword.is_empty()
            || keyword.chars().count() > 64
            || trim_js(&keyword) != keyword
            || keyword.chars().any(char::is_control)
            || snapshot.origin != ApiOrigin::Pc
            || snapshot.version == 0
            || !snapshot.data.as_ref().is_some_and(Value::is_object)
            || profile.user_agent != OFFICIAL_WINDOWS_USER_AGENT
            || profile.client_sign.as_deref().is_none_or(str::is_empty)
        {
            return Err(refused());
        }
        profile.validate().map_err(|_| refused())?;
        let account = credentials
            .request_credentials(&ApiOrigin::Pc.request_url(WireRoute::Eapi, ACCOUNT)?)?;
        let account_token = unique_music_u(&account.cookies)
            .ok_or_else(refused)?
            .to_owned();
        Ok(Self {
            inner,
            credentials,
            account_revision: account.revision,
            account_device: account.device_id,
            account_token,
            profile,
            mconfig_header: snapshot.request_header(APP_VERSION)?,
            owner,
            keyword,
            song,
            preview_only: false,
            progress: Mutex::default(),
        })
    }

    /// Select the ordinary comprehensive song card, without using the
    /// separately protected Songs tab or inventing an SDK token.
    pub fn with_comprehensive_track(mut self) -> Self {
        self.preview_only = true;
        self
    }

    pub async fn facts(&self) -> Facts {
        let p = self.progress.lock().await;
        Facts {
            expected_account_verified: p.account,
            playlist_pages: p.attempts[Read::Playlists as usize],
            playlist_count: p.playlists_done.then_some(p.playlists.len()),
            liked_count: p.likes,
            search_v3: p.v3,
            search_tab_read: p.tab,
            comprehensive_read: p.comprehensive,
            selected_song_found: p.selected_song_found,
            selected_from_preview: p.selected_from_preview,
            song_detail_read: p.detail,
            media_authorized: p.receipt.is_some() || p.media_consumed,
            media_consumed: p.media_consumed,
            attempted_reads: p.attempts.iter().map(|&value| usize::from(value)).sum(),
            denied_before_dispatch: p.denied,
            first_plan_rejection: p.first_plan_rejection,
            unfinished_read: p.unfinished_read.map(Read::label),
            closed_after_failure: p.closed,
        }
    }

    pub async fn take_media(&self, url: &MediaUrl) -> Result<MediaReceipt> {
        let mut p = self.progress.lock().await;
        if p.closed
            || !p.account
            || !p
                .receipt
                .as_ref()
                .is_some_and(|(allowed, _, expires)| allowed == url && Instant::now() < *expires)
        {
            return Err(refused());
        }
        self.require_original_account()?;
        let (_, receipt, _) = p.receipt.take().ok_or_else(refused)?;
        p.media_consumed = true;
        Ok(receipt)
    }

    fn permit(&self, p: &Progress, path: &str) -> Result<(Read, Value)> {
        if p.closed || (!p.account && path != ACCOUNT) {
            return Err(refused());
        }
        let selected = match path {
            ACCOUNT => (Read::Account, json!({})),
            PLAYLISTS if !p.playlists_done => (
                Read::Playlists,
                json!({
                    "uid":self.owner,"includeVideo":true,"limit":PAGE_SIZE,"offset":p.playlists.len()
                }),
            ),
            LIKES => (Read::Likes, json!({"uid":self.owner})),
            DEFAULT_WORD => (Read::DefaultWord, json!({})),
            ASSIGNMENT_PATH => (Read::Assignment, assignment_arguments()),
            TABS if p.v3 == Some(true) => (Read::Tabs, json!({"keyword":self.keyword})),
            V1 if p.v3 == Some(false) => (
                Read::Comprehensive,
                json!({
                    "keyword":self.keyword,"scene":"normal","needCorrect":true,"channel":"typing",
                    "cursor":"{\"page\":0,\"ignoreBlocks\":[],\"traceId\":\"\"}","bizQueryInfo":""
                }),
            ),
            V3 if p.v3 == Some(true) && p.tab => (
                Read::Comprehensive,
                json!({
                    "keyword":self.keyword,"scene":"normal","needCorrect":true,"channel":"typing",
                    "bizQueryInfo":""
                }),
            ),
            SONGS if p.comprehensive && !self.preview_only => (
                Read::Songs,
                json!({
                    "keyword":self.keyword,"scene":"normal","needCorrect":true,"channel":"typing",
                    "limit":SONG_PAGE_SIZE,"offset":0
                }),
            ),
            DETAIL if p.selected_song_found => (
                Read::Detail,
                json!({
                    "c":json!([{"id":self.song,"v":0}]).to_string()
                }),
            ),
            SOURCE if p.detail => (
                Read::Source,
                json!({
                    "ids":packed_ids(&[json!(self.song)])?,"level":"standard","encodeType":"aac",
                    "immerseType":"c51","trialMode":42
                }),
            ),
            _ => return Err(refused()),
        };
        let cap = if selected.0 == Read::Playlists {
            PAGE_LIMIT as u8
        } else {
            1
        };
        (p.attempts[selected.0 as usize] < cap)
            .then_some(selected)
            .ok_or_else(refused)
    }

    fn check_plan(
        &self,
        p: &Progress,
        plan: &RequestPlan,
    ) -> std::result::Result<Read, &'static str> {
        let (path, body) = decode_plan(plan).map_err(|_| "envelope")?;
        let (read, args) = self.permit(p, &path).map_err(|_| "allowlist")?;
        let scoped = self.scoped(&plan.url).map_err(|_| "credential_scope")?;
        // Reconstruct the exact current production frontend wire. This checks
        // the full scoped jar, UA, native sign/device and official conversion,
        // and refuses any extra fields, path/cache overrides, or SDK headers.
        let mut expected = build_request(RequestInput {
            origin: ApiOrigin::Pc,
            path: &path,
            payload: args,
            credentials: &scoped,
            profile: &self.profile,
            extra_headers: &Default::default(),
            options: RequestOptions {
                encrypt_response: plan.decrypt_response,
                ..Default::default()
            },
        })
        .map_err(|_| "reconstruction")?;
        expected
            .headers
            .insert("MConfig-Info".into(), self.mconfig_header.clone());
        if !same_headers(&expected.headers, &plan.headers) {
            return Err("headers");
        }
        if decode_plan(&expected).map_err(|_| "reconstruction")?.1 != body {
            return Err("payload");
        }
        Ok(read)
    }

    fn require_original_account(&self) -> Result<()> {
        self.scoped(&ApiOrigin::Pc.request_url(WireRoute::Eapi, ACCOUNT)?)?;
        Ok(())
    }

    fn scoped(
        &self,
        url: &url::Url,
    ) -> Result<cumulus_core::account::credentials::RequestCredentials> {
        let scoped = self
            .credentials
            .request_credentials(url)
            .map_err(|_| refused())?;
        if scoped.revision != self.account_revision
            || scoped.device_id != self.account_device
            || unique_music_u(&scoped.cookies) != Some(self.account_token.as_str())
        {
            return Err(refused());
        }
        Ok(scoped)
    }

    fn observe(&self, p: &mut Progress, read: Read, value: &Value) -> Result<()> {
        match read {
            Read::Account => {
                if id(&value["profile"]["userId"])? != self.owner {
                    return Err(bad_reply());
                }
                p.account = true;
            }
            Read::Playlists => {
                let rows = array(&value["playlist"], PAGE_SIZE)?;
                let more = match (value.get("more"), value.get("hasMore")) {
                    (Some(a), Some(b)) if a != b => return Err(bad_reply()),
                    (Some(value), _) | (None, Some(value)) if value.is_null() => {
                        rows.len() == PAGE_SIZE
                    }
                    (Some(value), _) | (None, Some(value)) => {
                        value.as_bool().ok_or_else(bad_reply)?
                    }
                    (None, None) => rows.len() == PAGE_SIZE,
                };
                if more
                    && (rows.is_empty()
                        || usize::from(p.attempts[Read::Playlists as usize]) == PAGE_LIMIT)
                {
                    return Err(bad_reply());
                }
                for row in rows {
                    if !p.playlists.insert(id(&row["id"])?) {
                        return Err(bad_reply());
                    }
                }
                p.playlists_done = !more;
            }
            Read::Likes => {
                let rows = array(&value["ids"], 100_000)?;
                let mut ids = HashSet::with_capacity(rows.len());
                for row in rows {
                    if !ids.insert(id(row)?) {
                        return Err(bad_reply());
                    }
                }
                p.likes = Some(ids.len());
            }
            Read::DefaultWord => (),
            Read::Assignment => {
                let groups = PcExperimentGroups::refreshed(value).map_err(|_| bad_reply())?;
                p.v3 = Some(groups.group("PH-PC-searchHot") == Some("t1"));
            }
            Read::Tabs => {
                let data = &value["data"];
                let rows = data
                    .get("algTabs")
                    .filter(|value| !value.is_null())
                    .or_else(|| data.get("defaultTabs"))
                    .ok_or_else(bad_reply)?;
                if array(rows, 64)?.is_empty() {
                    return Err(bad_reply());
                }
                p.tab = true;
            }
            Read::Comprehensive => {
                let blocks = array(&value["data"]["blocks"], 100)?;
                if self.preview_only {
                    let v3 = p.v3 == Some(true);
                    p.selected_from_preview = blocks.iter().take(50).any(|block| {
                        let ordinary = matches!(
                            block["blockCode"].as_str(),
                            Some("search_block_song" | "search_block_song_rec")
                        );
                        let mixed = block["blockCode"] == "search_block_best_match";
                        (ordinary || mixed)
                            && block["resources"].as_array().is_some_and(|rows| {
                                rows.iter().take(100).any(|row| {
                                    if (!ordinary || !v3) && row["type"] != "song" {
                                        return false;
                                    }
                                    let base = &row["baseInfo"];
                                    let candidate = if v3 {
                                        &base["simpleSongData"]["id"]
                                    } else {
                                        &base["id"]
                                    };
                                    id(candidate).is_ok_and(|id| id == self.song)
                                })
                            })
                    });
                    p.selected_song_found = p.selected_from_preview;
                }
                p.comprehensive = true;
            }
            Read::Songs => {
                let matches = array(&value["data"]["resources"], SONG_PAGE_SIZE)?
                    .iter()
                    .filter(|row| {
                        id(&row["baseInfo"]["simpleSongData"]["id"]).is_ok_and(|id| id == self.song)
                    })
                    .count();
                if matches > 1 {
                    return Err(bad_reply());
                }
                p.selected_song_found = matches == 1;
            }
            Read::Detail => {
                let rows = array(&value["songs"], 1)?;
                if rows.len() != 1 || id(&rows[0]["id"])? != self.song {
                    return Err(bad_reply());
                }
                p.detail = true;
            }
            Read::Source => {
                let rows = array(&value["data"], 1)?;
                let row = rows.first().ok_or_else(bad_reply)?;
                if id(&row["id"])? != self.song || !success(row) || !row["freeTrialInfo"].is_null()
                {
                    return Err(bad_reply());
                }
                let url = MediaUrl::parse(row["url"].as_str().ok_or_else(bad_reply)?)
                    .map_err(|_| bad_reply())?;
                if url::Url::parse(url.as_str())
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_owned))
                    == ApiOrigin::Pc.url().host_str().map(str::to_owned)
                {
                    return Err(bad_reply());
                }
                let bytes = row["size"]
                    .as_u64()
                    .filter(|size| (1..=MAX_MEDIA_BYTES).contains(size))
                    .ok_or_else(bad_reply)?;
                let md5 = row["md5"]
                    .as_str()
                    .filter(|value| {
                        value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                    .ok_or_else(bad_reply)?
                    .to_ascii_lowercase();
                let expiry = match row.get("expi") {
                    Some(value) => Duration::from_secs(
                        value.as_u64().filter(|&v| v > 0).ok_or_else(bad_reply)?,
                    ),
                    None => REQUEST_TIMEOUT,
                };
                let now = Instant::now();
                p.receipt = Some((
                    url,
                    MediaReceipt {
                        bytes,
                        md5,
                        expires: now + expiry.min(REQUEST_TIMEOUT + MEDIA_ARM_TIMEOUT),
                    },
                    now + expiry.min(REQUEST_TIMEOUT),
                ));
            }
        }
        Ok(())
    }
}

fn array(value: &Value, max: usize) -> Result<&[Value]> {
    value
        .as_array()
        .filter(|items| items.len() <= max)
        .map(Vec::as_slice)
        .ok_or_else(bad_reply)
}

fn decode_plan(plan: &RequestPlan) -> Result<(String, Value)> {
    if plan.route != WireRoute::Eapi || plan.body.len() > 65_536 {
        return Err(refused());
    }
    let params = std::str::from_utf8(&plan.body)
        .ok()
        .and_then(|form| form.strip_prefix("params="))
        .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(refused)?;
    let (path, body) = eapi::unwrap(params).map_err(|_| refused())?;
    if ApiOrigin::Pc
        .request_url(WireRoute::Eapi, &path)
        .map_err(|_| refused())?
        != plan.url
    {
        return Err(refused());
    }
    Ok((path, body))
}

fn success(value: &Value) -> bool {
    value["code"] == 200 || value["code"] == "200"
}

fn inspect_reply(reply: &HttpReply, encrypted: bool) -> Result<Value> {
    if reply.status != 200
        || reply.body_status != BodyStatus::Complete
        || reply.body.len() > MAX_API_BYTES
        || reply.refresh_token.is_some()
        || reply.set_cookies.iter().any(|cookie| {
            cookie.split_once('=').is_none_or(|(name, _)| {
                ["MUSIC_U", "MUSIC_A", "MUSIC_TU"]
                    .iter()
                    .any(|key| name.trim().eq_ignore_ascii_case(key))
            })
        })
    {
        return Err(bad_reply());
    }
    let value = match serde_json::from_slice(&reply.body) {
        Ok(value) => value,
        Err(_) if encrypted => {
            let plain = eapi::decrypt_response(&reply.body).map_err(|_| bad_reply())?;
            serde_json::from_slice(&plain).map_err(|_| bad_reply())?
        }
        Err(_) => return Err(bad_reply()),
    };
    success(&value).then_some(value).ok_or_else(bad_reply)
}

impl HttpBackend for Gate {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            let mut p = self.progress.lock().await;
            let read = self.check_plan(&p, &plan).map_err(|reason| {
                p.denied = p.denied.saturating_add(1);
                p.first_plan_rejection.get_or_insert(reason);
                refused()
            })?;
            p.attempts[read as usize] += 1;
            let encrypted = plan.decrypt_response;
            // Keep the failure latch set across await. A cancelled or
            // incomplete attempt can never reopen the gate or retry.
            p.closed = true;
            p.unfinished_read = Some(read);
            let mut reply = tokio::time::timeout(REQUEST_TIMEOUT, self.inner.post(plan))
                .await
                .map_err(|_| Error::Transport("signed window read timed out".into()))?
                .map_err(|_| Error::Transport("signed window read transport refused".into()))?;
            let value = inspect_reply(&reply, encrypted)?;
            self.observe(&mut p, read, &value)?;
            p.closed = false;
            p.unfinished_read = None;
            // Preserve the original body and encryption contract. Do not
            // apply response Cookie mutations or negotiate fresh PC config.
            reply.set_cookies.clear();
            reply.metadata.retain(|name, _| {
                !name.eq_ignore_ascii_case("mconfig-info")
                    && !name.eq_ignore_ascii_case("mconfig-bucket")
            });
            Ok(reply)
        })
    }
}
