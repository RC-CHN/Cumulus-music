use super::*;
use cumulus_core::{
    catalog::search::{SearchDiscovery, SearchDiscoveryQuery, results::SearchChannel},
    client::dto::{SearchCategory, SearchHistoryCommand, SearchResultQuery},
};

const DEFAULTS: &str = "/api/search/default/keyword/get";
const COMPLEX: &str = "/api/search/pc/complex/page";
const SONGS: &str = "/api/search/song/list/page";
const MUSIC_A: &str = "MUSIC_A=synthetic-guest-search; Path=/; Secure";

fn registration(cookies: &[&str]) -> HttpResponse {
    let mut result = reply(json!({"code":200,"userId":9001}));
    result.set_cookies = cookies.iter().map(|value| (*value).into()).collect();
    HttpResponse::Ready(Ok(result))
}

fn query(category: SearchCategory, channel: SearchChannel) -> SearchResultQuery {
    SearchResultQuery {
        keyword: "合成游客搜索".into(),
        category,
        offset: 0,
        limit: 20,
        correct: true,
        channel: Some(channel),
    }
}

async fn start(f: &Fixture) -> cumulus_core::client::pc_experiments::PcPresentationRuntimeLease {
    let lease = f.client.start_pc_presentation_runtime().unwrap();
    assert!(f.session.refresh().await.unwrap_err().is_service_code(301));
    status(
        &mut lease.subscribe_status(),
        PcPresentationRuntimeStatus::Ready,
    )
    .await;
    lease
}

#[tokio::test]
async fn registered_guest_search_reads_native_cookie_and_default_context_without_account_rights() {
    let mut complex = reply(json!({"code":200,"data":{"blocks":[
        {"blockCode":"search_block_song","resources":[{"type":"song","baseInfo":{"id":42,"name":"合成单曲"}}]},
        {"blockCode":"search_block_playlist","resources":[{"type":"playlist","baseInfo":{"id":99,"userId":71,"name":"不可见私有歌单","privacy":10}}]}
    ]}}));
    complex.set_cookies = vec!["MUSIC_U=forged-search-cookie; Path=/; Secure".into()];
    let f = Fixture::new(false, [
        json!({"code":301}).into(), registration(&[MUSIC_A, COOKIE]), groups(true).into(),
        json!({"code":200,"data":{"keywords":[{"realkeyword":"合成游客搜索","showKeyword":"合成占位","bizQueryInfo":"native-guest-context"}]}}).into(),
            HttpResponse::Ready(Ok(complex)),
        json!({"code":200,"data":{"totalCount":1,"resources":[{"baseInfo":{"simpleSongData":{"id":42,"name":"合成单曲"}}}]}}).into(),
        json!({"code":301}).into(),
    ]);
    assert!(!f.client.pc_presentation().unwrap().guest_search_ready);
    assert!(
        f.client
            .search_discovery(&f.epoch(), SearchDiscoveryQuery::Defaults)
            .await
            .is_err()
    );
    assert!(f.paths().is_empty());
    let lease = start(&f).await;
    let epoch = f.epoch();
    assert!(f.client.pc_presentation().unwrap().guest_search_ready);
    // A restore while on the anonymous adapter must not query or adopt a profile.
    assert!(f.session.refresh().await.unwrap_err().is_service_code(301));
    let SearchDiscovery::Defaults { words, .. } = f
        .client
        .search_discovery(&epoch, SearchDiscoveryQuery::Defaults)
        .await
        .unwrap()
        .value
    else {
        panic!("expected default words")
    };
    let receipt = words[0].default_receipt.as_deref().unwrap();
    let all = f
        .client
        .search_results_with_default(
            &epoch,
            query(SearchCategory::All, SearchChannel::Defaultquery),
            Some(receipt),
        )
        .await
        .unwrap()
        .value;
    assert_eq!(all.sections[0].items.len(), 1);
    assert_eq!(
        all.sections.len(),
        1,
        "private playlist did not enter the catalog"
    );
    assert!(all.presentation_receipt.is_none() && all.privilege_receipt.is_none());
    let songs = f
        .client
        .search_results(&epoch, query(SearchCategory::Songs, SearchChannel::Typing))
        .await
        .unwrap()
        .value;
    let serialized = serde_json::to_string(&songs).unwrap();
    assert!(serialized.contains("合成单曲"));
    assert!(!serialized.contains("queueDropReceipt"));
    let history = f
        .client
        .search_history(
            &epoch,
            SearchHistoryCommand::Remember {
                keyword: "合成游客搜索".into(),
            },
        )
        .unwrap();
    assert_eq!(history.value, ["合成游客搜索"]);
    let rejected = f
        .client
        .search_results(&epoch, query(SearchCategory::All, SearchChannel::Typing))
        .await;
    assert!(matches!(
        rejected,
        Err(Error::State("registered guest search is unavailable"))
    ));
    assert!(
        f.client
            .search_discovery(
                &epoch,
                SearchDiscoveryQuery::ChartSongs {
                    keywords: vec!["不可播放".into()]
                }
            )
            .await
            .is_err()
    );
    assert!(f.session.token().unwrap().uid.is_none());
    assert_eq!(f.session.snapshot().unwrap().status, SessionStatus::Unknown);
    assert_eq!(
        f.paths(),
        [
            ACCOUNT,
            ANONYMOUS_PATH,
            ASSIGNMENT_PATH,
            DEFAULTS,
            COMPLEX,
            SONGS,
            COMPLEX
        ]
    );
    let calls = f.http.calls();
    for call in &calls[3..] {
        let cookie = call.cookie.as_deref().unwrap();
        assert!(cookie.contains("MUSIC_A=synthetic-guest-search"));
        assert!(!cookie.contains("MUSIC_U"));
    }
    assert_eq!(calls[4].payload["bizQueryInfo"], "native-guest-context");
    assert_eq!(calls[4].payload["channel"], "defaultquery");
    assert_eq!(calls[6].payload["bizQueryInfo"], "");
    assert_eq!(calls[6].payload["channel"], "typing");
    lease.shutdown().await.unwrap();
    f.session
        .adopt(
            f.native.clone(),
            json!({"userId":71}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    assert!(
        f.client
            .search_history(&f.epoch(), SearchHistoryCommand::Read)
            .unwrap()
            .value
            .is_empty()
    );
    assert!(!f.client.pc_presentation().unwrap().guest_search_ready);
}

#[tokio::test]
async fn presentation_guest_cookie_does_not_establish_exact_search_scope() {
    for cookies in [
        vec!["NMTID=not-anonymous; Path=/; Secure"],
        vec!["MUSIC_A=wrong-path; Path=/eapi/search/search-only; Secure"],
        vec![MUSIC_A, "MUSIC_A=ambiguous; Path=/eapi/search; Secure"],
    ] {
        let f = Fixture::new(
            false,
            [
                json!({"code":301}).into(),
                registration(&cookies),
                groups(false).into(),
            ],
        );
        let lease = start(&f).await;
        assert!(!f.client.pc_presentation().unwrap().guest_search_ready);
        assert!(
            f.client
                .search_results(
                    &f.epoch(),
                    query(SearchCategory::All, SearchChannel::Typing)
                )
                .await
                .is_err()
        );
        assert_eq!(f.paths(), [ACCOUNT, ANONYMOUS_PATH, ASSIGNMENT_PATH]);
        lease.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn late_guest_search_cannot_publish_after_account_adoption() {
    let (held, reply_later) = gate();
    let f = Fixture::new(
        false,
        [
            json!({"code":301}).into(),
            registration(&[MUSIC_A]),
            groups(false).into(),
            held,
        ],
    );
    let lease = start(&f).await;
    lease.shutdown().await.unwrap();
    let client = f.client.clone();
    let epoch = f.epoch();
    let search = tokio::spawn(async move {
        client
            .search_results(&epoch, query(SearchCategory::All, SearchChannel::Typing))
            .await
    });
    f.http.wait(4).await;
    f.session
        .adopt(
            f.native.clone(),
            json!({"userId":71}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let target = ApiOrigin::Pc.url();
    let revision = f.credentials.request_credentials(&target).unwrap().revision;
    f.credentials
        .record_response(&target, &[COOKIE.into()], None, &revision)
        .unwrap();
    let mut late = reply(json!({"code":200,"data":{"blocks":[]}}));
    late.set_cookies = vec![
        "MUSIC_U=foreign-guest-reply; Path=/; Secure".into(),
        "MUSIC_A=late-guest-rotation; Path=/; Secure".into(),
    ];
    assert!(reply_later.send(Ok(late)).is_ok());
    assert!(matches!(search.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(f.session.token().unwrap().uid.as_deref(), Some("71"));
    assert_eq!(
        f.credentials.login_credentials().unwrap().token.as_deref(),
        Some("synthetic-presentation")
    );
    let guest = Credentials::open(
        Arc::new(
            cumulus_core::storage::StateStore::open(f.root.path().join("host/guest")).unwrap(),
        ),
        ApiOrigin::Pc,
    )
    .unwrap();
    let cookies = guest.request_credentials(&target).unwrap().cookies;
    assert_eq!(
        cookies,
        [("MUSIC_A".into(), "synthetic-guest-search".into())]
    );
    assert_eq!(
        f.paths(),
        [ACCOUNT, ANONYMOUS_PATH, ASSIGNMENT_PATH, COMPLEX]
    );
}
