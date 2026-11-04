use super::*;
use crate::{
    account::{AccountSession, profiles::ProfileRegistry},
    download::transfer::{MediaBody, MediaReply, MediaRequest},
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;

fn catalogue() -> Value {
    json!({"code": 200, "data":{"tabs":[
        {"name":"合成新品","detailVoList":[
            {"id":55, "name":"合法预览", "url":"https://p6.music.126.net/obj/fixture/catalogue.png?ticket=redacted",
             "previewUrl":"http://p5.music.126.net/obj/fixture/preview.png?token=redacted#ignored",
             "fileUrl":"https://private.invalid/payload.zip?key=redacted","packageType":1,"type":6,"cornerType":3},
            {"id":56,"name":"目录卡片","previewUrl":"https://interface.music.163.com/api/unsafe","type":15,"cornerType":4}
        ]},
        {"name":"空组","detailVoList":[]},
        {"name":"下一组","detailVoList":[{"id":57,"name":"第三项","cornerType":2}]}
    ]}})
}

#[test]
fn projection_keeps_order_and_opaque_image_keys_without_asset_or_package_urls() {
    let (catalogue, artwork) = project(&catalogue(), "test".into()).unwrap();
    let projected = serde_json::to_value(catalogue).unwrap();
    assert_eq!(
        projected,
        json!({"revision":"test","groups":[
            {"name":"合成新品","cards":[
                {"key":0,"name":"合法预览","entitlement":"vinyl","corner":"new","hasCover":true,"hasPreview":true},
                {"key":1,"name":"目录卡片","entitlement":"super_vinyl","corner":"limited","hasCover":false,"hasPreview":false}
            ]},
            {"name":"下一组","cards":[
                {"key":2,"name":"第三项","entitlement":null,"corner":"popular","hasCover":false,"hasPreview":false}
            ]}
        ]})
    );
    assert_eq!(
        artwork[0].preview.as_ref().unwrap().as_str(),
        "https://p5.music.126.net/obj/fixture/preview.png"
    );
    for bad in [
        "https://p6.music.126.net/api/logout.png",
        "https://evil.example/obj/fixture/a.png",
        "https://p6.music.126.net.evil.example/obj/fixture/a.png",
        "https://user@p6.music.126.net/obj/fixture/a.png",
        "https://p6.music.126.net/obj/fixture/a.svg",
        "https://p6.music.126.net/obj/fixture%2faccount/a.png",
        "https://p6.music.126.net/obj/../api/fixture.png",
        "https://p6.music.126.net:8080/obj/fixture/a.png",
    ] {
        assert!(artwork::url(&json!(bad)).is_none(), "{bad}");
    }
    assert!(
        artwork::url(&json!(
            "https://p1.music.126.net/A1B2C3D4E5F6G7H8I9/109951100000000000.jpg"
        ))
        .is_some()
    );
    assert!(project(&json!({"data":{"tabs":"bad"}}), "bad".into()).is_err());
    assert!(
        project(&json!({"data":{"tabs":[]}}), "empty".into())
            .unwrap()
            .0
            .groups
            .is_empty()
    );
}

#[derive(Default)]
struct ReadApi(AtomicUsize);
impl Api for ReadApi {
    fn request<'a>(&'a self, path: &'a str, payload: Value, cache: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!((path, payload, cache), (LIST, json!({}), false));
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(catalogue())
        })
    }
}
struct Bytes(Cursor<Vec<u8>>);
impl MediaBody for Bytes {
    fn read<'a>(&'a mut self, buffer: &'a mut [u8]) -> ApiFuture<'a, usize> {
        Box::pin(async move { Ok(self.0.read(buffer)?) })
    }
}
fn fixture_png() -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(10, 8, image::Rgb([20, 130, 80]))
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

#[derive(Default)]
struct Media {
    calls: AtomicUsize,
    started: Notify,
    hold: bool,
    invalid: bool,
}
impl MediaBackend for Media {
    fn get(&self, request: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            assert_eq!(
                request.url.as_str(),
                "https://p5.music.126.net/obj/fixture/preview.png"
            );
            assert_eq!((request.offset, request.if_range), (0, None));
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.started.notify_one();
            if self.hold {
                std::future::pending::<()>().await;
            }
            let bytes = if self.invalid {
                b"<svg>foreign active content</svg>".to_vec()
            } else {
                fixture_png()
            };
            Ok(MediaReply {
                status: 200,
                headers: BTreeMap::from([("content-length".into(), bytes.len().to_string())]),
                body: Box::new(Bytes(Cursor::new(bytes))),
            })
        })
    }
}

fn setup(
    media: Arc<Media>,
) -> (
    tempfile::TempDir,
    Arc<MusicClient>,
    Arc<AccountSession>,
    Arc<ReadApi>,
) {
    let directory = tempfile::tempdir().unwrap();
    let registry =
        ProfileRegistry::open(&directory.path().join("profiles"), ApiOrigin::Pc).unwrap();
    let api = Arc::new(ReadApi::default());
    let session = Arc::new(AccountSession::new(api.clone(), None));
    let client =
        MusicClient::with_session(session.clone(), registry).with_remote_skin_images(media);
    (directory, Arc::new(client), session, api)
}

#[tokio::test]
async fn only_current_verified_account_can_read_and_explicitly_select_preview() {
    let media = Arc::new(Media::default());
    let (_directory, client, session, api) = setup(media.clone());
    let guest = session.token().unwrap();
    assert!(
        client
            .remote_skin_catalog(&guest.generation.to_string())
            .await
            .is_err()
    );
    assert_eq!(api.0.load(Ordering::Relaxed), 0);
    session
        .adopt(api.clone(), json!({"userId":100}), &guest)
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let catalogue = client.remote_skin_catalog(&epoch).await.unwrap().value;
    assert_eq!(media.calls.load(Ordering::Relaxed), 0);
    assert!(
        client
            .remote_skin_image(&epoch, "wrong", 0, RemoteSkinImage::Preview)
            .await
            .is_err()
    );
    assert!(
        client
            .remote_skin_image(&epoch, &catalogue.revision, 1, RemoteSkinImage::Preview)
            .await
            .is_err()
    );
    let image = client
        .remote_skin_image(&epoch, &catalogue.revision, 0, RemoteSkinImage::Preview)
        .await
        .unwrap();
    assert!(image.value.starts_with("data:image/png;base64,iVBOR"));
    let next = client.remote_skin_catalog(&epoch).await.unwrap().value;
    assert_ne!(catalogue.revision, next.revision);
    assert!(
        client
            .remote_skin_image(&epoch, &catalogue.revision, 0, RemoteSkinImage::Preview)
            .await
            .is_err()
    );
    assert_eq!(api.0.load(Ordering::Relaxed), 2);
    assert_eq!(media.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn stale_inflight_preview_is_cancelled_on_account_switch() {
    let media = Arc::new(Media {
        hold: true,
        ..Default::default()
    });
    let (_directory, client, session, api) = setup(media.clone());
    session
        .adopt(
            api.clone(),
            json!({"userId":100}),
            &session.token().unwrap(),
        )
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let catalogue = client.remote_skin_catalog(&epoch).await.unwrap().value;
    let opening = tokio::spawn(async move {
        client
            .remote_skin_image(&epoch, &catalogue.revision, 0, RemoteSkinImage::Preview)
            .await
    });
    media.started.notified().await;
    session
        .adopt(api, json!({"userId":200}), &session.token().unwrap())
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), opening)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::StaleResponse)
    ));
}

#[tokio::test]
async fn a_successful_non_image_response_never_reaches_ipc_as_artwork() {
    let media = Arc::new(Media {
        invalid: true,
        ..Default::default()
    });
    let (_directory, client, session, api) = setup(media);
    session
        .adopt(api, json!({"userId":100}), &session.token().unwrap())
        .unwrap();
    let epoch = session.token().unwrap().generation.to_string();
    let catalogue = client.remote_skin_catalog(&epoch).await.unwrap().value;
    assert!(matches!(
        client
            .remote_skin_image(&epoch, &catalogue.revision, 0, RemoteSkinImage::Preview)
            .await,
        Err(Error::Media(_))
    ));
}

struct BadReply {
    status: u16,
    length: usize,
}
impl MediaBackend for BadReply {
    fn get(&self, _: MediaRequest) -> ApiFuture<'_, MediaReply> {
        Box::pin(async move {
            struct NeverRead;
            impl MediaBody for NeverRead {
                fn read<'a>(&'a mut self, _: &'a mut [u8]) -> ApiFuture<'a, usize> {
                    panic!("unapproved response body was read");
                }
            }
            Ok(MediaReply {
                status: self.status,
                headers: BTreeMap::from([("content-length".into(), self.length.to_string())]),
                body: Box::new(NeverRead),
            })
        })
    }
}

#[tokio::test]
async fn redirects_and_declared_over_limit_bodies_are_rejected_before_reading() {
    let url = artwork::url(&json!("https://p6.music.126.net/obj/fixture/a.png")).unwrap();
    for bad in [
        BadReply {
            status: 302,
            length: 30,
        },
        BadReply {
            status: 200,
            length: 5 * 1024 * 1024 + 1,
        },
    ] {
        assert!(matches!(
            artwork::download(&bad, url.clone()).await,
            Err(Error::Media(_))
        ));
    }
}

#[test]
fn artwork_larger_than_the_pixel_bounds_never_crosses_ipc() {
    let mut wide = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(4097, 1, image::Rgb([20, 130, 80]))
        .write_to(&mut wide, image::ImageFormat::Png)
        .unwrap();
    assert!(matches!(
        artwork::render(wide.get_ref(), RemoteSkinImage::Preview),
        Err(Error::Media("skin image dimensions exceed limit"))
    ));
}
