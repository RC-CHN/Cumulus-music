//! Explicit passive diagnostic; it never changes the interface table.

#[cfg(all(windows, target_arch = "x86_64"))]
mod runtime {
    use cumulus_windows_address_watch::{AddressWatcher, Close, NativeError, Terminal, Wake};
    use std::time::Duration;

    #[derive(Default)]
    struct Round {
        pending_arm: bool,
        exclusive: Option<bool>,
        before_close: Option<&'static str>,
        cancel_accepted: Option<bool>,
        terminal: Option<&'static str>,
    }

    impl Round {
        fn pass(&self) -> bool {
            self.pending_arm
                && self.exclusive == Some(true)
                && self.before_close == Some("idle")
                && self.cancel_accepted == Some(true)
                && self.terminal == Some("cancelled")
        }

        fn json(&self) -> String {
            format!(
                concat!(
                    "{{\"pendingArm\":{},\"exclusive\":{},",
                    "\"beforeClose\":{},\"cancelAccepted\":{},\"terminal\":{}}}"
                ),
                self.pending_arm,
                optional_bool(self.exclusive),
                optional_label(self.before_close),
                optional_bool(self.cancel_accepted),
                optional_label(self.terminal)
            )
        }
    }

    fn optional_bool(value: Option<bool>) -> &'static str {
        match value {
            Some(true) => "true",
            Some(false) => "false",
            None => "null",
        }
    }

    fn optional_label(value: Option<&str>) -> String {
        value.map_or_else(|| "null".to_owned(), |label| format!("\"{label}\""))
    }

    fn arm_and_cancel() -> Round {
        let Ok(mut watch) = AddressWatcher::start() else {
            return Round::default();
        };
        let exclusive = matches!(AddressWatcher::start(), Err(NativeError::Active));
        let before_close = match watch.wait(Duration::ZERO) {
            Ok(Wake::Idle) => "idle",
            Ok(Wake::Changed { .. }) => "changed",
            Ok(Wake::Cancelled) => "cancelled",
            Err(_) => "error",
        };
        let (cancel_accepted, terminal) = match watch.close() {
            Close::NoPending => (None, "unarmed"),
            Close::Acknowledged {
                cancel_accepted,
                terminal,
            } => (
                Some(cancel_accepted),
                match terminal {
                    Terminal::Changed => "changed",
                    Terminal::Cancelled => "cancelled",
                },
            ),
            Close::Retained { cancel_accepted } => (Some(cancel_accepted), "retained"),
        };
        Round {
            pending_arm: true,
            exclusive: Some(exclusive),
            before_close: Some(before_close),
            cancel_accepted,
            terminal: Some(terminal),
        }
    }

    pub fn run() {
        let first = arm_and_cancel();
        let second = first.pass().then(arm_and_cancel);
        println!(
            concat!(
                "{{\"nativeWindowsIpv4Watch\":true,\"addressDataRead\":false,",
                "\"networkConfigurationChanged\":false,\"playbackEnabled\":false,",
                "\"first\":{},\"second\":{}}}"
            ),
            first.json(),
            second
                .as_ref()
                .map_or_else(|| "null".to_owned(), Round::json)
        );
        if !second.is_some_and(|round| round.pass()) {
            std::process::exit(1);
        }
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn main() {
    runtime::run();
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("This passive diagnostic requires Windows x86_64.");
    std::process::exit(2);
}
