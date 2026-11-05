use super::*;

async fn default_rows(client: &MusicClient, epoch: &str) -> Value {
    serde_json::to_value(
        client
            .search_discovery(epoch, SearchDiscoveryQuery::Defaults)
            .await
            .unwrap()
            .value,
    )
    .unwrap()
}

#[tokio::test]
async fn a_default_word_retains_its_exact_context_on_legacy_and_v3_only() {
    for v3 in [false, true] {
        let (root, api, _, mut client) = fixture();
        if v3 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            ProfileRegistry::open(root.path(), ApiOrigin::Pc)
                .unwrap()
                .store()
                .put(
                    "2",
                    "search-experiment-v1",
                    "assignment-v2",
                    &json!({"attempted_at":now,"v3":true}),
                )
                .unwrap();
            client = client.with_official_search_experiment();
        }
        let epoch = client.account().unwrap().epoch;
        *api.reply.lock().unwrap() = json!({"code":200,"data":{"keywords":[
            {"realkeyword":"合成","showKeyword":"第一条","bizQueryInfo":"PRIVATE-FIRST"},
            {"realkeyword":"合成","showKeyword":"第二条","bizQueryInfo":{"token":"PRIVATE-SECOND","rows":[3,2]}},
            {"name":"兼容词","bizQueryInfo":"PRIVATE-NO-REAL-WORD"},
            {"realkeyword":"太长","bizQueryInfo":"a".repeat(5000)}
        ]}});
        let rows = default_rows(&client, &epoch).await;
        assert!(!rows.to_string().contains("PRIVATE"));
        assert!(!rows.to_string().contains("bizQueryInfo"));
        assert_eq!(rows["words"][0]["label"], "第一条");
        assert_eq!(rows["words"][1]["label"], "第二条");
        let receipts = [0, 1].map(|i| rows["words"][i]["defaultReceipt"].as_str().unwrap());
        assert_ne!(receipts[0], receipts[1]);
        assert!(rows["words"][2].get("defaultReceipt").is_none());
        assert!(rows["words"][3].get("defaultReceipt").is_none());
        let payloads: Value =
            serde_json::from_str(include_str!("../fixtures/search-results.json")).unwrap();
        *api.reply.lock().unwrap() = if v3 {
            json!({"code":200,"data":{"blocks":[]}})
        } else {
            payloads["all"].clone()
        };
        let mut request = query(C::All);
        request.channel = Some(Channel::Defaultquery);
        for (receipt, expected) in [
            (receipts[1], json!({"token":"PRIVATE-SECOND","rows":[3,2]})),
            (receipts[0], json!("PRIVATE-FIRST")),
            // Back/retry on the same visit may re-use the exact row.
            (receipts[1], json!({"token":"PRIVATE-SECOND","rows":[3,2]})),
        ] {
            let view = client
                .search_results_with_default(&epoch, request.clone(), Some(receipt))
                .await
                .unwrap()
                .value;
            assert!(!serde_json::to_string(&view).unwrap().contains("PRIVATE"));
            let calls = api.calls.lock().unwrap();
            let (path, args) = calls.last().unwrap();
            assert_eq!(
                path,
                if v3 {
                    "/api/search/pc/complex/page/v3"
                } else {
                    "/api/search/pc/complex/page"
                }
            );
            assert_eq!(args["bizQueryInfo"], expected);
            assert_eq!(args["channel"], "defaultquery");
        }
        request.channel = Some(Channel::Typing);
        client
            .search_results_with_default(&epoch, request.clone(), Some(receipts[0]))
            .await
            .unwrap();
        assert_eq!(
            api.calls.lock().unwrap().last().unwrap().1["bizQueryInfo"],
            ""
        );
        request.channel = Some(Channel::Defaultquery);
        request.keyword = "another".into();
        client
            .search_results_with_default(&epoch, request.clone(), Some(receipts[0]))
            .await
            .unwrap();
        assert_eq!(
            api.calls.lock().unwrap().last().unwrap().1["bizQueryInfo"],
            ""
        );
        *api.reply.lock().unwrap() = payloads["songs"].clone();
        request.category = C::Songs;
        request.keyword = "合成".into();
        client
            .search_results_with_default(&epoch, request, Some(receipts[0]))
            .await
            .unwrap();
        assert!(
            api.calls
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .1
                .get("bizQueryInfo")
                .is_none()
        );
        assert_eq!(*api.checked.lock().unwrap(), ["1", "1", "1"]);
        assert_eq!(
            api.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(path, _)| path == "/api/search/default/keyword/get")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn default_context_expires_on_rotation_account_change_and_during_discovery() {
    let (_root, api, session, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    let defaults =
        json!({"code":200,"data":{"keywords":[{"realkeyword":"合成","bizQueryInfo":"PRIVATE"}]}});
    *api.reply.lock().unwrap() = defaults.clone();
    let old = default_rows(&client, &epoch).await;
    let receipt = old["words"][0]["defaultReceipt"].as_str().unwrap();
    api.revision.store(2, Ordering::SeqCst);
    let mut request = query(C::All);
    request.channel = Some(Channel::Defaultquery);
    let payloads: Value =
        serde_json::from_str(include_str!("../fixtures/search-results.json")).unwrap();
    *api.reply.lock().unwrap() = payloads["all"].clone();
    client
        .search_results_with_default(&epoch, request.clone(), Some(receipt))
        .await
        .unwrap();
    assert_eq!(
        api.calls.lock().unwrap().last().unwrap().1["bizQueryInfo"],
        ""
    );

    *api.reply.lock().unwrap() = defaults;
    api.hold.store(true, Ordering::SeqCst);
    let rotate = async {
        api.started.notified().await;
        api.revision.store(3, Ordering::SeqCst);
        api.release.notify_one();
    };
    let (rows, ()) = tokio::join!(default_rows(&client, &epoch), rotate);
    assert!(rows["words"][0].get("defaultReceipt").is_none());
    api.hold.store(false, Ordering::SeqCst);
    let new = default_rows(&client, &epoch).await;
    let new_receipt = new["words"][0]["defaultReceipt"].as_str().unwrap();
    session
        .adopt(api.clone(), json!({"userId":3}), &session.token().unwrap())
        .unwrap();
    let new_epoch = client.account().unwrap().epoch;
    *api.reply.lock().unwrap() = payloads["all"].clone();
    assert!(
        client
            .search_results_with_default(&epoch, request.clone(), Some(new_receipt))
            .await
            .is_err()
    );
    client
        .search_results_with_default(&new_epoch, request, Some(new_receipt))
        .await
        .unwrap();
    assert_eq!(
        api.calls.lock().unwrap().last().unwrap().1["bizQueryInfo"],
        ""
    );
    assert!(api.checked.lock().unwrap().is_empty());
}
