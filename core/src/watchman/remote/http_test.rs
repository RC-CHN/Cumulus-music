use super::*;
use crate::watchman::{ApiHost, ReportKind, RequestKind, encode_packet};
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
        let mut buf = [0; 4096];
        while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
            let n = stream.read(&mut buf).unwrap();
            assert_ne!(n, 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        let end = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap() + 4;
        let header = String::from_utf8_lossy(&bytes[..end]);
        let len = header
            .lines()
            .find_map(|l| {
                l.to_ascii_lowercase()
                    .strip_prefix("content-length:")
                    .and_then(|v| v.trim().parse::<usize>().ok())
            })
            .unwrap_or(0);
        while bytes.len() < end + len {
            let n = stream.read(&mut buf).unwrap();
            assert_ne!(n, 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        write!(stream,"HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: http://{address}/must-not-follow\r\nSet-Cookie: DO_NOT_STORE=1; Path=/\r\nConnection: close\r\n\r\n",body.len()).unwrap();
        let _ = stream.write_all(&body);
        String::from_utf8(bytes).unwrap()
    });
    (format!("http://{address}/probe"), worker)
}
#[tokio::test]
async fn exact_native_report_form_cb_response_and_cookie_free_transport() {
    let backend = NativeBackend::new("Cumulus Native Transport Test").unwrap();
    let request = Request {
        host: ApiHost::Primary,
        callback: "__wmjsonp_synthetic".into(),
        kind: RequestKind::Report {
            build: crate::watchman::SdkBuild::Af2952a4,
            kind: ReportKind::Device,
            packet: encode_packet(b"synthetic", [0, 127, 128, 255]).unwrap(),
        },
    };
    for status in ["200 OK", "302 Found", "503 Unavailable"] {
        let (url, worker) = server(
            status,
            b"__wmjsonp_synthetic([200,0,\"tid\",\"did\",null,\"ni\"]);".to_vec(),
        );
        let result = backend.execute(&request, &url).await;
        if status == "200 OK" {
            assert_eq!(result.unwrap()[0], 200);
        } else {
            assert!(matches!(result, Err(Error::HttpStatus { .. })));
        }
        let raw = worker.join().unwrap();
        assert!(raw.ends_with(&request.form().unwrap()));
        let lower = raw.to_ascii_lowercase();
        assert!(lower.contains("content-type: application/x-www-form-urlencoded"));
        assert!(!lower.contains("\r\ncookie:"));
        assert!(!lower.contains("\r\nauthorization:"));
        assert!(!lower.contains("music_u"));
    }
}
#[tokio::test]
async fn native_configuration_get_and_malformed_or_oversized_response_rejection() {
    let backend = NativeBackend::new("Cumulus Test").unwrap();
    let request = Request {
        host: ApiHost::Secondary,
        callback: "__wmjsonp_test".into(),
        kind: RequestKind::Configuration {
            product: "YD00000558929251".into(),
            cache_key: "a+b".into(),
            timestamp_ms: 1000,
        },
    };
    for body in [
        b"__wmjsonp_wrong({\"code\":200})".to_vec(),
        b"__wmjsonp_test({bad})".to_vec(),
        vec![0; crate::watchman::MAX_BYTES + 1],
    ] {
        let (url, worker) = server("200 OK", body);
        assert!(
            backend
                .execute(&request, &format!("{url}?{}", request.form().unwrap()))
                .await
                .is_err()
        );
        let text = worker.join().unwrap();
        assert!(text.starts_with(
            "GET /probe?pn=YD00000558929251&cvk=a%2Bb&cb=__wmjsonp_test&t=1000 HTTP/1.1"
        ));
    }
}
