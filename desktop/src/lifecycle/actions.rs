use super::*;
use cumulus_core::{
    client::dto::WindowPreferencesCommand,
    preferences::window::{ClosePlan, WindowSettings},
};

pub(super) fn request<R: tauri::Runtime>(app: &tauri::AppHandle<R>, exit: bool) {
    let Some(host) = app.try_state::<Host>() else {
        return;
    };
    let Ok(mut state) = host.inner.lock() else {
        reveal(app);
        return;
    };
    if state.view.busy {
        return;
    }
    if !exit && state.view.prompt.is_some() {
        drop(state);
        reveal(app);
        return;
    }
    let id = state.next();
    let available = state.available();
    state.view.busy = true;
    state.view.message = None;
    drop(state);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if exit {
            finish_exit(&app, id).await;
            return;
        }
        let result = (|| {
            let client = app.state::<MusicClient>();
            let epoch = client.account()?.epoch;
            let preferences = client.window_preferences(&epoch)?.value;
            Ok::<_, cumulus_core::Error>(ClosePrompt {
                id: id.to_string(),
                epoch,
                preferences,
            })
        })();
        match result {
            Ok(prompt) => match prompt.preferences.settings.plan(available) {
                ClosePlan::Hide if !prompt.preferences.damaged => hide(&app, id).await,
                ClosePlan::Exit if !prompt.preferences.damaged => finish_exit(&app, id).await,
                _ => {
                    change(&app, |state| {
                        if state.operation == id {
                            state.view.busy = false;
                            state.view.prompt = Some(prompt);
                        }
                    });
                    reveal(&app);
                }
            },
            Err(_) => {
                // Read failure must leave an explicit exit route. Never save
                // a guessed revision or silently replace damaged preferences.
                change(&app, |state| {
                    if state.operation == id {
                        state.view.busy = false;
                        state.view.message = Some("无法读取关闭设置；本次选择不会被保存。".into());
                        state.view.prompt = Some(ClosePrompt {
                            id: id.to_string(),
                            epoch: String::new(),
                            preferences: WindowPreferencesView {
                                revision: None,
                                settings: Default::default(),
                                damaged: true,
                            },
                        });
                    }
                });
                reveal(&app);
            }
        }
    });
}
pub(super) async fn command<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    command: CloseCommand,
) -> Result<(), String> {
    let host = app.try_state::<Host>().ok_or("窗口关闭服务尚未启动")?;
    let (prompt, behavior, remember, operation) = {
        let mut state = host.inner.lock().map_err(|_| "窗口状态不可用")?;
        let id = match &command {
            CloseCommand::Cancel { id } | CloseCommand::Confirm { id, .. } => id,
        };
        let prompt = state
            .view
            .prompt
            .as_ref()
            .filter(|prompt| &prompt.id == id)
            .cloned()
            .ok_or("关闭提示已更新，请重新选择")?;
        if state.view.busy {
            return Err("正在处理关闭请求".into());
        }
        match command {
            CloseCommand::Cancel { .. } => {
                state.next();
                state.view.prompt = None;
                state.view.message = None;
                drop(state);
                change(app, |_| {});
                return Ok(());
            }
            CloseCommand::Confirm {
                behavior, remember, ..
            } => {
                if behavior == CloseBehavior::Tray && !state.available() {
                    return Err("系统托盘暂不可用，请保留窗口或选择退出。".into());
                }
                state.view.busy = true;
                (prompt, behavior, remember, state.next())
            }
        }
    };
    change(app, |_| {});
    if remember {
        let result = prompt
            .preferences
            .revision
            .clone()
            .ok_or("关闭设置需要先恢复，当前无法保存".to_owned())
            .and_then(|revision| {
                app.state::<MusicClient>()
                    .window_preferences_command(
                        &prompt.epoch,
                        WindowPreferencesCommand::Change {
                            expected_revision: revision,
                            settings: WindowSettings {
                                close_behavior: behavior,
                                remember_choice: true,
                            },
                        },
                    )
                    .map(|reply| reply.value)
                    .map_err(|e| {
                        cumulus_core::client::ClientError::from(e)
                            .message
                            .to_owned()
                    })
            });
        match result {
            Ok(preferences) => change(app, |state| {
                if state.operation == operation
                    && let Some(prompt) = &mut state.view.prompt
                {
                    // A successful save may be followed by a failed native
                    // hide or room cleanup. An explicit retry must use the
                    // committed revision, not the prompt's original CAS.
                    prompt.preferences = preferences;
                }
            }),
            Err(error) => {
                fail(
                    app,
                    operation,
                    "设置未保存，请取消后重新打开关闭提示。".into(),
                );
                return Err(error);
            }
        }
    }
    match behavior {
        CloseBehavior::Tray => hide(app, operation).await,
        CloseBehavior::Exit => finish_exit(app, operation).await,
    };
    Ok(())
}
fn fail<R: tauri::Runtime>(app: &tauri::AppHandle<R>, operation: u64, message: String) {
    change(app, |state| {
        if state.operation == operation {
            state.view.busy = false;
            state.exiting = false;
            state.view.hidden = false;
            state.view.message = Some(message);
        }
    });
    reveal(app);
}
async fn hide<R: tauri::Runtime>(app: &tauri::AppHandle<R>, operation: u64) {
    let host = app.state::<Host>();
    let eligible = host
        .inner
        .lock()
        .map(|mut state| {
            let ok = state.operation == operation && state.available();
            if ok {
                state.view.hidden = true;
            }
            ok
        })
        .unwrap_or(false);
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if !eligible || window.hide().is_err() {
        fail(app, operation, "无法隐藏到托盘，主窗口已保留。".into());
        return;
    }
    // Native calls acknowledge submission, not the window manager's state.
    // Poll at most 500ms and recheck the host/operation before accepting hide.
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let current = host
            .inner
            .lock()
            .map(|state| state.operation == operation && state.view.hidden && state.available())
            .unwrap_or(false);
        if !current {
            fail(app, operation, "系统托盘状态已改变，主窗口已恢复。".into());
            return;
        }
        if window.is_visible().ok() == Some(false) {
            change(app, |state| {
                if state.operation == operation {
                    state.view.busy = false;
                    state.view.prompt = None;
                    state.view.message = None;
                }
            });
            return;
        }
    }
    fail(app, operation, "窗口隐藏未得到确认，主窗口已恢复。".into());
}
async fn finish_exit<R: tauri::Runtime>(app: &tauri::AppHandle<R>, operation: u64) {
    let host = app.state::<Host>();
    let proceed = host
        .inner
        .lock()
        .map(|mut state| {
            if state.operation != operation {
                return false;
            }
            state.exiting = true;
            true
        })
        .unwrap_or(false);
    if !proceed {
        return;
    }
    change(app, |_| {});
    // Same room cleanup used by the previous close handler; a failed leave
    // retains a reachable window. Merely hiding never comes through here.
    match app.state::<MusicClient>().close_together().await {
        Ok(()) => {
            if let Some(window) = app.get_webview_window("main")
                && window.destroy().is_err()
            {
                fail(app, operation, "关闭窗口失败，请重试。".into());
            }
        }
        Err(error) => {
            let public = cumulus_core::client::ClientError::from(error);
            fail(
                app,
                operation,
                format!(
                    "退出房间尚未完成：{}。窗口已保留，请核对后再关闭。",
                    public.message
                ),
            );
        }
    }
}
