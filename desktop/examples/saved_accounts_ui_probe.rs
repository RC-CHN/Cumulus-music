//! Production account chooser, local cleanup fault, explicit retry and restart.
#[path = "support/assets.rs"]
mod assets;
#[path = "support/auth.rs"]
mod fixture;
use cumulus_core::{
    account::profiles::ProfileRegistry,
    client::{
        MusicClient,
        dto::{LoginCommand, LoginMode},
    },
    protocol::http::ApiOrigin,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

fn main() {
    // SAFETY: before GTK/runtime or worker creation.
    unsafe { cumulus_desktop::prepare_graphics() };
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit fixture root"));
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    if !reopen {
        fs::create_dir(&root).expect("new root");
    }
    let output = root.join(if reopen { "reopen" } else { "first" });
    fs::create_dir(&output).expect("new evidence directory");
    let mut context = assets::context();
    let window_config = context.config().app.windows[0].clone();
    context.config_mut().app.windows[0].create = false;
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        let service=Arc::new(fixture::Service::default());
        let client=MusicClient::open_with_http_backend(&root.join("state"),None,service.clone())?;
        if !reopen {
            tauri::async_runtime::block_on(async {
                client.restore_account().await?;
                for address in ["alice@example.test","verify@example.test"] {
                    let attempt=client.begin_login(&client.account()?.epoch,LoginMode::Email,None).await?;
                    client.login_command(&attempt.operation_id,LoginCommand::Email {address:address.into(),password:"fixture-password".into(),remember:true}).await?;
                    if address.starts_with("verify") {
                        client.login_command(&attempt.operation_id,LoginCommand::VerificationMethods).await?;
                        client.login_command(&attempt.operation_id,LoginCommand::SecurityMailVerify {code:"123456".into()}).await?;
                    }
                }
                Ok::<_,cumulus_core::Error>(())
            })?;
        }
        let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
        let id=if reopen {
            fs::read_to_string(root.join("fixture-target.txt"))?
        } else {
            let id=registry.saved()?.into_iter().find(|p|p.uid=="2").unwrap().session_id;
            fs::write(root.join("fixture-target.txt"),&id)?;
            id
        };
        assert!(id.len()==32&&id.bytes().all(|c|c.is_ascii_hexdigit()));
        let credential_path=root.join("state/sessions").join(&id).join("core.sqlite3");
        if !reopen {
            rusqlite::Connection::open(&credential_path)?.execute_batch("CREATE TRIGGER deny_cleanup BEFORE DELETE ON state WHEN OLD.kind='credentials' BEGIN SELECT RAISE(FAIL,'fixture cleanup failure'); END;")?;
        }
        let client=cumulus_desktop::configure_native_audio(client,&app.path().resource_dir()?,true);
        app.manage(client);
        let window=tauri::WebviewWindowBuilder::from_config(app,&window_config)?
            .data_directory(root.join("webview")).build()?;
        if reopen||small {window.set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"saved-account-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let handle=app.handle().clone();
        app.listen("saved-account-proof",move|event|{
            let mut value:Value=serde_json::from_str(event.payload()).unwrap();
            let stage=value["stage"].as_str().unwrap_or("").to_owned();
            if stage=="resize" {
                handle.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0)).unwrap();
                return;
            }
            if !stage.starts_with("saved_") && !["complete","failed"].contains(&stage.as_str()){return;}
            let client=handle.state::<MusicClient>();
            let account=client.account().unwrap();
            assert_eq!(account.nickname.as_deref(),Some("二次验证账号"));
            let saved=client.saved_accounts(&account.epoch).unwrap();
            if stage=="saved_pending" {
                assert!(registry.removal_pending(&id).unwrap());
                assert!(saved.value.iter().find(|s|s.id==id).unwrap().removal_pending);
                rusqlite::Connection::open(&credential_path).unwrap().execute_batch("DROP TRIGGER deny_cleanup;").unwrap();
            }
            if stage=="saved_done" || stage=="complete" {
                assert_eq!(saved.value.len(),1);
                assert!(saved.value[0].current);
                assert!(!registry.removal_pending(&id).unwrap());
                let count:i64=rusqlite::Connection::open(&credential_path).unwrap().query_row("SELECT count(*) FROM state WHERE kind='credentials' AND key='session'",[],|row|row.get(0)).unwrap();
                assert_eq!(count,0);
                value["credentialRecordRemoved"]=json!(true);
            }
            value["nativeAccount"]=serde_json::to_value(account).unwrap();
            value["savedAccounts"]=serde_json::to_value(saved).unwrap();
            value["networkRequests"]=json!(0);
            value["fixtureRequests"]=json!(*service.paths.lock().unwrap());
            fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if ["complete","failed"].contains(&stage.as_str()){handle.exit(if stage=="complete"{0}else{1});}
            else {handle.get_webview_window("main").unwrap().eval(format!("window.__savedAck={};",json!(stage))).unwrap();}
        });
        Ok(())
    }).on_page_load(move|webview,payload|{
        if payload.event()==PageLoadEvent::Finished {
            webview.eval(format!("window.__savedReopen={reopen};")).unwrap();
            webview.eval(include_str!("saved_accounts_ui_probe.js")).unwrap();
        }
    }).run(context).expect("saved account proof");
}
