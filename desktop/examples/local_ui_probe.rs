//! Real WebView, FFprobe and libmpv proof using generated, caller-owned media.
//! No credentials or remote requests. Mixed playlist mode uses a sealed account.
#[path = "support/mixed_playlist_fixture.rs"]
mod mixed_playlist_fixture;
#[path = "../../core/tests/support/native_lyric_wave.rs"]
mod native_lyric_wave;
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
    let batch_hide = std::env::var("CUMULUS_LOCAL_BATCH_HIDE_PROOF").as_deref() == Ok("1");
    let batch_play = std::env::var("CUMULUS_LOCAL_BATCH_PLAY_PROOF").as_deref() == Ok("1");
    let batch_add = std::env::var("CUMULUS_LOCAL_BATCH_ADD_PROOF").as_deref() == Ok("1");
    let directory_selection =
        std::env::var("CUMULUS_LOCAL_DIRECTORY_SELECTION_PROOF").as_deref() == Ok("1");
    assert!(!batch_add || (!batch_play && !batch_hide && !group_hide && !folder_reveal));
    assert!(
        !(batch_hide || batch_play) || !(group_hide || folder_reveal || (batch_hide && batch_play))
    );
    let queue_reveal = std::env::var("CUMULUS_LOCAL_QUEUE_REVEAL_PROOF").as_deref() == Ok("1");
    assert!(
        !queue_reveal
            || (cfg!(target_os = "linux")
                && !folder_reveal
                && !group_hide
                && !batch_hide
                && !batch_play
                && !batch_add
                && std::env::var_os("CUMULUS_LOCAL_QUEUE_PROOF").is_none())
    );
    let queue = queue_reveal
        || folder_reveal
        || group_hide
        || batch_hide
        || batch_play
        || batch_add
        || std::env::var_os("CUMULUS_LOCAL_QUEUE_PROOF").is_some();
    assert!(
        !queue || (!cold && !reopen),
        "queue proof needs a fresh profile"
    );

    assert!(
        !cold || !reopen,
        "cold-start proof requires a fresh directory"
    );
    assert!(
        !directory_selection || (!queue && !cold && !reopen),
        "directory selection needs a fresh dedicated profile"
    );
    let mixed_queue = std::env::var_os("CUMULUS_MIXED_PLAYLIST_QUEUE_PROOF").is_some();
    let mixed_queue_script = mixed_queue.then(|| {
        let path = PathBuf::from(
            std::env::var_os("CUMULUS_MIXED_PLAYLIST_QUEUE_SCRIPT")
                .expect("explicit mixed queue proof script"),
        );
        assert!(path.is_absolute());
        let code = fs::read_to_string(path).expect("mixed queue proof script");
        assert!(code.len() < 64 * 1024);
        code
    });
    let mixed = mixed_queue || std::env::var_os("CUMULUS_MIXED_PLAYLIST_PROOF").is_some();
    assert!(!mixed || !(reopen || queue || cold || directory_selection));
    let mixed_fixture = mixed.then(|| {
        fs::create_dir(&root).expect("new synthetic mixed playlist root");
        let fixture = mixed_playlist_fixture::Fixture::new(&root).expect("mixed playlist audio");
        if mixed_queue {
            fixture
                .add_queue_anchor()
                .expect("separate local playback anchor");
        }
        Arc::new(fixture)
    });
    if !reopen && !mixed {
        fs::create_dir(&root).expect("new proof root");
        fixtures(&root, cold || directory_selection, queue).expect("generated local fixtures");
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
            if batch_hide || batch_play || batch_add {
                let mut wav = fs::read(media.join("雨声.wav")).expect("synthetic audio bytes");
                let size = 16_000u32;
                wav.truncate(44 + size as usize);
                wav[4..8].copy_from_slice(&(size + 36).to_le_bytes());
                wav[40..44].copy_from_slice(&size.to_le_bytes());
                for index in 0..105 {
                    // The two chosen rows need enough decoded clock for a real
                    // pause/resume proof, while the old hide fixture stays 1s.
                    let mut longer = Vec::new();
                    let bytes = if (batch_play || batch_add) && matches!(index, 104 | 4) {
                        let samples = fs::read(media.join("雨声.wav"))
                            .expect("synthetic batch playback samples");
                        let length = 16_000u32 * 12;
                        longer.extend_from_slice(&samples[..44 + length as usize]);
                        longer[4..8].copy_from_slice(&(length + 36).to_le_bytes());
                        longer[40..44].copy_from_slice(&length.to_le_bytes());
                        &longer
                    } else {
                        &wav
                    };
                    fs::write(
                        media.join(format!("qx-local-selection {index:03}.wav")),
                        bytes,
                    )
                    .expect("synthetic batch audio");
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
        let store=registry.store();
        let api: Arc<dyn Api>=match &mixed_fixture {
            Some(fixture)=>fixture.clone(),
            None=>offline.clone(),
        };
        let session=Arc::new(AccountSession::new(api,Some(store.clone())));
        let client=MusicClient::with_session(session.clone(),registry).with_audio_factory(||Ok(Box::new(MpvEngine::open(MpvConfig{null_output:true,..Default::default()})?)));
        tauri::async_runtime::block_on(client.start_local_library())?;
        if mixed {
            let path=setup.join("media").to_string_lossy().into_owned();
            tauri::async_runtime::block_on(client.local_command(cumulus_core::client::dto::LocalCommand::Watch{path}))?;
        }
        app.manage(client);
        if small {app.get_webview_window("main").unwrap().set_size(tauri::LogicalSize::new(760.0,540.0))?;}
        app.add_capability(r#"{"identifier":"local-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
        let output=setup.clone();let handle=app.handle().clone();app.listen("local-proof",move|event|{
            let mut value:Value=serde_json::from_str(event.payload()).expect("proof JSON");
            let stage=value["stage"].as_str().unwrap_or("").to_owned();
            let stage=stage.as_str();
            if let Some(fixture)=&mixed_fixture {
                let stages = if mixed_queue {
                    &["ux_01_paused_baseline","ux_02_local_next_menu","ux_03_local_next","ux_04_online_next",
                      "ux_05_entire_playlist","ux_06_drag_target","ux_07_drag_boundary","ux_08_drag_inserted",
                      "ux_09_local_queue_source","ux_10_followed_playlist_source"][..]
                } else {
                    &["ux_01_add_dialog","ux_02_add_cancelled","ux_03_mixed_read","ux_04_local_playing",
                      "ux_05_remove_dialog","ux_06_remove_cancelled","ux_07_removed"][..]
                };
                if !stages.contains(&stage) && !["complete","failed"].contains(&stage) {return;}
                value["nativeReceipt"]=fixture.receipt(&store).expect("mixed playlist receipt");
                let pending=output.join(format!("{stage}.pending"));
                fs::write(&pending,serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                fs::rename(pending,output.join(format!("{stage}.json"))).unwrap();
                if stage=="complete"||stage=="failed" {
                    handle.exit(if stage=="complete"{0}else{1});
                } else if std::env::var_os("CUMULUS_PROOF_CAPTURE_ACK").is_some() {
                    let marker=output.join(format!("{stage}.captured"));
                    let stage=stage.to_owned();let handle=handle.clone();
                    tauri::async_runtime::spawn(async move {
                        if tokio::time::timeout(std::time::Duration::from_secs(8),async {
                            while !marker.is_file() {tokio::time::sleep(std::time::Duration::from_millis(40)).await;}
                        }).await.is_ok() {
                            handle.get_webview_window("main").unwrap().eval(format!("window.__mixedCapture={};",json!(stage))).unwrap();
                        }
                    });
                }
                return;
            }
            if queue_reveal && stage=="await_queue_file_call" {
                let expected=value["count"].as_u64().filter(|count| (1..=2).contains(count)).unwrap() as usize;
                let calls=output.parent().unwrap().join("show-items.jsonl");
                let handle=handle.clone();
                tauri::async_runtime::spawn(async move {
                    if tokio::time::timeout(std::time::Duration::from_secs(3),async {
                        while fs::read_to_string(&calls).map_or(0, |log| log.lines().count()) != expected {
                            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                        }
                    }).await.is_ok() {
                        handle.get_webview_window("main").unwrap().eval(format!("window.__queueFileCalls={expected};")).unwrap();
                    }
                });
                return;
            }
            if queue_reveal && stage=="replace_queue_ncm" {
                fs::OpenOptions::new().append(true).open(output.join("media/归途.ncm")).unwrap()
                    .write_all(b"changed synthetic file").unwrap();
                handle.get_webview_window("main").unwrap().eval("window.__queueNcmChanged=true;").unwrap();
                return;
            }
            let folder_capture = folder_reveal && ["ux_01_folder_menu", "ux_02_folder_focus", "ux_03_folder_failure"].contains(&stage);
            let hide_capture = group_hide && ["ux_01_group_hide_menu", "ux_02_group_hidden", "ux_03_group_restored"].contains(&stage);
            let batch_capture = batch_hide && ["ux_01_batch_menu", "ux_02_batch_cross_page", "ux_03_batch_hidden", "ux_04_batch_restored"].contains(&stage);
            let batch_play_capture = batch_play && ["ux_01_batch_menu", "ux_02_batch_cross_page", "ux_03_batch_playing", "ux_04_batch_resumed", "ux_05_batch_play_back"].contains(&stage);
            let batch_add_capture = batch_add && ["ux_01_batch_menu", "ux_02_batch_cross_page", "ux_03_batch_added", "ux_04_batch_empty_started", "ux_05_batch_active_duplicate", "ux_06_batch_add_back"].contains(&stage);
            let directory_capture = directory_selection && [
                "ux_01_picker_requested", "ux_02_unchecked_after_cancel", "ux_03_checked",
                "ux_04_paused_root", "ux_05_deselected", "ux_06_rechecked",
            ].contains(&stage);
            let queue_reveal_capture = queue_reveal && ["ux_01_queue_local_cue", "ux_02_queue_local_ncm",
                "ux_03_queue_local_changed"].contains(&stage);
            if folder_capture || hide_capture || batch_capture || batch_play_capture || batch_add_capture || directory_capture || queue_reveal_capture {
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
            if directory_selection {
                if stage=="complete"||stage=="failed" {
                    fs::write(output.join(format!("{stage}.json")),serde_json::to_vec_pretty(&value).unwrap()).unwrap();
                    handle.exit(if stage=="complete"{0}else{1});
                }
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
            webview.eval(format!("window.__localProof={};",json!({"root":root,"reopen":reopen,"capture":capture,"batchPlay":batch_play,"batchAdd":batch_add}))).unwrap();
            webview.eval(if let Some(script)=&mixed_queue_script {script.as_str()} else if mixed {include_str!("mixed_playlist_ui_probe.js")} else if cold {include_str!("local_cold_start_probe.js")} else if directory_selection {
                include_str!("local_directory_selection_ui_probe.js")
            } else if queue_reveal {
                include_str!("local_queue_file_reveal_ui_probe.js")
            } else if folder_reveal {
                include_str!("local_folder_reveal_ui_probe.js")
            } else if group_hide {
                include_str!("local_group_hide_ui_probe.js")
            } else if batch_hide || batch_play || batch_add {
                include_str!("local_batch_hide_ui_probe.js")
            } else if queue {
                include_str!("local_queue_ui_probe.js")
            } else {include_str!("local_ui_probe.js")}).unwrap();
        }
    }).run(tauri::generate_context!()).expect("local UI proof");
}
