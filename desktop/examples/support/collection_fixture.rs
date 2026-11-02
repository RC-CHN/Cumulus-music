//! Collection page fault injection, confined to the explicit WebView example.
use cumulus_core::{
    Error,
    protocol::api::{Api, ApiFuture},
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};
pub struct Library {
    base: super::fixture::Library,
    mode: AtomicU8,
    calls: Mutex<Vec<Value>>,
    pending: AtomicBool,
    delayed: AtomicU64,
    delayed_first: AtomicU64,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            base: super::fixture::Library::default(),
            mode: AtomicU8::new(0),
            calls: Mutex::new(Vec::new()),
            pending: AtomicBool::new(false),
            delayed: AtomicU64::new(0),
            delayed_first: AtomicU64::new(0),
        }
    }
}
impl Library {
    pub fn mode(&self, mode: &str) {
        self.mode.store(
            match mode {
                "fail_saved" => 1,
                "fail_more" => 2,
                "fail_purchase" => 3,
                "empty_purchase" => 4,
                "empty_saved" => 5,
                "fail_follow" => 6,
                "empty_follow" => 7,
                "delay_more" => 8,
                "expire" => 9,
                "delay_first" => 10,
                _ => 0,
            },
            Ordering::SeqCst,
        );
    }
    pub fn report(&self) -> Value {
        json!({"calls":self.calls.lock().unwrap().clone(),"pending":self.pending.load(Ordering::SeqCst),"delayedCompleted":self.delayed.load(Ordering::SeqCst),"delayedFirstCompleted":self.delayed_first.load(Ordering::SeqCst)})
    }
}
fn album(id: u64) -> Value {
    json!({"id":id,"name":if id==106 {"臺灣 Ａ [Live]".into()}else{format!("收藏专辑 {id}")},"artists":[{"id":12,"name":"测试歌手"}],"artist":{"id":13,"name":"主要署名"},"size":11,"publishTime":1700000000000_i64})
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            let mode = self.mode.load(Ordering::SeqCst);
            if [
                "/api/album/sublist",
                "/api/digitalAlbum/purchased",
                "/api/user/follow/users/mixed/get/v2",
            ]
            .contains(&path)
            {
                self.calls
                    .lock()
                    .unwrap()
                    .push(json!({"path":path,"args":args,"mode":mode}));
                if mode == 9 {
                    return Ok(json!({"code":301}));
                }
            }
            match path {
                "/api/album/sublist" => {
                    let offset = args["offset"].as_u64().unwrap_or(0);
                    let limit = args["limit"].as_u64().unwrap_or(50);
                    if mode == 10 && offset == 0 {
                        self.pending.store(true, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                        self.pending.store(false, Ordering::SeqCst);
                        self.delayed_first.fetch_add(1, Ordering::SeqCst);
                    }
                    if mode == 8 && offset > 0 {
                        self.pending.store(true, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                        self.pending.store(false, Ordering::SeqCst);
                        self.delayed.fetch_add(1, Ordering::SeqCst);
                    }
                    if mode == 1 || mode == 2 && offset > 0 {
                        return Ok(json!({"code":503}));
                    }
                    if mode == 5 {
                        return Ok(json!({"code":200,"data":[],"hasMore":false,"count":0}));
                    }
                    Ok(
                        json!({"code":200,"data":(offset..(offset+limit).min(123)).map(|n|album(n+7)).collect::<Vec<_>>(),"hasMore":offset+limit<123,"count":123}),
                    )
                }
                "/api/digitalAlbum/purchased" => {
                    if mode == 3 {
                        return Ok(json!({"code":503}));
                    }
                    let rows = if mode == 4 {
                        vec![]
                    } else {
                        vec![album(1001), album(1002)]
                    };
                    Ok(json!({"code":200,"total":rows.len(),"paidAlbums":rows}))
                }
                "/api/user/follow/users/mixed/get/v2" => {
                    if mode == 6 {
                        return Ok(json!({"code":503}));
                    }
                    if mode == 7 {
                        return Ok(
                            json!({"code":200,"data":{"records":[],"page":{"more":false,"cursor":"0","size":19}}}),
                        );
                    }
                    let cursor = args["page"]["cursor"].as_str().unwrap_or("0");
                    let offset = if cursor == "0" {
                        0
                    } else {
                        cursor
                            .strip_prefix("90071992547409931234:")
                            .and_then(|v| v.parse::<usize>().ok())
                            .ok_or(Error::InvalidInput("unexpected fixture cursor"))?
                    };
                    let end = (offset + 20).min(205);
                    let rows=(offset..end).map(|n|if n==204 {json!({"userProfile":{"userId":999,"nickname":"仅用户资料"}})}else{json!({"artistInfo":{"id":n+12,"name":format!("关注歌手 {}",n+1)},"userProfile":if n%2==0{json!({"userId":n+99,"nickname":"关联用户","signature":"合成关注记录，用于检查布局"})}else{Value::Null}})}).collect::<Vec<_>>();
                    Ok(
                        json!({"code":200,"data":{"records":rows,"page":{"more":end<205,"cursor":format!("90071992547409931234:{end}"),"size":19}}}),
                    )
                }
                _ => self.base.request(path, args, cache).await,
            }
        })
    }
}
