use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
fn server(status: &str, body: Vec<u8>, extra: &str) -> (String, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let extra = extra.to_owned();
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
                    .and_then(|s| s.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while bytes.len() < end + length {
            let n = stream.read(&mut buffer).unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buffer[..n]);
        }
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: http://{address}/must-not-follow\r\nSet-Cookie: MUST_NOT_PERSIST=1\r\nConnection: close\r\n{extra}\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        let _ = stream.write_all(&body);
        bytes
    });
    (format!("http://{address}/scoped-fixture"), worker)
}
fn request(operation: OssOperation) -> OssRequest {
    let allocation=OssAllocation::parse(&serde_json::json!({"channel":3,"bucket":"fixture-bucket","objectKey":"audio.flac","endpoint":"oss-cn-hangzhou.aliyuncs.com",
        "resourceId":"fixture","accessKeyId":"fixture-access","secretAccessKey":"fixture-secret","token":"fixture-token"})).unwrap();
    OssRequest::new(
        &allocation,
        "owned-task",
        operation,
        "Sun, 22 Nov 2015 08:16:38 GMT",
    )
    .unwrap()
}
#[tokio::test]
async fn actual_http_sends_scoped_signature_body_md5_and_never_replays_cookies_redirects_or_retries()
 {
    let backend = ReqwestOssBackend::new().unwrap();
    for (status, body) in [
        ("200 OK", b"".as_slice()),
        ("302 Found", b""),
        ("503 Service Unavailable", b""),
        (
            "200 OK",
            b"<Error><Code>InvalidPart</Code><Message>private diagnostic</Message></Error>",
        ),
    ] {
        let (url, worker) = server(
            status,
            body.to_vec(),
            "ETag: \"900150983CD24FB0D6963F7D28E17F72\"\r\n",
        );
        let mut request = request(OssOperation::Put(b"abc".to_vec()));
        request.url = url;
        let result = backend.request(request).await;
        if status == "200 OK" && body.is_empty() {
            assert_eq!(
                result.unwrap().part_etag().unwrap(),
                "900150983cd24fb0d6963f7d28e17f72"
            );
        } else {
            let error = result.err().unwrap();
            assert!(
                matches!(error,Error::HttpStatus{status:s,..} if s==status[..3].parse::<u16>().unwrap())
            );
            assert!(!error.to_string().contains("private"));
        }
        let request = String::from_utf8(worker.join().unwrap()).unwrap();
        let lower = request.to_ascii_lowercase();
        assert!(request.ends_with("\r\n\r\nabc"));
        assert!(lower.contains("content-md5: kafqmdzst7dwlj99kof/cg=="));
        assert!(lower.contains("authorization: oss fixture-access:"));
        assert!(lower.contains("x-oss-security-token: fixture-token"));
        assert!(lower.contains("x-oss-meta-cumulus-upload: owned-task"));
        assert!(lower.contains("x-oss-forbid-overwrite: true"));
        assert!(lower.contains("accept-encoding: identity"));
        assert!(!lower.contains("\r\ncookie:"));
    }
}
#[tokio::test]
async fn actual_get_streams_large_content_to_digests_and_bounds_xml_and_object_reads() {
    let backend = ReqwestOssBackend::new().unwrap();
    let data = vec![42u8; 2 * 1024 * 1024];
    let (url, worker) = server(
        "200 OK",
        data.clone(),
        "x-oss-meta-cumulus-upload: owned-task\r\n",
    );
    let mut call = request(OssOperation::Verify {
        bytes: data.len() as u64,
    });
    call.url = url;
    let reply = backend.request(call).await.unwrap();
    assert!(reply.body.is_empty());
    assert!(
        reply
            .verified_object(
                "owned-task",
                data.len() as u64,
                &hex::encode(md5::Md5::digest(&data)),
                &hex::encode(sha2::Sha256::digest(&data))
            )
            .unwrap()
            .content_verified
    );
    worker.join().unwrap();
    for (operation, body) in [
        (OssOperation::Initiate, vec![b' '; 1024 * 1024 + 1]),
        (OssOperation::Verify { bytes: 3 }, b"abcd".to_vec()),
    ] {
        let (url, worker) = server("200 OK", body, "");
        let mut call = request(operation);
        call.url = url;
        assert!(matches!(
            backend.request(call).await,
            Err(Error::Response(_))
        ));
        worker.join().unwrap();
    }
}
