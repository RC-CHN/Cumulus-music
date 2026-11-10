use super::*;
use cumulus_core::{
    Error,
    account::profiles::ProfileRegistry,
    client::{MusicClient, dto::*},
    podcast::{PodcastScope, PurchasedKind},
    protocol::http::ApiOrigin,
};

const BOUGHT: &str = "/api/voicelist/bought/list";
const LIKED: &str = "/api/content/my/liked/voice";
const RIGHTS: &str = "/api/dj/voices/download/check";
const MUSICS: &str = "/api/dj/program/song/musics";

fn radio() -> Value {
    json!({"productType":0,"id":81,"name":"购买播单 81","programCount":20})
}
fn voice() -> Value {
    let mut item = program();
    item["productType"] = json!(1);
    item["buyed"] = json!(true);
    item["programFeeType"] = json!(10);
    item
}
fn paid(rows: Vec<Value>, more: bool) -> Reply {
    json!({"code":200,"data":{"count":3,"list":rows,"hasMore":more}}).into()
}
fn liked() -> Reply {
    json!({"code":200,"data":{"data":[],"hasMore":false}}).into()
}
fn fixture(replies: Vec<Reply>) -> (Fixture, MusicClient) {
    let f = Fixture::new(replies);
    let client = MusicClient::with_session(
        f.session.clone(),
        ProfileRegistry::open(&f.root.path().join("state"), ApiOrigin::Pc).unwrap(),
    )
    .with_download_backend(f.media.clone());
    (f, client)
}
fn query(offset: usize, revision: Option<&str>) -> PodcastQuery {
    PodcastQuery {
        scope: PodcastScope::Purchased,
        keyword: String::new(),
        offset,
        limit: 50,
        revision: revision.map(str::to_owned),
    }
}
fn target(revision: &str, kind: PurchasedKind, id: &str) -> PodcastPurchasedTarget {
    PodcastPurchasedTarget {
        kind,
        id: id.into(),
        revision: revision.into(),
    }
}
fn rights(revision: &str) -> PodcastPurchasedTarget {
    target(revision, PurchasedKind::Episode, "81")
}
fn assert_path(f: &Fixture, expected: &[&str]) {
    assert_eq!(
        f.api
            .calls()
            .iter()
            .map(|call| call.path.as_str())
            .collect::<Vec<_>>(),
        expected,
    );
}

#[tokio::test]
async fn purchased_mixed_rows_keep_namespace_order_and_require_explicit_twenty_offset() {
    let (f, client) = fixture(vec![
        paid(vec![radio(), voice()], true),
        liked(),
        paid(
            vec![json!({"productType":7,"id":81,"name":"unknown type"})],
            true,
        ),
        paid(
            vec![voice(), json!({"id":86,"name":"旧版播单","voiceCount":5})],
            false,
        ),
    ]);
    let first = client
        .podcast_page("1", query(0, None), false)
        .await
        .unwrap();
    let purchase = first.purchases.as_ref().unwrap();
    assert_eq!(purchase.next_offset, Some(20));
    assert!(matches!(purchase.rows.as_slice(),
        [PodcastPurchasedRow::Radio(radio), PodcastPurchasedRow::Episode(voice)]
        if radio.id == "81" && voice.id == "81" && voice.radio_id.as_deref() == Some("71")));
    assert!(first.cards.is_empty() && first.episodes.is_empty());
    assert_path(&f, &[BOUGHT, LIKED]);
    for kind in [PurchasedKind::Radio, PurchasedKind::Episode] {
        let result = client
            .podcast_purchased_target("1", &target(&first.revision, kind, "81"))
            .unwrap();
        let value = serde_json::to_value(result.value).unwrap();
        assert_eq!(value["kind"], serde_json::to_value(kind).unwrap());
        assert_eq!(value["item"]["id"], "81");
        assert!(value["item"].get("mainSong").is_none());
    }
    assert!(matches!(
        client
            .podcast_page("1", query(40, Some(&first.revision)), false)
            .await,
        Err(Error::QueueChanged)
    ));
    assert_path(&f, &[BOUGHT, LIKED]);
    let second = client
        .podcast_page("1", query(20, Some(&first.revision)), false)
        .await
        .unwrap();
    let rows = second.purchases.as_ref().unwrap();
    assert_eq!(rows.next_offset, Some(40));
    assert_eq!(rows.rows.len(), 2);
    assert!(matches!(
        client.podcast_purchased_target("1", &rights(&first.revision)),
        Err(Error::QueueChanged)
    ));
    let third = client
        .podcast_page("1", query(40, Some(&second.revision)), false)
        .await
        .unwrap();
    let rows = third.purchases.as_ref().unwrap();
    assert_eq!(rows.next_offset, None);
    assert!(matches!(rows.rows.as_slice(),
        [PodcastPurchasedRow::Radio(radio), PodcastPurchasedRow::Episode(voice), PodcastPurchasedRow::Radio(legacy)]
        if radio.id == "81" && voice.id == "81" && legacy.id == "86"));
    assert_path(&f, &[BOUGHT, LIKED, BOUGHT, BOUGHT]);
    let requests = f
        .api
        .calls()
        .into_iter()
        .filter(|call| call.path == BOUGHT)
        .map(|call| call.payload)
        .collect::<Vec<_>>();
    assert_eq!(
        requests,
        [
            json!({"limit":20,"offset":0}),
            json!({"limit":20,"offset":20}),
            json!({"limit":20,"offset":40})
        ]
    );
}

#[tokio::test]
async fn purchase_status_and_same_id_radio_do_not_grant_download_entitlement() {
    let (f, client) = fixture(vec![
        paid(vec![radio(), voice()], false),
        liked(),
        permission(false),
    ]);
    let page = client
        .podcast_page("1", query(0, None), false)
        .await
        .unwrap();
    assert!(matches!(
        client
            .download_purchased_voice_prepare(
                "1",
                target(&page.revision, PurchasedKind::Radio, "81")
            )
            .await,
        Err(Error::InvalidInput(_))
    ));
    assert_path(&f, &[BOUGHT, LIKED]);
    let plan = client
        .download_purchased_voice_prepare("1", rights(&page.revision))
        .await
        .unwrap();
    assert!(plan.receipt.is_none());
    assert_eq!(serde_json::to_value(plan).unwrap()["state"], "restricted");
    assert_path(&f, &[BOUGHT, LIKED, RIGHTS]);
    assert_eq!(f.api.calls()[2].payload, json!({"voiceIdsStr":["81"]}));
    assert!(f.service.list().unwrap().is_empty());
}

#[tokio::test]
async fn late_program_detail_cannot_commit_a_retired_purchase_revision() {
    let (held, release) = support::gate();
    let options = json!({"code":200,"programs":[{"musics":[{"size":1048576,"bitrate":128000}]}]});
    let (f, client) = fixture(vec![
        paid(vec![radio(), voice()], false),
        liked(),
        permission(true),
        options.into(),
        held,
        paid(vec![radio()], false),
        liked(),
    ]);
    let client = Arc::new(client);
    let page = client
        .podcast_page("1", query(0, None), false)
        .await
        .unwrap();
    let plan = client
        .download_purchased_voice_prepare("1", rights(&page.revision))
        .await
        .unwrap();
    let receipt = plan.receipt.unwrap();
    let confirm = {
        let client = client.clone();
        tokio::spawn(async move {
            let command: DownloadCommand = serde_json::from_value(json!({
                "kind":"confirm_voice","receipt":receipt,"quality":"standard","tags":false,"cover":false
            })).unwrap();
            client.download_command("1", command).await
        })
    };
    f.api.wait_for_calls(5).await;
    let replacement = client
        .podcast_page("1", query(0, None), true)
        .await
        .unwrap();
    assert_ne!(page.revision, replacement.revision);
    let Reply::Ready(Ok(item)) = detail() else {
        unreachable!()
    };
    release.send(Ok(item)).unwrap();
    assert!(matches!(confirm.await.unwrap(), Err(Error::QueueChanged)));
    assert!(f.service.list().unwrap().is_empty());
    assert_path(
        &f,
        &[
            BOUGHT,
            LIKED,
            RIGHTS,
            MUSICS,
            "/api/dj/program/detail",
            BOUGHT,
            LIKED,
        ],
    );
}
