use super::{
    cache,
    policy::{self, Attempt, Mode, Outcome},
    probe,
};
use serde::Serialize;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize)]
struct Report {
    schema: u32,
    selected: Option<Mode>,
    cached: Option<Mode>,
    attempts: Vec<Attempt>,
    real_music_requests: u32,
}

/// Only called from the desktop binary's initial thread, before GUI or workers.
/// An explicit diagnostic invocation never initializes account/business state.
pub fn bootstrap() -> bool {
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    if command.as_deref() == Some(std::ffi::OsStr::new(probe::ARG)) {
        let code = args
            .next()
            .filter(|_| args.next().is_none())
            .map(|root| probe::run(Path::new(&root)))
            .unwrap_or(2);
        std::process::exit(code);
    }
    let diagnostic =
        command.as_deref() == Some(std::ffi::OsStr::new("--cumulus-renderer-diagnostics"));
    let result = select();
    if diagnostic {
        match result {
            Ok(report) => {
                println!("{}", serde_json::to_string(&report).unwrap());
                std::process::exit(if report.selected.is_some() { 0 } else { 1 });
            }
            Err(error) => {
                eprintln!("renderer check failed: {error}");
                std::process::exit(1);
            }
        }
    }
    match result {
        Ok(report) => {
            if let Some(mode) = report.selected {
                // SAFETY: single-threaded launcher; every probe process has exited.
                unsafe { std::env::set_var("CUMULUS_RENDERER_EFFECTIVE", mode.name()) };
                true
            } else if report
                .attempts
                .last()
                .is_some_and(|a| a.outcome == Outcome::Cancelled)
            {
                false
            } else {
                show_failure();
                false
            }
        }
        Err(error) => {
            eprintln!("renderer check could not run: {error}");
            show_failure();
            false
        }
    }
}

fn select() -> std::io::Result<Report> {
    let requested = std::env::var("CUMULUS_RENDERER").unwrap_or_else(|_| "auto".into());
    let requested = Mode::parse(&requested)
        .ok_or_else(|| std::io::Error::other("unknown CUMULUS_RENDERER mode"))?;
    let environment = cache::environment();
    let path = cache::path();
    let cached = path
        .as_ref()
        .and_then(|path| cache::read(path, &environment, cache::now()));
    let modes = policy::candidates(requested, cached, cfg!(target_os = "linux"));
    let executable = std::env::current_exe()?;
    let (selected, attempts) = policy::choose(&modes, |mode| check(&executable, mode, TIMEOUT))?;
    if requested == Mode::Auto {
        if let Some(mode) = selected {
            // Do not extend the retry period on every launch: after seven days
            // the fast path is tried again, even on daily-use machines.
            if cached != Some(mode)
                && let Some(path) = path
                && cache::write(&path, &environment, mode, cache::now()).is_err()
            {
                eprintln!("renderer preference could not be saved; startup continues");
            }
        } else if !attempts
            .last()
            .is_some_and(|a| a.outcome == Outcome::Cancelled)
            && let Some(path) = path
        {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(Report {
        schema: 1,
        selected,
        cached,
        attempts,
        real_music_requests: 0,
    })
}

fn check(executable: &Path, mode: Mode, timeout: Duration) -> std::io::Result<Outcome> {
    let root = tempfile::Builder::new()
        .prefix("cumulus-renderer-")
        .tempdir()?;
    let mut command = Command::new(executable);
    command
        .arg(probe::ARG)
        .arg(root.path())
        .env("CUMULUS_RENDERER_EFFECTIVE", mode.name())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    #[cfg(target_os = "windows")]
    let _job = match super::job::contain(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    if let Err(error) = std::fs::write(root.path().join("start"), b"start") {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(probe::outcome(status, root.path()));
        }
        if start.elapsed() >= timeout {
            #[cfg(target_os = "linux")]
            // SAFETY: child has its own process group. Kill only this graphics
            // check and its WebKit children, never the caller's process group.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            child.wait()?;
            return Ok(Outcome::TimedOut);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn show_failure() {
    // Native dialog, no WebView required. Never retry the complete music client.
    use tauri_plugin_dialog::DialogExt;
    eprintln!("Cumulus could not initialize any renderer");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let _ = tauri::Builder::default().plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            app.dialog().message("界面无法启动，已尝试可用的图形兼容模式。请更新显卡驱动或系统 WebView 运行库后重试。账号和音乐文件未被修改。")
                .title("Cumulus 启动失败").show(move |_| handle.exit(1));
            Ok(())
        }).run(context);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn child_exit_without_render_ack_and_hung_child_are_not_success() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("probe");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            check(&executable, Mode::Auto, Duration::from_secs(2)).unwrap(),
            Outcome::Failed
        );
        std::fs::write(&executable, "#!/bin/sh\nsleep 20\n").unwrap();
        let start = Instant::now();
        assert_eq!(
            check(&executable, Mode::Auto, Duration::from_millis(100)).unwrap(),
            Outcome::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(3));
        std::fs::write(&executable, "#!/bin/sh\nexit 125\n").unwrap();
        assert_eq!(
            check(&executable, Mode::Auto, Duration::from_secs(2)).unwrap(),
            Outcome::Cancelled
        );
    }
}
