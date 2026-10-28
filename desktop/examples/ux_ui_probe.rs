//! Production UI/IPC inspection with isolated synthetic account and preferences.
#[path = "support/assets.rs"]
mod assets;
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

struct Library;
fn song(id: u64) -> Value {
    json!({"id":id,"name":format!("合成歌曲 {id} · A quiet afternoon"),"ar":[{"name":"Cumulus"}],"al":{"name":"界面体验验证"},"dt":if id==1 {6_000_000} else {235_000},"fee":id%3})
}
fn playlist(id: u64) -> Value {
    json!({"id":id,"userId":if id<20 {2} else {3},"name":if id==1 {"我喜欢的音乐".into()} else if id==2 {"午后散步 · 一份用来检查较长名称与编辑入口的歌单".into()} else {format!("合成歌单 {id}")},"creator":{"userId":if id<20 {2} else {3},"nickname":"Cumulus"},"specialType":if id==1 {5} else {0},"privacy":10,"trackCount":40,"description":"沿着熟悉的小路，留一点时间给音乐。这是一段较长的合成简介，用来检查窄窗口里的文字、封面和操作入口。","tags":["轻音乐"],"trackIds":(1..=40).map(|id|json!({"id":id,"v":0})).collect::<Vec<_>>()})
}
impl Api for Library {
    fn request<'a>(&'a self, path: &'a str, args: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            Ok(match path {
                "/api/rtrs/abt/front/expinfo/list" => json!({"code":200,"data":[]}),
                "/api/relay/setting/show" => json!({"code":200,"data":{"groups":[]}}),
                "/api/w/nuser/account/get" => {
                    json!({"code":200,"profile":{"userId":2,"nickname":"界面体验验证"}})
                }
                "/api/user/playlist" => {
                    json!({"code":200,"more":false,"playlist":(1..=24).map(playlist).collect::<Vec<_>>()})
                }
                "/api/song/like/get" => json!({"code":200,"ids":[3,7,10]}),
                "/api/v3/discovery/recommend/songs" => {
                    json!({"code":200,"data":{"dailySongs":(1..=40).map(song).collect::<Vec<_>>()}})
                }
                "/api/v6/playlist/detail" => {
                    json!({"code":200,"playlist":playlist(args["id"].as_str().and_then(|s|s.parse().ok()).or_else(||args["id"].as_u64()).unwrap_or(2))})
                }
                "/api/v3/song/detail" => {
                    let rows: Vec<Value> =
                        serde_json::from_str(args["c"].as_str().unwrap()).unwrap();
                    json!({"code":200,"songs":rows.iter().map(|row|song(row["id"].as_str().and_then(|s|s.parse().ok()).or_else(||row["id"].as_u64()).unwrap())).collect::<Vec<_>>()})
                }
                "/api/playlist/catalogue" => {
                    json!({"code":200,"categories":{"0":"风格"},"sub":[{"name":"轻音乐","category":0},{"name":"摇滚","category":0}]})
                }
                "/api/cloudsearch/pc" => {
                    json!({"code":200,"result":{"songs":(1..=12).map(song).collect::<Vec<_>>(),"songCount":12}})
                }
                _ => {
                    return Err(cumulus_core::Error::Response(
                        "unsupported UX fixture endpoint",
                    ));
                }
            })
        })
    }
}
fn main() {
    // SAFETY: first operation before GTK/runtime or worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new proof root"));
    let output = root.join(if std::env::var_os("CUMULUS_PROOF_REOPEN").is_some() {
        "reopen"
    } else {
        "first"
    });
    fs::create_dir_all(&root).unwrap();
    fs::create_dir(&output).expect("new evidence directory");
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default()).setup(move |app| {
        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        let session=Arc::new(AccountSession::new(Arc::new(Library),Some(registry.store())));
        let client=MusicClient::with_session(session,registry);
        app.manage(cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true));
        app.add_capability(r#"{"identifier":"ux-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("ux-proof",move |event| {
            let value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("");
            if stage=="resize" {
                handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(value["width"].as_f64().unwrap(),value["height"].as_f64().unwrap())).unwrap();return;
            }
            if !(["complete","failed"].contains(&stage) || stage.starts_with("ux_") && stage.chars().all(|c|c.is_ascii_alphanumeric()||c=='_')){return;}
            fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if ["complete","failed"].contains(&stage) {handle.exit(if stage=="complete" {0} else {1});}
        });
        // Absolute config dataDirectory values are ignored by Tauri's safe-path
        // resolver. The builder API owns this isolated test directory explicitly.
        tauri::WebviewWindowBuilder::from_config(app, &window_config)?
            .data_directory(root.join("webview"))
            .build()?;
        Ok(())
    }).on_page_load(|view, payload| {
        if payload.event()==PageLoadEvent::Finished && view.label()=="main" {
            let script=std::env::var_os("CUMULUS_UX_SCRIPT").map(|p|fs::read_to_string(p).unwrap());
            view.eval(script.as_deref().unwrap_or(include_str!("ux_ui_probe.js"))).unwrap();
        }
    }).run(context).expect("UX probe");
}
