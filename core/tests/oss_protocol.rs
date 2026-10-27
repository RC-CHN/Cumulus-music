use cumulus_core::{Error, cloud::oss::*};
use serde_json::{Value, json};
use sha2::Digest;
use std::collections::BTreeMap;
const DATE: &str = "Sun, 22 Nov 2015 08:16:38 GMT";
fn raw() -> Value {
    json!({"channel":3,"bucket":"oracle-bucket","objectKey":"音乐/曲目 +%.flac","endpoint":"oss-cn-hangzhou.aliyuncs.com",
    "resourceId":"R","accessKeyId":"oracle-key","secretAccessKey":"oracle-secret","token":"oracle-sts-token"})
}
fn allocation() -> OssAllocation {
    OssAllocation::parse(&raw()).unwrap()
}
fn reply(body: &str) -> OssReply {
    OssReply {
        status: 200,
        headers: BTreeMap::new(),
        body: body.as_bytes().into(),
        digest: None,
    }
}
fn xml(root: &str, fields: &str) -> String {
    format!("<{root}><Bucket>oracle-bucket</Bucket><Key>音乐/曲目 +%.flac</Key>{fields}</{root}>")
}
#[test]
fn signatures_match_twenty_five_frozen_unmodified_official_sdk_cases() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/oss-signatures.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 25);
    for case in cases {
        let headers = serde_json::from_value(case["headers"].clone()).unwrap();
        let params = serde_json::from_value(case["params"].clone()).unwrap();
        let signature = sign_v1(
            case["method"].as_str().unwrap(),
            case["bucket"].as_str().unwrap(),
            case["key"].as_str().unwrap(),
            &headers,
            &params,
            case["secret"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(
            format!("OSS {}:{signature}", case["access"].as_str().unwrap()),
            case["authorization"].as_str().unwrap()
        );
    }
    let duplicate = BTreeMap::from([("Date".into(), DATE.into()), ("date".into(), DATE.into())]);
    assert!(
        sign_v1(
            "GET",
            "bucket",
            "key",
            &duplicate,
            &BTreeMap::new(),
            "secret"
        )
        .is_err()
    );
    assert!(
        sign_v1(
            "GET",
            "bucket",
            "key",
            &BTreeMap::new(),
            &BTreeMap::from([("callback".into(), "x".into())]),
            "secret"
        )
        .is_err()
    );
}
#[test]
fn crc64_matches_eleven_official_sdk_vectors_whole_streamed_and_restored() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/oss-crc64.json")).unwrap();
    let bytes = include_bytes!("fixtures/oss-crc64.bin");
    assert_eq!(
        hex::encode(sha2::Sha256::digest(bytes)),
        fixture["data_sha256"]
    );
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 11);
    for case in cases {
        let start = case["offset"].as_u64().unwrap() as usize;
        let len = case["length"].as_u64().unwrap() as usize;
        let split = case["split"].as_u64().unwrap() as usize;
        let data = &bytes[start..start + len];
        let expected = case["expected"].as_str().unwrap().parse::<u64>().unwrap();
        let prefix = case["prefix"].as_str().unwrap().parse::<u64>().unwrap();
        assert_eq!(crc64(data, 0), expected);
        assert_eq!(crc64(&data[..split], 0), prefix);
        assert_eq!(crc64(&data[split..], prefix), expected);
        assert_eq!(
            data.chunks(97)
                .fold(0, |previous, chunk| crc64(chunk, previous)),
            expected
        );
    }
    assert!(!check_crc64(None, 17).unwrap());
    assert!(!check_crc64(Some("0"), 17).unwrap());
    assert!(check_crc64(Some("17"), 17).unwrap());
    for value in ["18", "-1", "+17", "18446744073709551616", " 17", ""] {
        assert!(check_crc64(Some(value), 17).is_err());
    }
}
#[test]
fn allocation_accepts_only_scoped_hosts_and_complete_same_object_credentials() {
    for endpoint in [
        "http://oss-cn-hangzhou.aliyuncs.com",
        "https://oracle-bucket.oss-cn-hangzhou.aliyuncs.com/",
    ] {
        let mut value = raw();
        value["endpoint"] = json!(endpoint);
        assert_eq!(
            OssAllocation::parse(&value).unwrap().endpoint(),
            "https://oracle-bucket.oss-cn-hangzhou.aliyuncs.com"
        );
    }
    for (key, value) in [
        ("channel", json!(1)),
        ("key", json!("different")),
        ("objectKey", json!("/leading")),
        ("objectKey", json!("a/../b")),
        ("objectKey", json!("./b")),
        ("bucket", json!("Aaa")),
        ("bucket", json!("ab")),
        ("bucket", json!("-bad")),
        ("token", json!("bad\r\nheader")),
        ("secretAccessKey", json!("")),
        (
            "endpoint",
            json!("https://other.oss-cn-hangzhou.aliyuncs.com"),
        ),
        (
            "endpoint",
            json!("https://oss-cn-hangzhou.aliyuncs.com.evil.test"),
        ),
        (
            "endpoint",
            json!("https://user@oss-cn-hangzhou.aliyuncs.com"),
        ),
        (
            "endpoint",
            json!("https://oss-cn-hangzhou.aliyuncs.com/path"),
        ),
        (
            "endpoint",
            json!("https://oss-cn-hangzhou.aliyuncs.com:444"),
        ),
        (
            "endpoint",
            json!("https://oss-cn-hangzhou.aliyuncs.com?q=x"),
        ),
        ("resourceId", Value::Null),
    ] {
        let mut a = raw();
        a[key] = value;
        assert!(OssAllocation::parse(&a).is_err(), "field {key}");
    }
    let first = allocation();
    let mut other = raw();
    other["token"] = json!("renewed");
    other["accessKeyId"] = json!("renewed-key");
    assert!(first.same_object(&OssAllocation::parse(&other).unwrap()));
    other["resourceId"] = json!("R2");
    assert!(!first.same_object(&OssAllocation::parse(&other).unwrap()));
}
#[test]
fn request_encodes_unicode_plus_percent_slashes_and_signs_exact_binary_body() {
    let a = allocation();
    let request = OssRequest::new(
        &a,
        "task-owned",
        OssOperation::Part {
            upload_id: "id+with/=special space".into(),
            number: 2,
            data: b"abc".to_vec(),
        },
        DATE,
    )
    .unwrap();
    assert_eq!(
        request.url(),
        "https://oracle-bucket.oss-cn-hangzhou.aliyuncs.com/%E9%9F%B3%E4%B9%90/%E6%9B%B2%E7%9B%AE%20%2B%25.flac?partNumber=2&uploadId=id%2Bwith%2F%3Dspecial%20space"
    );
    assert_eq!(request.body(), Some(b"abc".as_slice()));
    assert_eq!(request.headers()["Content-MD5"], "kAFQmDzST7DWlj99KOF/cg==");
    assert_eq!(request.headers()["Content-Length"], "3");
    assert_eq!(request.headers()["Accept-Encoding"], "identity");
    assert!(!request.headers().contains_key("Cookie"));
    let put = OssRequest::new(&a, "task-owned", OssOperation::Put(b"abc".to_vec()), DATE).unwrap();
    assert_eq!(put.headers()["x-oss-meta-cumulus-upload"], "task-owned");
    assert_eq!(put.headers()["x-oss-forbid-overwrite"], "true");
    let complete = OssRequest::new(
        &a,
        "task-owned",
        OssOperation::Complete {
            upload_id: "u".into(),
            parts: vec![PartTag {
                number: 1,
                etag: "900150983cd24fb0d6963f7d28e17f72".into(),
            }],
        },
        DATE,
    )
    .unwrap();
    assert_eq!(
        std::str::from_utf8(complete.body().unwrap()).unwrap(),
        "<?xml version='1.0' encoding='utf-8'?>\n<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>\"900150983CD24FB0D6963F7D28E17F72\"</ETag></Part></CompleteMultipartUpload>"
    );
    assert_eq!(complete.headers()["Content-Type"], "application/xml");
    let mut key = raw();
    key["objectKey"] = json!("目录//emoji-😀/%2e.flac");
    let a = OssAllocation::parse(&key).unwrap();
    assert!(
        OssRequest::new(&a, "owned", OssOperation::Head, DATE)
            .unwrap()
            .url()
            .ends_with("/%E7%9B%AE%E5%BD%95//emoji-%F0%9F%98%80/%252e.flac")
    );
}
#[test]
fn xml_scopes_namespaces_entities_and_invalid_documents_are_checked_before_accepting_upload_id() {
    let a = allocation();
    let valid = xml(
        "InitiateMultipartUploadResult",
        "<UploadId>u+id/=x</UploadId>",
    );
    assert_eq!(reply(&valid).initiated(&a).unwrap(), "u+id/=x");
    let prefixed = valid
        .replace(
            "<InitiateMultipartUploadResult>",
            "<x:InitiateMultipartUploadResult xmlns:x=\"urn:oss\">",
        )
        .replace(
            "</InitiateMultipartUploadResult>",
            "</x:InitiateMultipartUploadResult>",
        );
    assert_eq!(reply(&prefixed).initiated(&a).unwrap(), "u+id/=x");
    for bad in [
        valid.replace("oracle-bucket", "foreign"),
        valid.replace("<UploadId>", "<UploadId>duplicate</UploadId><UploadId>"),
        valid.replace("音乐/曲目 +%.flac", "wrong"),
        format!("<!DOCTYPE X [<!ENTITY x 'expanded'>]>{valid}"),
        valid.replace("u+id/=x", "&unbound;"),
        valid.replace("</UploadId>", "</Other>"),
        format!("{valid}{valid}"),
        "<a>".repeat(20),
        " ".repeat(1024 * 1024 + 1),
    ] {
        assert!(reply(&bad).initiated(&a).is_err());
    }
    let mut key = raw();
    key["objectKey"] = json!("a<&b.flac");
    let a = OssAllocation::parse(&key).unwrap();
    let data = "<InitiateMultipartUploadResult><Bucket>oracle-bucket</Bucket><Key>a&lt;&amp;b.flac</Key><UploadId>U</UploadId></InitiateMultipartUploadResult>";
    assert_eq!(reply(data).initiated(&a).unwrap(), "U");
}
#[test]
fn part_pages_reject_duplicate_identity_bad_order_and_skipping_markers() {
    let a = allocation();
    let fields = "<UploadId>U</UploadId><IsTruncated>true</IsTruncated><NextPartNumberMarker>2</NextPartNumberMarker><Part><PartNumber>2</PartNumber><Size>123</Size><ETag>\"900150983CD24FB0D6963F7D28E17F72\"</ETag></Part>";
    let valid = xml("ListPartsResult", fields);
    let page = reply(&valid).parts(&a, "U", 1).unwrap();
    assert_eq!(page.next_marker, Some(2));
    assert_eq!(page.parts[0].bytes, 123);
    for bad in [
        valid.replace(">2</NextPartNumberMarker>", ">3</NextPartNumberMarker>"),
        valid.replace(">2</PartNumber>", ">1</PartNumber>"),
        valid.replace(">123</Size>", ">0</Size>"),
        valid.replace(">U</UploadId>", ">other</UploadId>"),
        valid.replace("true", "maybe"),
        valid.replace("900150983CD24FB0D6963F7D28E17F72", "not-an-etag"),
    ] {
        assert!(reply(&bad).parts(&a, "U", 1).is_err());
    }
}
#[test]
fn completion_checks_available_crc_but_never_uses_multipart_etag_as_whole_file_md5() {
    let a = allocation();
    let data = xml(
        "CompleteMultipartUploadResult",
        "<ETag>\"multipart-final-not-md5-3\"</ETag>",
    );
    let mut response = reply(&data);
    assert!(!response.multipart_completed(&a, 91).unwrap().crc64_verified);
    response
        .headers
        .insert("x-oss-hash-crc64ecma".into(), "0".into());
    assert!(!response.multipart_completed(&a, 91).unwrap().crc64_verified);
    response
        .headers
        .insert("x-oss-hash-crc64ecma".into(), "91".into());
    assert!(response.multipart_completed(&a, 91).unwrap().crc64_verified);
    response
        .headers
        .insert("x-oss-hash-crc64ecma".into(), "92".into());
    assert!(response.multipart_completed(&a, 91).is_err());
    let mut put = reply("");
    put.headers
        .insert("etag".into(), "\"900150983CD24FB0D6963F7D28E17F72\"".into());
    assert!(
        put.put_completed("900150983cd24fb0d6963f7d28e17f72", 0)
            .is_ok()
    );
    assert!(
        put.put_completed("00000000000000000000000000000000", 0)
            .is_err()
    );
    let error = reply("<Error><Code>InvalidPart</Code><Message>private data</Message></Error>")
        .multipart_completed(&a, 0)
        .err()
        .unwrap();
    assert!(matches!(error, Error::HttpStatus { status: 200, .. }));
    assert!(!error.to_string().contains("private"));
}
#[test]
fn readonly_reconciliation_requires_nonce_size_and_both_actual_content_digests() {
    let mut response = reply("");
    let md5 = hex::encode(md5::Md5::digest(b"abc"));
    let sha = hex::encode(sha2::Sha256::digest(b"abc"));
    response
        .headers
        .insert("x-oss-meta-cumulus-upload".into(), "owned".into());
    response.headers.insert("content-length".into(), "3".into());
    response.object_headers("owned", 3).unwrap();
    assert!(response.verified_object("owned", 3, &md5, &sha).is_err());
    response.digest = Some(ObjectDigest {
        bytes: 3,
        md5: md5.clone(),
        sha256: sha.clone(),
    });
    assert!(
        response
            .verified_object("owned", 3, &md5, &sha)
            .unwrap()
            .content_verified
    );
    assert!(response.verified_object("other", 3, &md5, &sha).is_err());
    assert!(response.verified_object("owned", 4, &md5, &sha).is_err());
    assert!(
        response
            .verified_object("owned", 3, &md5, &"0".repeat(64))
            .is_err()
    );
}
