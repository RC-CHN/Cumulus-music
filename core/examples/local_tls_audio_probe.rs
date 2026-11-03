//! Ordinary libmpv HTTPS URL decoding against a closed synthetic fixture.
#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("local TLS audio probe requires Linux namespace isolation");
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use cumulus_core::{
        download::transfer::MediaUrl,
        playback::engine::{
            AudioEngine, Cancellation, LoadRequest, MediaInput, MpvConfig, MpvEngine,
        },
    };
    use std::{
        net::{Ipv4Addr, ToSocketAddrs},
        path::Path,
        thread,
        time::{Duration, Instant},
    };

    // Fail before URL load if this was not started by the closed fixture.
    const HOST: &str = "cumulus-local-fixture.music.126.net";
    assert!(Path::new("/proof-local-tls").is_file());
    assert!(!Path::new("/etc/resolv.conf").exists());
    let resolved = (HOST, 443).to_socket_addrs()?.collect::<Vec<_>>();
    assert_eq!(
        resolved,
        [std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, 443))]
    );
    let url = MediaUrl::parse(&format!("https://{HOST}/synthetic/audio.m4a"))?;
    let mut engine = MpvEngine::open(MpvConfig {
        null_output: true,
        ..Default::default()
    })?;
    let mut request = LoadRequest::new(MediaInput::Url(url));
    request.timeout = Duration::from_secs(6);
    let loaded = engine.load(request, &Cancellation::default())?;
    assert_eq!(loaded.codec.as_deref(), Some("aac"));
    engine.pause(false)?;
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let state = engine.poll()?;
        if state.audio_output.as_deref() == Some("null")
            && state.position.is_some_and(|position| position > 0.12)
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "URL playback clock did not advance"
        );
        thread::sleep(Duration::from_millis(15));
    }
    engine.pause(true)?;
    engine.stop()?;
    assert!(engine.poll()?.entry_id.is_none());
    println!("ordinary-url-aac-clock-ok");
    Ok(())
}
