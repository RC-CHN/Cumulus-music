use super::*;
use cumulus_core::protocol::{
    api::{ApiFuture, identifier},
    wire::eapi,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
pub struct Scope {
    pub name: String,
    pub owner: String,
    pub id: Option<String>,
    pub create_sent: bool,
    pub cleanup: bool,
}
pub struct Backend {
    inner: ReqwestBackend,
    root: PathBuf,
    pub scope: Mutex<Scope>,
    serial: AtomicUsize,
    pub creates: AtomicUsize,
    pub deletes: AtomicUsize,
}
impl Backend {
    pub fn new(root: PathBuf, scope: Scope) -> Result<Self> {
        let serial = fs::read_dir(&root)?
            .map(|entry| {
                let name = entry?.file_name();
                Ok(name.to_str().and_then(|name| {
                    name.strip_prefix("write-request-")?
                        .strip_suffix(".json")?
                        .parse::<usize>()
                        .ok()
                }))
            })
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .max()
            .map_or(0, |n| n + 1);
        let creates = usize::from(root.join("write-create-intent.json").exists());
        let deletes = usize::from(root.join("write-delete-intent.json").exists());
        Ok(Self {
            inner: ReqwestBackend::new(Duration::from_secs(20))?,
            root,
            scope: Mutex::new(scope),
            serial: AtomicUsize::new(serial),
            creates: AtomicUsize::new(creates),
            deletes: AtomicUsize::new(deletes),
        })
    }
    // Durable before HTTP: a crash or a lost reply never permits another send.
    fn reserve(&self, operation: &str, value: &Value) -> Result<()> {
        save_new(&self.root, &format!("write-{operation}-intent.json"), value)
    }
}
impl HttpBackend for Backend {
    fn post(&self, request: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            if request.url.scheme() != "https"
                || request.url.host_str() != Some("interfacepc.music.163.com")
            {
                return Err(Error::State("write proof origin mismatch"));
            }
            let mut intent = "read";
            let (path, payload) = if request.route == WireRoute::Eapi {
                let form = std::str::from_utf8(&request.body)
                    .map_err(|_| Error::State("invalid EAPI form"))?;
                let (path, payload) = eapi::unwrap(
                    form.strip_prefix("params=")
                        .ok_or(Error::State("missing EAPI params"))?,
                )?;
                {
                    let mut scope = self.scope.lock().map_err(|_| Error::LockPoisoned)?;
                    match path.as_str() {
                        "/api/w/nuser/account/get" => (),
                        "/api/user/playlist" => {
                            if identifier(&payload["uid"])? != scope.owner {
                                return Err(Error::State("foreign playlist list denied"));
                            }
                        }
                        "/api/v6/playlist/detail" => (),
                        "/api/playlist/create" => {
                            if scope.create_sent
                                || identifier(&payload["uid"])? != scope.owner
                                || payload["name"] != scope.name
                                || ![json!(10), json!("10")].contains(&payload["privacy"])
                            {
                                return Err(Error::State("create outside private proof scope"));
                            }
                            if payload["checkToken"].as_str()
                                != request.headers.get("X-antiCheatToken").map(String::as_str)
                                || payload["checkToken"].as_str().is_none_or(str::is_empty)
                            {
                                return Err(Error::State("official create requires fresh token"));
                            }
                            self.reserve(
                                "create",
                                &json!({"owner":scope.owner,"name":scope.name,"private":true}),
                            )?;
                            scope.create_sent = true;
                            self.creates.fetch_add(1, Ordering::SeqCst);
                            intent = "isolated_private_create";
                        }
                        _ => return Err(Error::State("unreviewed write proof route")),
                    }
                }
                (path, payload)
            } else {
                let scope = self.scope.lock().map_err(|_| Error::LockPoisoned)?;
                if request.url.as_str() != "https://interfacepc.music.163.com/weapi/playlist/delete"
                    || !scope.cleanup
                    || scope.id.is_none()
                    || self.deletes.load(Ordering::SeqCst) != 0
                {
                    return Err(Error::State("cleanup outside one verified resource"));
                }
                self.reserve(
                    "delete",
                    &json!({"owner":scope.owner,"name":scope.name,"id":scope.id}),
                )?;
                self.deletes.store(1, Ordering::SeqCst);
                intent = "preselected_cleanup_delete";
                (
                    "/api/playlist/delete".into(),
                    json!({"pid":scope.id,"body":"WEAPI encrypted by native client"}),
                )
            };
            let n = self.serial.fetch_add(1, Ordering::SeqCst);
            save_new(
                &self.root,
                &format!("write-request-{n}.json"),
                &json!({"path":path,"intent":intent,"route":if request.route==WireRoute::Eapi{"eapi"}else{"weapi"},"payload":payload,"mconfig":request.headers.get("MConfig-Info"),"antiCheatToken":request.headers.get("X-antiCheatToken")}),
            )?;
            let response = self.inner.post(request).await;
            match response {
                Ok(reply) => {
                    let body = serde_json::from_slice::<Value>(&reply.body)
                        .or_else(|_| {
                            eapi::decrypt_response(&reply.body)
                                .map_err(|_| ())
                                .and_then(|p| serde_json::from_slice(&p).map_err(|_| ()))
                        })
                        .ok();
                    save_new(
                        &self.root,
                        &format!("write-response-{n}.json"),
                        &json!({"status":reply.status,"metadata":reply.metadata,"body":body}),
                    )?;
                    Ok(reply)
                }
                Err(e) => {
                    save_new(
                        &self.root,
                        &format!("write-transfer-error-{n}.json"),
                        &json!({"error":e.to_string()}),
                    )?;
                    Err(e)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            name: "proof".into(),
            owner: "1".into(),
            id: None,
            create_sent: true,
            cleanup: false,
        }
    }
    #[test]
    fn reopening_keeps_durable_intents_and_appends_without_overwriting_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::new(dir.path().into(), scope()).unwrap();
        backend.reserve("create", &json!({"private":true})).unwrap();
        backend.reserve("delete", &json!({"id":"7"})).unwrap();
        save_new(
            dir.path(),
            "write-request-8.json",
            &json!({"intent":"read"}),
        )
        .unwrap();
        let reopened = Backend::new(dir.path().into(), scope()).unwrap();
        assert_eq!(reopened.serial.load(Ordering::SeqCst), 9);
        assert_eq!(reopened.creates.load(Ordering::SeqCst), 1);
        assert_eq!(reopened.deletes.load(Ordering::SeqCst), 1);
        assert!(reopened.reserve("create", &json!({})).is_err());
        assert!(reopened.reserve("delete", &json!({})).is_err());
        assert!(save_new(dir.path(), "write-request-8.json", &json!({})).is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("write-request-8.json")).unwrap(),
            "{\n  \"intent\": \"read\"\n}"
        );
    }
}
