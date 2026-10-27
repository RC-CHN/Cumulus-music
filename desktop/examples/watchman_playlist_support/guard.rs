use super::*;
use cumulus_core::protocol::{
    api::{ApiFuture, identifier},
    wire::{eapi, serialization::official_data},
};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Permit {
    label: String,
    path: &'static str,
    fields: Value,
    route: WireRoute,
}
impl Permit {
    pub fn command(label: &str, command: &PlaylistCommand, owner: &str) -> Result<Self> {
        let (path, fields) = match command {
            PlaylistCommand::Create {
                name,
                private: true,
            } => (
                "/api/playlist/create",
                json!({"uid":owner,"name":name,"privacy":10}),
            ),
            PlaylistCommand::Metadata {
                id,
                name,
                tags,
                description,
                ..
            } => {
                let mut fields = json!({});
                for (path, value) in [
                    ("/api/playlist/update/name", json!({"id":id,"name":name})),
                    (
                        "/api/playlist/tags/update",
                        json!({"id":id,"tags":tags.join(";")}),
                    ),
                    (
                        "/api/playlist/desc/update",
                        json!({"id":id,"desc":description}),
                    ),
                ] {
                    fields[path] = serde_json::to_string(&value)?.into();
                }
                ("/api/batch", fields)
            }
            PlaylistCommand::Tracks {
                id, change, ids, ..
            } => {
                let mut fields = json!({"pid":id,"trackIds":serde_json::to_string(ids)?,"op":match change{TrackChange::Add=>"add",TrackChange::Delete=>"del",TrackChange::Reorder=>"update",_=>return Err(Error::State("explicit track list required in proof"))}});
                if *change != TrackChange::Reorder {
                    fields["imme"] = json!(true);
                }
                ("/api/v1/playlist/manipulate/tracks", fields)
            }
            PlaylistCommand::Order { ids, .. } => (
                "/api/playlist/order/update",
                json!({"ids":serde_json::to_string(ids)?}),
            ),
            PlaylistCommand::Delete { id, .. } => ("/api/playlist/delete", json!({"pid":id})),
            _ => return Err(Error::State("unreviewed playlist proof command")),
        };
        Ok(Self {
            label: label.into(),
            path,
            fields: official_data(&fields)?,
            route: WireRoute::Eapi,
        })
    }
    pub fn cleanup(id: &str) -> Self {
        Self {
            label: "cleanup".into(),
            path: "/api/playlist/delete",
            fields: json!({"pid":id}),
            route: WireRoute::Weapi,
        }
    }
}
pub struct Backend {
    inner: ReqwestBackend,
    root: PathBuf,
    pending: Mutex<Option<Permit>>,
    pub owner: Mutex<String>,
    serial: AtomicUsize,
}
impl Backend {
    pub fn new(root: PathBuf) -> Result<Self> {
        let serial = fs::read_dir(&root)?
            .filter_map(|v| v.ok())
            .filter_map(|e| {
                e.file_name()
                    .to_str()?
                    .strip_prefix("playlist-request-")?
                    .strip_suffix(".json")?
                    .parse::<usize>()
                    .ok()
            })
            .max()
            .map_or(0, |n| n + 1);
        Ok(Self {
            inner: ReqwestBackend::new(Duration::from_secs(20))?,
            root,
            pending: Mutex::new(None),
            owner: Mutex::new(String::new()),
            serial: AtomicUsize::new(serial),
        })
    }
    pub fn permit(&self, permit: Permit) -> Result<()> {
        let mut pending = self.pending.lock().map_err(|_| Error::LockPoisoned)?;
        if pending.is_some() {
            return Err(Error::State("previous proof permit remains active"));
        }
        save(
            &self.root,
            &format!("permit-{}.json", permit.label),
            &json!({"path":permit.path,"fields":permit.fields,"route":if permit.route==WireRoute::Eapi{"eapi"}else{"weapi"}}),
        )?;
        *pending = Some(permit);
        Ok(())
    }
    pub fn disarm(&self) -> Result<()> {
        *self.pending.lock().map_err(|_| Error::LockPoisoned)? = None;
        Ok(())
    }
    pub fn write_count(&self) -> Result<usize> {
        Ok(fs::read_dir(&self.root)?
            .collect::<std::io::Result<Vec<_>>>()?
            .iter()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with("sent-") && n.ends_with(".json"))
            })
            .count())
    }
    fn claim(&self, path: &str, payload: &Value, route: WireRoute) -> Result<String> {
        let mut pending = self.pending.lock().map_err(|_| Error::LockPoisoned)?;
        let permit = pending
            .as_ref()
            .ok_or(Error::State("mutation has no current proof permit"))?;
        let mut fields = payload.clone();
        let object = fields
            .as_object_mut()
            .ok_or(Error::State("invalid proof fields"))?;
        object.remove("header");
        object.remove("checkToken");
        if permit.path != path || permit.route != route || fields != permit.fields {
            return Err(Error::State("mutation differs from exact proof intent"));
        }
        save(
            &self.root,
            &format!("sent-{}.json", permit.label),
            &json!({"path":path,"fields":fields,"route":if route==WireRoute::Eapi{"eapi"}else{"weapi"}}),
        )?;
        Ok(pending.take().unwrap().label)
    }
}
impl HttpBackend for Backend {
    fn post(&self, request: RequestPlan) -> ApiFuture<'_, HttpReply> {
        Box::pin(async move {
            ApiOrigin::Pc.require_url(&request.url)?;
            let (path, payload) = if request.route == WireRoute::Eapi {
                let s = std::str::from_utf8(&request.body)
                    .map_err(|_| Error::State("invalid proof EAPI form"))?;
                eapi::unwrap(
                    s.strip_prefix("params=")
                        .ok_or(Error::State("missing EAPI params"))?,
                )?
            } else {
                let p = self.pending.lock().map_err(|_| Error::LockPoisoned)?;
                let p = p.as_ref().ok_or(Error::State("no cleanup permit"))?;
                (p.path.into(), p.fields.clone())
            };
            if request.url != ApiOrigin::Pc.request_url(request.route, &path)? {
                return Err(Error::State("proof request URL differs"));
            }
            let label = match path.as_str() {
                "/api/w/nuser/account/get" => "read".into(),
                "/api/user/playlist" => {
                    if identifier(&payload["uid"])?
                        != *self.owner.lock().map_err(|_| Error::LockPoisoned)?
                    {
                        return Err(Error::State("foreign list read denied"));
                    }
                    "read".into()
                }
                "/api/v6/playlist/detail" => "read".into(),
                _ => self.claim(&path, &payload, request.route)?,
            };
            if label == "read" && request.route != WireRoute::Eapi {
                return Err(Error::State("unexpected compatibility read"));
            }
            let n = self.serial.fetch_add(1, Ordering::SeqCst);
            save(
                &self.root,
                &format!("playlist-request-{n}.json"),
                &json!({"path":path,"payload":payload,"intent":label,"route":if request.route==WireRoute::Eapi{"eapi"}else{"weapi"},"mconfig":request.headers.get("MConfig-Info"),"antiCheatToken":request.headers.get("X-antiCheatToken")}),
            )?;
            match self.inner.post(request).await {
                Ok(reply) => {
                    let body = serde_json::from_slice::<Value>(&reply.body)
                        .or_else(|_| {
                            eapi::decrypt_response(&reply.body)
                                .map_err(|_| ())
                                .and_then(|b| serde_json::from_slice(&b).map_err(|_| ()))
                        })
                        .ok();
                    save(
                        &self.root,
                        &format!("playlist-response-{n}.json"),
                        &json!({"status":reply.status,"body":body,"metadata":reply.metadata}),
                    )?;
                    Ok(reply)
                }
                Err(e) => {
                    save(
                        &self.root,
                        &format!("playlist-error-{n}.json"),
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
    #[test]
    fn exact_intent_rejects_other_resource_and_duplicate_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let backend = Backend::new(dir.path().into()).unwrap();
        let c = PlaylistCommand::Delete {
            id: "7".into(),
            revision: "0".repeat(64),
        };
        backend
            .permit(Permit::command("delete", &c, "1").unwrap())
            .unwrap();
        assert!(
            backend
                .claim("/api/playlist/delete", &json!({"pid":"8"}), WireRoute::Eapi)
                .is_err()
        );
        assert!(
            backend
                .claim(
                    "/api/playlist/delete",
                    &json!({"pid":"7"}),
                    WireRoute::Weapi
                )
                .is_err()
        );
        assert_eq!(
            backend
                .claim(
                    "/api/playlist/delete",
                    &json!({"pid":"7","header":"{}","checkToken":"native"}),
                    WireRoute::Eapi
                )
                .unwrap(),
            "delete"
        );
        assert!(
            backend
                .claim("/api/playlist/delete", &json!({"pid":"7"}), WireRoute::Eapi)
                .is_err()
        );
        let reopened = Backend::new(dir.path().into()).unwrap();
        assert!(
            reopened
                .permit(Permit::command("delete", &c, "1").unwrap())
                .is_err()
        );
        assert_eq!(reopened.write_count().unwrap(), 1);
    }
}
