//! No network. Exercises the production Interface QR and WEAPI dispatch.
use cumulus_core::{
    Error,
    protocol::{
        api::ApiFuture,
        http::{ApiOrigin, BodyStatus, HttpBackend, HttpReply, RequestPlan, WireRoute},
        wire::eapi,
    },
    storage::StateStore,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

pub fn enabled() -> bool {
    std::env::var_os("CUMULUS_EXTERNAL_IMPORT_PROOF").is_some()
}

pub struct Wire {
    store: Arc<StateStore>,
    requests: Mutex<Vec<String>>,
    polls: AtomicUsize,
    sends: AtomicUsize,
}

impl Wire {
    pub fn new(store: Arc<StateStore>) -> Self {
        Self {
            store,
            requests: Mutex::default(),
            polls: AtomicUsize::new(0),
            sends: AtomicUsize::new(0),
        }
    }

    pub fn report(&self) -> Value {
        json!({
            "requests":*self.requests.lock().unwrap(),
            "dispatches":self.sends.load(Ordering::Acquire),
        })
    }
}

fn reply(body: Value, cookies: Vec<String>) -> HttpReply {
    HttpReply {
        status: 200,
        body: serde_json::to_vec(&body).unwrap(),
        body_status: BodyStatus::Complete,
        set_cookies: cookies,
        refresh_token: None,
        metadata: BTreeMap::new(),
    }
}

impl HttpBackend for Wire {
    fn post(&self, plan: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            assert_eq!(plan.url.origin(), ApiOrigin::Interface.url().origin());
            let form: BTreeMap<_, _> = url::form_urlencoded::parse(&plan.body).collect();
            match (plan.route, plan.url.path()) {
                (WireRoute::Weapi, "/weapi/playlist/import/name/task/create") => {
                    let cookies = &plan.headers["Cookie"];
                    assert!(cookies.contains("MUSIC_U=fixture-interface"));
                    assert!(cookies.contains("__csrf=fixture-csrf"));
                    assert_eq!(plan.url.query(), Some("csrf_token=fixture-csrf"));
                    assert_eq!(plan.headers["Origin"], "https://mp.music.163.com");
                    assert_eq!(form.len(), 2);
                    assert_eq!(form["encSecKey"].len(), 256);
                    assert!(!form["params"].is_empty());
                    let row = self
                        .store
                        .get("2", "external-playlist-import-v1", "latest")?
                        .unwrap();
                    assert_eq!(row["phase"]["state"], "unknown");
                    let n = self.sends.fetch_add(1, Ordering::AcqRel);
                    self.requests.lock().unwrap().push("submit".into());
                    Ok(reply(
                        match n {
                            0 => {
                                json!({"code":200,"data":{"taskId":"fixture-task-id-must-not-leak"}})
                            }
                            1 => json!({"code":200,"data":{}}),
                            _ => panic!("unexpected import replay"),
                        },
                        vec![],
                    ))
                }
                (WireRoute::Eapi, url_path) => {
                    assert_eq!(form.len(), 1);
                    let (path, body) = eapi::unwrap(&form["params"])?;
                    assert_eq!(url_path, format!("/eapi/{}", &path[5..]));
                    self.requests.lock().unwrap().push(path.clone());
                    match path.as_str() {
                        "/api/login/qrcode/unikey" => {
                            assert_eq!(body["type"], 3);
                            Ok(reply(
                                json!({"code":200,"unikey":"synthetic-import-code"}),
                                vec![],
                            ))
                        }
                        "/api/login/qrcode/client/login" => {
                            assert_eq!(body["type"], 3);
                            assert_eq!(body["key"], "synthetic-import-code");
                            match self.polls.fetch_add(1, Ordering::AcqRel) {
                                0 => Ok(reply(json!({"code":801}), vec![])),
                                1 => Ok(reply(json!({"code":802}), vec![])),
                                2 => Ok(reply(
                                    json!({"code":803}),
                                    vec![
                                        "MUSIC_U=fixture-interface; Path=/; Secure; HttpOnly"
                                            .into(),
                                        "__csrf=fixture-csrf; Domain=music.163.com; Path=/; Secure"
                                            .into(),
                                    ],
                                )),
                                _ => panic!("unexpected QR replay"),
                            }
                        }
                        "/api/w/nuser/account/get" => {
                            assert!(plan.headers["Cookie"].contains("MUSIC_U=fixture-interface"));
                            Ok(reply(json!({"code":200,"profile":{"userId":2}}), vec![]))
                        }
                        _ => Err(Error::State("unexpected import authorization endpoint")),
                    }
                }
                _ => Err(Error::State("unexpected external import route")),
            }
        })
    }
}
