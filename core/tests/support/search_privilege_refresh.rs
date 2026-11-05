use super::*;

fn comprehensive() -> Value {
    let track = |id| {
        json!({"type":"song","baseInfo":{"id":id,"name":"合成歌曲",
        "mv":600+id,"privilege":{"st":-200},"cookie":"PRIVATE_SEARCH"}})
    };
    json!({"code":200,"data":{"blocks":[
        {"blockCode":"search_block_best_match","resources":[track(90)]},
        {"blockCode":"search_block_song","resources":(1..=7).map(track).collect::<Vec<_>>()},
        {"blockCode":"search_block_song_rec","resources":[track(80)]}
    ]}})
}

fn member(now: u64, level: u8, active: bool) -> Value {
    json!({"code":200,"data":{"userId":2,"now":now,
        "associator":{"expireTime":if active {99_999_999} else {0},"vipLevel":level}}})
}

fn privilege() -> Value {
    json!({"code":200,"data":[
        {"id":2,"st":-200,"fee":1},
        {"id":99,"st":0,"fee":0},
        {"id":"2","st":"0","fee":"32","freeTrialPrivilege":{
            "userConsumable":true,"resConsumable":true,"freeLimitTagType":"VIP_AND_FREE_TAG"
        },"cookie":"PRIVATE_REPLY"},
        {"id":1,"st":-200,"fee":16}
    ]})
}

#[tokio::test]
async fn comprehensive_observation_and_request_use_only_the_native_six_song_rows() {
    let (_root, api, _session, client) = fixture();
    let mut observations = client.subscribe_membership_observations().unwrap();
    *api.reply.lock().unwrap() = comprehensive();
    let current = client
        .search_results("1", query(C::All))
        .await
        .unwrap()
        .value;
    let receipt = current.privilege_receipt.unwrap();
    let view = serde_json::to_value(current.sections).unwrap();
    assert_eq!(view[1]["items"].as_array().unwrap().len(), 7);
    assert!(view[1]["items"][1].get("mvId").is_none());

    *api.reply.lock().unwrap() = member(1_000_000, 5, false);
    client.member_rights("1", true).await.unwrap();
    let first = observations.borrow_and_update().clone().unwrap();
    assert_eq!(first.epoch, "1");
    *api.reply.lock().unwrap() = member(1_000_010, 6, false);
    client.member_rights("1", true).await.unwrap();
    assert!(
        !observations.has_changed().unwrap(),
        "level/timestamp alone is not the tuple"
    );
    *api.reply.lock().unwrap() = member(1_000_020, 6, true);
    client.member_rights("1", true).await.unwrap();
    assert!(observations.has_changed().unwrap());
    let changed = observations.borrow_and_update().clone().unwrap();
    assert_ne!(first.revision, changed.revision);

    *api.reply.lock().unwrap() = privilege();
    assert!(matches!(
        client
            .search_refresh_privileges("1", &receipt, Some(&first.revision))
            .await,
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        client
            .search_refresh_privileges("1", "bad receipt", None)
            .await,
        Err(Error::InvalidInput(_))
    ));
    let fresh = client
        .search_refresh_privileges("1", &receipt, Some(&changed.revision))
        .await
        .unwrap();
    let value = serde_json::to_value(fresh.value).unwrap();
    assert_eq!(value["receipt"], receipt);
    assert_eq!(value["section"], "search_block_song:1");
    assert_eq!(
        value["rows"],
        json!([
            {"id":"1","coverUrl":null,"mvId":null,"replacementMv":false,"vipBadge":true,"limitedFree":false},
            {"id":"2","coverUrl":null,"mvId":"602","replacementMv":false,"vipBadge":true,"limitedFree":true}
        ])
    );
    assert!(!value.to_string().contains("PRIVATE"));
    let calls = api.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|(path, _)| path == "/api/song/enhance/privilege")
            .collect::<Vec<_>>(),
        vec![&(
            "/api/song/enhance/privilege".into(),
            json!({"ids":"[\"1\",\"2\",\"3\",\"4\",\"5\",\"6\"]"})
        )]
    );
}

#[tokio::test]
async fn comprehensive_privilege_refresh_rejects_old_account_credentials_and_entitlement() {
    for scenario in ["account", "credentials", "observation"] {
        let (_root, api, session, client) = fixture();
        *api.reply.lock().unwrap() = comprehensive();
        let receipt = client
            .search_results("1", query(C::All))
            .await
            .unwrap()
            .value
            .privilege_receipt
            .unwrap();
        let client = Arc::new(client);
        *api.reply.lock().unwrap() = privilege();
        api.hold.store(true, Ordering::SeqCst);
        let task = {
            let client = client.clone();
            tokio::spawn(async move { client.search_refresh_privileges("1", &receipt, None).await })
        };
        tokio::time::timeout(std::time::Duration::from_secs(4), api.started.notified())
            .await
            .unwrap();
        match scenario {
            "account" => {
                session
                    .adopt(api.clone(), json!({"userId":3}), &session.token().unwrap())
                    .unwrap();
            }
            "credentials" => api.revision.store(2, Ordering::SeqCst),
            "observation" => {
                api.hold.store(false, Ordering::SeqCst);
                *api.reply.lock().unwrap() = member(1_000_020, 6, true);
                client.member_rights("1", true).await.unwrap();
                *api.reply.lock().unwrap() = privilege();
            }
            _ => unreachable!(),
        }
        api.release.notify_one();
        assert!(
            matches!(task.await.unwrap(), Err(Error::StaleResponse)),
            "{scenario}"
        );
    }
}

#[tokio::test]
async fn an_empty_first_ordinary_block_does_not_borrow_a_later_blocks_songs() {
    let (_root, api, _session, client) = fixture();
    let mut response = comprehensive();
    response["data"]["blocks"]
        .as_array_mut()
        .unwrap()
        .insert(1, json!({"blockCode":"search_block_song","resources":[]}));
    *api.reply.lock().unwrap() = response;
    let view = client
        .search_results("1", query(C::All))
        .await
        .unwrap()
        .value;
    assert!(
        view.sections
            .iter()
            .any(|section| section.id == "search_block_song:2")
    );
    assert!(view.privilege_receipt.is_none());
}
