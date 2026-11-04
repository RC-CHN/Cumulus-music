use cumulus_core::{
    Error,
    pc_connectivity::{
        PcConnectivity, PcConnectivityEffect as Effect, PcConnectivitySignal as Signal,
        PcRequestObservation as Request,
    },
};
use serde::Deserialize;
use std::time::Duration;

fn ms(time: u64) -> Duration {
    Duration::from_millis(time)
}

#[derive(Deserialize)]
struct Trace {
    name: String,
    initial: bool,
    actions: Vec<(u64, Option<String>)>,
    samples: Vec<(u64, bool)>,
    stable: Vec<(u64, bool)>,
    probes: Vec<(u64, bool)>,
}

#[test]
fn matches_original_m119_and_installed_rx_with_a_virtual_host() {
    for line in include_str!("fixtures/official_pc_connectivity_3_1_42.jsonl").lines() {
        let trace: Trace = serde_json::from_str(line).unwrap();
        let mut model = PcConnectivity::new(trace.initial, ms(0));
        let mut navigator = trace.initial;
        let mut stable = vec![(0, model.is_online())];
        let mut probes = Vec::new();
        let mut samples = Vec::new();
        let mut observe = |when, effect| match effect {
            Some(Effect::ConfirmedOnline(online)) => stable.push((when, online)),
            Some(Effect::RestartPlaylistCountPollingNow) => probes.push((when, true)),
            None => {}
        };
        for (at, action) in trace.actions {
            while let Some(due) = model.next_probe_at().filter(|due| *due <= ms(at)) {
                observe(
                    due.as_millis() as u64,
                    model.poll_probe(due, navigator).unwrap(),
                );
            }
            let signal = match action.as_deref() {
                Some("native-off") => Some(Signal::NativeNotification { online: false }),
                Some("native-on") => Some(Signal::NativeNotification { online: true }),
                Some("browser-on" | "browser-off") => {
                    navigator = action.as_deref() == Some("browser-on");
                    Some(Signal::BrowserNotification { online: navigator })
                }
                Some("request-on") => Some(Signal::Request(Request::ResolvedBeforeFormatting)),
                Some("request-off") => Some(Signal::Request(Request::OtherWrapperRejection)),
                None => None,
                _ => panic!("unknown recorded connectivity input"),
            };
            if let Some(signal) = signal {
                observe(at, model.signal(ms(at), signal).unwrap());
            }
            samples.push((at, model.is_online()));
        }
        assert_eq!(stable, trace.stable, "{}", trace.name);
        assert_eq!(samples, trace.samples, "{}", trace.name);
        assert_eq!(probes, trace.probes, "{}", trace.name);
    }
}

#[test]
fn response_and_encryption_errors_do_not_trigger_or_confirm_a_probe() {
    let mut model = PcConnectivity::new(true, ms(0));
    for (time, result) in [
        (0, Request::ResponseError),
        (50, Request::FrontendEncryptionError),
        (6050, Request::ResponseError),
    ] {
        assert_eq!(
            model.signal(ms(time), Signal::Request(result)).unwrap(),
            None
        );
        assert!(model.is_online());
        assert_eq!(model.next_probe_at(), None);
    }

    let mut offline = PcConnectivity::new(false, ms(0));
    for result in [Request::ResponseError, Request::FrontendEncryptionError] {
        assert_eq!(
            offline.signal(ms(1), Signal::Request(result)).unwrap(),
            None
        );
        assert!(!offline.is_online());
        assert_eq!(offline.next_probe_at(), None);
    }
}

#[test]
fn stale_timer_wakes_and_invalid_monotonic_times_cannot_change_connectivity() {
    let mut model = PcConnectivity::new(true, ms(0));
    model
        .signal(ms(1), Signal::BrowserNotification { online: false })
        .unwrap();
    assert_eq!(model.next_probe_at(), Some(ms(5001)));
    model
        .signal(ms(102), Signal::BrowserNotification { online: false })
        .unwrap();
    assert_eq!(model.next_probe_at(), Some(ms(5102)));
    assert_eq!(model.poll_probe(ms(5001), false).unwrap(), None);
    assert!(model.is_online());
    assert!(matches!(
        model.poll_probe(ms(4000), false),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(model.next_probe_at(), Some(ms(5102)));
}
