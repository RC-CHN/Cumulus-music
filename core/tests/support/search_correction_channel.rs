use super::*;

const CHANNEL: &str = "synthetic_qc_reminder";

fn correction(keyword: &str, channel: Value) -> Value {
    json!({"code":200,"data":{"blocks":[{"blockCode":"search_block_correction","resources":[
        {"baseInfo":{"queryRewriteDTO":{"keyword":"合成","rewriteQuery":"正确合成","searchQcReminder":{
            "qcReminders":[{"qcReminderPart":"仍然搜索 "},{"qcReminderPart":keyword,"highLight":true}],
            "qcReminderChannel":channel
        }}}}
    ]}]}})
}

async fn read_receipt(client: &MusicClient, epoch: &str) -> Option<String> {
    let view = client
        .search_results(epoch, query(C::All))
        .await
        .unwrap()
        .value;
    let output = serde_json::to_string(&view).unwrap();
    assert!(!output.contains(CHANNEL) && !output.contains("qcReminderChannel"));
    view.correction_receipt
}

#[tokio::test]
async fn correction_channel_applies_on_legacy_v3_and_classified_followup() {
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
        *api.reply.lock().unwrap() = correction("合成", json!(CHANNEL));
        let receipt = read_receipt(&client, &epoch).await.unwrap();
        let mut q = query(C::All);
        q.correct = false;
        let view = client
            .search_results_with_context(&epoch, q.clone(), None, Some(&receipt))
            .await
            .unwrap()
            .value;
        assert!(view.correction.is_none() && view.correction_receipt.is_none());
        {
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
            assert_eq!(args["channel"], CHANNEL);
            assert_eq!(args["needCorrect"], false);
            assert_eq!(args["bizQueryInfo"], "");
            assert_eq!(args["keyword"], "合成");
        }
        let payloads: Value =
            serde_json::from_str(include_str!("../fixtures/search-results.json")).unwrap();
        *api.reply.lock().unwrap() = payloads["songs"].clone();
        q.category = C::Songs;
        q.offset = 10;
        client
            .search_results_with_context(&epoch, q.clone(), None, Some(&receipt))
            .await
            .unwrap();
        {
            let calls = api.calls.lock().unwrap();
            let (path, args) = calls.last().unwrap();
            assert_eq!(path, "/api/search/song/list/page");
            assert_eq!(
                (
                    args["channel"].as_str(),
                    args["offset"].as_u64(),
                    args["needCorrect"].as_bool()
                ),
                (Some(CHANNEL), Some(10), Some(false))
            );
        }
        assert_eq!(&*api.checked.lock().unwrap(), &["1", "1"]);
    }
}

#[tokio::test]
async fn correction_channel_rejects_unmatched_untrusted_and_retired_receipts() {
    let (_root, api, session, client) = fixture();
    let epoch = client.account().unwrap().epoch;
    for (word, invalid) in [
        ("合成", json!(null)),
        ("合成", json!("")),
        ("合成", json!("https://private.invalid")),
        ("合成", json!("malformed\nchannel")),
        ("合成", json!("a".repeat(65))),
        ("别的词", json!(CHANNEL)),
    ] {
        *api.reply.lock().unwrap() = correction(word, invalid);
        assert!(read_receipt(&client, &epoch).await.is_none());
    }
    *api.reply.lock().unwrap() = correction("合成", json!(CHANNEL));
    let receipt = read_receipt(&client, &epoch).await.unwrap();
    let mut request = query(C::All);
    request.correct = false;
    let mut wrong_word = request.clone();
    wrong_word.keyword = "别的词".into();
    for (q, id) in [
        (wrong_word, receipt.as_str()),
        (request.clone(), "invalid"),
        (request.clone(), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ] {
        client
            .search_results_with_context(&epoch, q, None, Some(id))
            .await
            .unwrap();
        assert_eq!(
            api.calls.lock().unwrap().last().unwrap().1["channel"],
            "suggest"
        );
    }
    api.revision.store(2, Ordering::SeqCst);
    client
        .search_results_with_context(&epoch, request.clone(), None, Some(&receipt))
        .await
        .unwrap();
    assert_eq!(
        api.calls.lock().unwrap().last().unwrap().1["channel"],
        "suggest"
    );
    let receipt = read_receipt(&client, &epoch).await.unwrap();
    session
        .adopt(api.clone(), json!({"userId":3}), &session.token().unwrap())
        .unwrap();
    let next = client.account().unwrap().epoch;
    client
        .search_results_with_context(&next, request, None, Some(&receipt))
        .await
        .unwrap();
    assert_eq!(
        api.calls.lock().unwrap().last().unwrap().1["channel"],
        "suggest"
    );
    assert!(api.checked.lock().unwrap().is_empty());
}
