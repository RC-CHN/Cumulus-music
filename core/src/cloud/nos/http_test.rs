use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
fn server(status: &str, body: Vec<u8>) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
        }
        let end = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap() + 4;
        let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
        let length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|v| v.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while bytes.len() < end + length {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
        }
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: http://{address}/must-not-follow\r\nSet-Cookie: SHOULD_NOT_PERSIST=1\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        let _ = stream.write_all(&body);
        String::from_utf8(bytes).unwrap()
    });
    (
        format!("http://{address}/object?offset=0&complete=true&version=1.0"),
        worker,
    )
}
#[tokio::test]
async fn native_http_posts_exact_bytes_hex_file_md5_and_only_object_token_with_no_redirect_or_cookie_jar()
 {
    let backend = ReqwestNosBackend::new().unwrap();
    for status in ["200 OK", "302 Found", "503 Service Unavailable"] {
        let (url, worker) = server(status, br#"{"offset":3,"context":"fixture"}"#.to_vec());
        let request = NosRequest {
            url,
            token: "SCOPED_TEST".into(),
            md5: "900150983cd24fb0d6963f7d28e17f72".into(),
            content_type: "audio/flac".into(),
            method: NosMethod::Chunk,
            body: b"abc".to_vec(),
        };
        let result = backend.request(request).await;
        match status {
            "200 OK" => assert_eq!(result.unwrap()["offset"], 3),
            "302 Found" => assert!(matches!(result, Err(Error::HttpStatus { status: 302, .. }))),
            _ => assert!(matches!(result, Err(Error::HttpStatus { status: 503, .. }))),
        }
        let request = worker.join().unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(request.ends_with("\r\n\r\nabc"));
        assert!(lower.contains("content-md5: 900150983cd24fb0d6963f7d28e17f72"));
        assert!(lower.contains("x-nos-token: scoped_test"));
        assert!(lower.contains("accept-encoding: identity"));
        assert!(!lower.contains("\r\ncookie:"));
        assert!(!lower.contains("\r\nauthorization:"));
    }
}
#[tokio::test]
async fn native_http_rejects_non_object_and_oversized_responses() {
    let backend = ReqwestNosBackend::new().unwrap();
    for body in [b"[]".to_vec(), vec![b' '; 1024 * 1024 + 1]] {
        let (url, worker) = server("200 OK", body);
        let result = backend.json(backend.client.get(url)).await;
        assert!(matches!(result, Err(Error::Response(_))));
        worker.join().unwrap();
    }
}
