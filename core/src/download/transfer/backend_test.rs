use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::Instant,
};

#[tokio::test]
async fn actual_http_has_no_cookie_replay_redirect_retry_or_transparent_decompression() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    let server = std::thread::spawn(move || {
        for index in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing HTTP request");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("listener failed: {e}"),
                }
            };
            // Winsock may inherit nonblocking mode from the listener. This
            // fixture uses blocking reads with a deadline after accepting.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut b = [0; 1];
            while !bytes.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut b).unwrap();
                bytes.push(b[0]);
                assert!(bytes.len() < 8192);
            }
            seen.lock()
                .unwrap()
                .push(String::from_utf8(bytes).unwrap().to_lowercase());
            if index == 0 {
                write!(stream,"HTTP/1.1 302 Found\r\nLocation: http://{addr}/redirected\r\nSet-Cookie: fixture=synthetic; Path=/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            } else {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Encoding: gzip\r\nConnection: close\r\n\r\nabc").unwrap();
            }
        }
    });
    // Only this private unit test bypasses HTTPS/CDN URL construction.
    let backend = ReqwestMediaBackend {
        client: client_builder(Duration::from_secs(2))
            .no_proxy()
            .build()
            .unwrap(),
    };
    let request = MediaRequest {
        url: MediaUrl(format!("http://{addr}/first")),
        offset: 0,
        if_range: None,
    };
    assert_eq!(backend.get(request).await.unwrap().status, 302);
    let mut reply = backend
        .get(MediaRequest {
            url: MediaUrl(format!("http://{addr}/second")),
            offset: 2,
            if_range: Some("\"fixture\"".into()),
        })
        .await
        .unwrap();
    assert_eq!(reply.headers["content-encoding"], "gzip");
    let mut out = [0u8; 3];
    let mut used = 0;
    while used < out.len() {
        let count = reply.body.read(&mut out[used..]).await.unwrap();
        assert!(count > 0, "truncated media response");
        used += count;
    }
    assert_eq!(&out, b"abc");
    server.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].starts_with("get /second "));
    for request in requests.iter() {
        assert!(request.contains("accept-encoding: identity\r\n"));
        assert!(!request.contains("\r\ncookie:"));
        assert!(!request.contains("\r\nauthorization:"));
    }
    assert!(requests[1].contains("range: bytes=2-\r\n"));
    assert!(requests[1].contains("if-range: \"fixture\"\r\n"));
}
