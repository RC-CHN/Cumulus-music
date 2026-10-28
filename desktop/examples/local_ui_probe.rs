//! Real WebView, FFprobe and libmpv proof using generated, caller-owned media.
//! No credentials or remote requests. The API adapter only represents signed out.
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    playback::engine::{MpvConfig, MpvEngine},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::{Listener, Manager, webview::PageLoadEvent};
#[cfg(target_os = "linux")]
fn verify_locale(root: &Path, reopened: bool) {
    let read = |category| {
        // SAFETY: read-only startup inspection after GTK initialization; copy
        // each libc-owned string immediately, without changing any category.
        unsafe {
            let pointer = libc::setlocale(category, std::ptr::null());
            assert!(!pointer.is_null());
            std::ffi::CStr::from_ptr(pointer)
                .to_string_lossy()
                .into_owned()
        }
    };
    let numeric = read(libc::LC_NUMERIC);
    let messages = read(libc::LC_MESSAGES);
    assert_eq!(numeric, "C");
    assert!(!matches!(messages.as_str(), "C" | "POSIX"));
    if let Ok(expected) = std::env::var("LC_ALL")
        && !expected.is_empty()
    {
        assert_eq!(
            messages, expected,
            "GUI message locale must remain unchanged"
        );
    }
    let name = if reopened {
        "native-startup-reopened.json"
    } else {
        "native-startup.json"
    };
    fs::write(
        root.join(name),
        serde_json::to_vec_pretty(&json!({
            "numericLocale":numeric,"messageLocale":messages,"afterGuiInitialization":true
        }))
        .unwrap(),
    )
    .unwrap();
}
struct Offline;
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/w/nuser/account/get");
            Ok(json!({"code":301}))
        })
    }
}
fn fixtures(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let media = root.join("media");
    fs::create_dir(&media)?;
    let samples = 8000u32 * 120;
    let size = samples * 2;
    let mut file = fs::File::create_new(media.join("album.wav"))?;
    file.write_all(b"RIFF")?;
    file.write_all(&(size + 36).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&8000u32.to_le_bytes())?;
    file.write_all(&16000u32.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&size.to_le_bytes())?;
    file.write_all(&vec![0; size as usize])?;
    drop(file);
    fs::copy(media.join("album.wav"), media.join("雨声.wav"))?;
    fs::write(
        media.join("album.cue"),
        "TITLE \"夜行集\"\nPERFORMER \"Cumulus\"\nFILE \"album.wav\" WAVE\nTRACK 01 AUDIO\nTITLE \"夜行 · 上篇\"\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nTITLE \"夜行 · 下篇\"\nINDEX 01 00:30:00\n",
    )?;
    fs::write(
        media.join("album.lrc"),
        "[00:00]夜色渐深\n[00:35]沿着灯火向前\n[00:50]音乐陪你到天明\n",
    )?;
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(media.join("album.wav"))
        .args(["-t", "60", "-c:a", "flac"])
        .arg(root.join("source.flac"))
        .status()?;
    if !status.success() {
        return Err("generate FLAC failed".into());
    }
    cumulus_core::media::ncm::write_file(
        root.join("source.flac"),
        media.join("归途.ncm"),
        cumulus_core::media::ncm::NcmWriteOptions {
            metadata: Some(
                &json!({"musicName":"归途","album":"离线练习","artist":[["Cumulus",0]],"duration":60000,"format":"flac"}),
            ),
            ..Default::default()
        },
    )?;
    Ok(())
}
fn main() {
    let root = PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("explicit proof root"));
    let reopen = std::env::var_os("CUMULUS_PROOF_REOPEN").is_some();
    let small = std::env::var_os("CUMULUS_PROOF_SMALL").is_some();
    if !reopen {
        fs::create_dir(&root).expect("new proof root");
        fixtures(&root).expect("generated local fixtures");
    }
    let setup = root.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        #[cfg(target_os="linux")]
        verify_locale(&setup,reopen);
        let registry=ProfileRegistry::open(&setup.join("state"),ApiOrigin::Pc)?;
        let session=Arc::new(AccountSession::new(Arc::new(Offline),Some(registry.store())));
        let client=MusicClient::with_session(session.clone(),registry).with_audio_factory(||Ok(Box::new(MpvEngine::open(MpvConfig{null_output:true,..Default::default()})?)));
        tauri::async_runtime::block_on(client.start_local_library())?;
        app.manage(client);
        if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"local-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let output=setup.clone();let handle=app.handle().clone();app.listen("local-proof",move|event|{
            let value:Value=serde_json::from_str(event.payload()).expect("proof JSON");let stage=value["stage"].as_str().unwrap_or("");
            if stage=="history_fault" {
                let enabled=value["enabled"].as_bool().expect("fault switch");
                let connection=rusqlite::Connection::open(output.join("state/core.sqlite3")).unwrap();
                connection.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
                connection.execute_batch(if enabled {
                    "CREATE TRIGGER history_fixture_fault BEFORE INSERT ON state WHEN NEW.kind='play-history-v1' AND NEW.key='local:song' BEGIN SELECT RAISE(ABORT,'synthetic history failure'); END;"
                } else { "DROP TRIGGER history_fixture_fault;" }).unwrap();
                handle.get_webview_window("main").unwrap().eval(format!("window.__historyFaultReady={enabled};")).unwrap();
                return;
            }
            if !["local_picker","local_library","local_cue","local_ncm","local_groups","local_reopened","local_history_warning","complete","failed"].contains(&stage){return;}
            let suffix=if reopen{"-reopened"}else{""};
            if stage=="complete" {
                let history=cumulus_core::history::HistoryService::new(session.clone()).unwrap();
                let token=session.token().unwrap();
                let songs=history.snapshot_for(&token,cumulus_core::history::HistoryKind::Song).unwrap();
                assert_eq!(songs.local.len(),2,"only successfully played CUE/NCM entries enter history");
                assert!(songs.local.iter().all(|r|r.id.starts_with("local:")));
                assert!(songs.remote.is_none());
                let persisted=serde_json::to_value(&songs.local).unwrap();
                let prior=output.join("history-local.json");
                if reopen { assert_eq!(serde_json::from_slice::<Value>(&fs::read(prior).unwrap()).unwrap(),persisted,"paused restore must not rewrite listening history"); }
                fs::write(output.join(format!("history-local{suffix}.json")),serde_json::to_vec_pretty(&persisted).unwrap()).unwrap();
            }
            fs::write(output.join(format!("{stage}{suffix}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
            if stage=="complete"||stage=="failed"{handle.exit(if stage=="complete"{0}else{1});}
        });Ok(())
    }).on_page_load(move|webview,payload|{
        if payload.event()==PageLoadEvent::Finished {
            webview.eval(format!("window.__localProof={};",json!({"root":root,"reopen":reopen}))).unwrap();
            webview.eval(include_str!("local_ui_probe.js")).unwrap();
        }
    }).run(tauri::generate_context!()).expect("local UI proof");
}
