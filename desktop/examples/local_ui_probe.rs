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
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
struct Offline {
    hold: bool,
    started: AtomicBool,
    release: tokio::sync::Notify,
}
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/w/nuser/account/get");
            if self.hold {
                self.started.store(true, Ordering::Release);
                self.release.notified().await;
                return Err(cumulus_core::Error::Transport(
                    "synthetic account restore timeout".into(),
                ));
            }
            Ok(json!({"code":301}))
        })
    }
}
fn fixtures(root: &Path, cue_only: bool, queue: bool) -> Result<(), Box<dyn std::error::Error>> {
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
    let (cue_album, cue_artist) = if queue {
        ("北京", "北京")
    } else {
        ("夜行集", "Cumulus")
    };
    fs::write(
        media.join("album.cue"),
        format!(
            "TITLE \"{cue_album}\"\nPERFORMER \"{cue_artist}\"\nFILE \"album.wav\" WAVE\nTRACK 01 AUDIO\nTITLE \"夜行 · 上篇\"\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nTITLE \"夜行 · 下篇\"\nINDEX 01 00:30:00\n"
        ),
    )?;
    if cue_only {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    let ffmpeg = {
        // Packaging redirects libmpv to its staged runtime, which does not
        // carry the CLI used to generate this synthetic NCM source.
        std::env::var_os("CUMULUS_LOCAL_FIXTURE_FFMPEG")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.is_file())
            .ok_or("local fixture generation needs the verified absolute FFmpeg executable")?
    };
    #[cfg(not(target_os = "windows"))]
    let ffmpeg = "ffmpeg";
    let status = std::process::Command::new(ffmpeg)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(media.join("album.wav"))
        .args(["-t", "60", "-c:a", "flac"])
        .arg(root.join("source.flac"))
        .status()?;
    if !status.success() {
        return Err("generate FLAC failed".into());
    }
    let (ncm_album, ncm_artist) = if queue {
        ("上海", "上海")
    } else {
        ("离线练习", "Cumulus")
    };
    cumulus_core::media::ncm::write_file(
        root.join("source.flac"),
        media.join("归途.ncm"),
        cumulus_core::media::ncm::NcmWriteOptions {
            metadata: Some(
                &json!({"musicName":"归途","album":ncm_album,"artist":[[ncm_artist,0]],"duration":60000,"format":"flac"}),
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
    let cold = std::env::var_os("CUMULUS_LOCAL_COLD_START_PROOF").is_some();
    let folder_reveal = std::env::var("CUMULUS_LOCAL_FOLDER_REVEAL_PROOF").as_deref() == Ok("1");
    assert!(!folder_reveal || cfg!(target_os = "linux"));
    let group_hide = std::env::var("CUMULUS_LOCAL_GROUP_HIDE_PROOF").as_deref() == Ok("1");
    assert!(!group_hide || !folder_reveal);
    let queue =
        folder_reveal || group_hide || std::env::var_os("CUMULUS_LOCAL_QUEUE_PROOF").is_some();
    assert!(
        !queue || (!cold && !reopen),
        "queue proof needs a fresh profile"
    );

    assert!(
        !cold || !reopen,
        "cold-start proof requires a fresh directory"
    );
    if !reopen {
        fs::create_dir(&root).expect("new proof root");
        fixtures(&root, cold, queue).expect("generated local fixtures");
        if queue {
            let media = root.join("media");
            fs::create_dir(media.join("Bonus")).expect("synthetic nested directory");
            fs::copy(
                media.join("雨声.wav"),
                media.join("Bonus").join("Bonus Track.wav"),
            )
            .expect("synthetic nested audio file");
            if group_hide {
                for (folder, file) in [
                    ("Bonus/Deep", "Nested audible.wav"),
                    ("Bonus plus", "Neighbor audible.wav"),
                ] {
                    let directory = media.join(folder);
                    fs::create_dir(&directory).expect("synthetic group directory");
                    fs::copy(media.join("雨声.wav"), directory.join(file))
                        .expect("synthetic group media");
                }
            }
        }
    }
    let setup = root.clone();
    cumulus_desktop::configure(tauri::Builder::default()).setup(move|app|{
        #[cfg(target_os="linux")]
        verify_locale(&setup,reopen);
        let registry=ProfileRegistry::open(&setup.join("state"),ApiOrigin::Pc)?;
        let offline=Arc::new(Offline { hold:cold, started:AtomicBool::new(false), release:tokio::sync::Notify::new() });
        let session=Arc::new(AccountSession::new(offline.clone(),Some(registry.store())));
        let client=MusicClient::with_session(session.clone(),registry).with_audio_factory(||Ok(Box::new(MpvEngine::open(MpvConfig{null_output:true,..Default::default()})?)));
        tauri::async_runtime::block_on(client.start_local_library())?;
        app.manage(client);
        if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"local-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let output=setup.clone();let handle=app.handle().clone();app.listen("local-proof",move|event|{
            let value:Value=serde_json::from_str(event.payload()).expect("proof JSON");let stage=value["stage"].as_str().unwrap_or("");
            let folder_capture = folder_reveal && ["ux_01_folder_menu", "ux_02_folder_focus", "ux_03_folder_failure"].contains(&stage);
            let hide_capture = group_hide && ["ux_01_group_hide_menu", "ux_02_group_hidden", "ux_03_group_restored"].contains(&stage);
            if folder_capture || hide_capture {
                fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                let marker=output.join(format!("{stage}.captured"));let stage=stage.to_owned();let handle=handle.clone();
                tauri::async_runtime::spawn(async move {
                    if tokio::time::timeout(std::time::Duration::from_secs(8),async {
                        while !marker.is_file() {tokio::time::sleep(std::time::Duration::from_millis(40)).await;}
                    }).await.is_ok() {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__folderCaptureAck={};",json!(stage))).unwrap();
                    }
                });
                return;
            }
            if queue {
                if ["local_queue_started", "ux_01_local_track_menu", "ux_02_local_track_playing", "ux_03_local_queue_drop", "complete", "failed"].contains(&stage) {
                    fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    if stage=="complete"||stage=="failed"{handle.exit(if stage=="complete"{0}else{1});}
                }
                return;
            }
            if cold {
                if stage=="inspect_restore" {
                    let started=offline.started.load(Ordering::Acquire);
                    handle.get_webview_window("main").unwrap().eval(format!("window.__coldRestoreStarted={started};")).unwrap();
                } else if stage=="release_restore" {
                    offline.release.notify_one();
                } else if ["local_cold_playing","complete","failed"].contains(&stage) {
                    fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    if stage=="complete"||stage=="failed"{handle.exit(if stage=="complete"{0}else{1});}
                }
                return;
            }
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
            if !["local_picker","local_library","local_cue","local_ncm","local_groups","local_reopened","local_history_warning","local_history_page","local_history_replay","complete","failed"].contains(&stage){return;}
            let suffix=if reopen{"-reopened"}else{""};
            if stage=="local_library" && !reopen {
                // The real GTK folder picker must select the audio folder,
                // without mistaking its only child for the chosen directory.
                // Sidecars are discovered at playback, after the scan.
                let lyrics=output.join("media/lyric/lrc");
                fs::create_dir_all(&lyrics).unwrap();
                fs::write(lyrics.join("album.lrc"),
                    "[00:00]夜色渐深\n[00:35]沿着灯火向前\n[00:50]音乐陪你到天明\n").unwrap();
                assert!(!output.join("media/album.lrc").exists());
            }
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
            let capture=std::env::var_os("CUMULUS_PROOF_CAPTURE_ACK").is_some();
            webview.eval(format!("window.__localProof={};",json!({"root":root,"reopen":reopen,"capture":capture}))).unwrap();
            webview.eval(if cold {include_str!("local_cold_start_probe.js")} else if folder_reveal {
                include_str!("local_folder_reveal_ui_probe.js")
            } else if group_hide {
                include_str!("local_group_hide_ui_probe.js")
            } else if queue {
                include_str!("local_queue_ui_probe.js")
            } else {include_str!("local_ui_probe.js")}).unwrap();
        }
    }).run(tauri::generate_context!()).expect("local UI proof");
}
