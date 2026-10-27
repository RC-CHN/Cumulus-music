//! Production Vue/IPC/Rust comment workflows with synthetic HTTP, no real accounts.
#[path = "support/comment_edit.rs"]
mod fixture;
use cumulus_core::{
    client::MusicClient,
    comments::{CommentResource, ResourceKind},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit proof root"));
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    if !reopen {
        fs::create_dir(&root).expect("new proof root");
    }
    let output = root.join(if reopen { "reopen" } else { "first" });
    fs::create_dir(&output).expect("new run");
    let service = Arc::new(fixture::Service::open(root.join("fixture.json"), reopen).unwrap());
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app| {
        let client=MusicClient::open_with_http_backend(&root.join("state"),None,service.clone())?;
        app.manage(client);
        if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"comment-edit-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("comment-edit-proof",move|event| {
            let mut value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("").to_owned();
            if !stage.starts_with("edit_") && !["complete","failed"].contains(&stage.as_str()) {return;}
            let client=handle.state::<MusicClient>();let account=client.account().unwrap();
            let rows=client.comments_write_operations(&account.epoch,CommentResource{kind:ResourceKind::Track,id:"123".into(),owner:None}).unwrap();
            value["operations"]=serde_json::to_value(rows).unwrap();
            value["fixture"]=serde_json::to_value(&*service.state.lock().unwrap()).unwrap();
            value["realAccountWrites"]=json!(0);
            fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if ["complete","failed"].contains(&stage.as_str()) {handle.exit(if stage=="complete"{0}else{1});}
            else {handle.get_webview_window("main").unwrap().eval(format!("window.__editAck={};",json!(stage))).unwrap();}
        });Ok(())
    }).on_page_load(move|webview,payload| {
        if payload.event()==PageLoadEvent::Finished {
            webview.eval(format!("window.__editProof={};",json!({"reopen":reopen}))).unwrap();
            webview.eval(include_str!("comment_edit_ui_probe.js")).unwrap();
        }
    }).run(tauri::generate_context!()).expect("comment edit UI proof");
}
