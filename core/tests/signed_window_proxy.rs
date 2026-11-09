#![cfg(all(target_os = "linux", feature = "signed-window-read-proof"))]

use cumulus_core::{
    download::transfer::{MediaBackend, MediaRequest, MediaUrl, ReqwestMediaBackend},
    protocol::http::{ApiOrigin, HttpBackend, RequestPlan, ReqwestBackend, WireRoute},
};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    thread,
    time::{Duration, Instant},
};

const LIMIT: Duration = Duration::from_secs(3);

fn denying_proxy() -> (SocketAddr, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let accepted = thread::spawn(move || {
        let deadline = Instant::now() + LIMIT;
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "missing proxy request");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("test proxy accept failed: {error}"),
            }
        };
        stream.set_read_timeout(Some(LIMIT)).unwrap();
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            assert!(head.len() < 2048);
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            head.extend_from_slice(&byte);
        }
        stream
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
        String::from_utf8(head).unwrap()
    });
    (address, accepted)
}

fn require_exact_connect(head: String, authority: &str) {
    let mut lines = head.split("\r\n");
    assert_eq!(
        lines.next().unwrap(),
        format!("CONNECT {authority}:443 HTTP/1.1")
    );
    assert!(lines.any(|line| {
        let line = line.to_ascii_lowercase();
        line.strip_prefix("host:")
            .is_some_and(|value| [authority, &format!("{authority}:443")].contains(&value.trim()))
    }));
    assert!(!head.contains("synthetic-private-body"));
}

#[test]
fn proof_constructors_reject_other_broker_addresses() {
    for address in [
        "127.0.0.1:0",
        "127.0.0.2:3128",
        "[::1]:3128",
        "0.0.0.0:3128",
    ] {
        let address = address.parse().unwrap();
        assert!(ReqwestBackend::for_signed_read_proof(LIMIT, address).is_err());
        assert!(ReqwestMediaBackend::for_signed_read_proof(LIMIT, address).is_err());
    }
}

#[tokio::test]
async fn native_api_and_media_use_only_the_explicit_connect_broker() {
    let (broker, accepted) = denying_proxy();
    let backend = ReqwestBackend::for_signed_read_proof(LIMIT, broker).unwrap();
    let result = backend
        .post(RequestPlan {
            url: ApiOrigin::Pc
                .request_url(WireRoute::Eapi, "/api/w/nuser/account/get")
                .unwrap(),
            headers: Default::default(),
            body: b"synthetic-private-body".to_vec(),
            route: WireRoute::Eapi,
            decrypt_response: false,
        })
        .await;
    assert!(result.is_err());
    require_exact_connect(accepted.join().unwrap(), "interfacepc.music.163.com");

    let (broker, accepted) = denying_proxy();
    let backend = ReqwestMediaBackend::for_signed_read_proof(LIMIT, broker).unwrap();
    let result = backend
        .get(MediaRequest {
            url: MediaUrl::parse("https://m1.music.126.net/proof-synthetic.m4a").unwrap(),
            offset: 0,
            if_range: None,
        })
        .await;
    assert!(result.is_err());
    require_exact_connect(accepted.join().unwrap(), "m1.music.126.net");
}
