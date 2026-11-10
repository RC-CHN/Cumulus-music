//! Sealed signed PC catalogue reads and presentation ownership.
use super::*;
use cumulus_core::protocol::pc_experiments::PcExperimentGroups;

const PROMOTE: &str = "/api/usertool/sound/mobile/promote";
const ALL: &str = "/api/usertool/sound/mobile/all";
const MODERN: &str = "/api/usertool/sound/queryAllSound";
const EFFECT: &str = "PH-PC-yinxiaogaiban2";
const MINIBAR: &str = "PH-PC-minibarNew";
const LIKE: &str = "PH-PC-minibarNewLike";

fn assignment(flags: &[(&str, &str)]) -> Value {
    json!({"code":200,"data":flags.iter().map(|(name, group)| json!({"expName":name,"expGroupName":group})).collect::<Vec<_>>()})
}

fn catalogue(ids: &[u64]) -> Value {
    json!({"code":200,"data":ids.iter().map(|id| json!({
        "id":id,"name":format!("音效{id}"),"subTitle":"可显示的说明","type":if *id == 3 {3} else {1},
        "newAdd":if *id == 2 {1} else {0},"imgUrl":"https://p1.music.126.net/private-asset",
        "link":"https://example.invalid/private-material","md5":"private-digest",
        "vipAuthRespItem":{"interestsVipType":15,"authResult":{"private":"value"}}
    })).collect::<Vec<_>>()})
}

fn ids(group: &Value) -> Vec<u64> {
    group["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_u64().unwrap())
        .collect()
}

async fn overview(f: &Fixture) -> Value {
    serde_json::to_value(
        f.client
            .effect_directory_overview(&f.epoch())
            .await
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn complete_audio_page_truth_table() {
    let labels = [None, Some(""), Some("c"), Some("t1"), Some("t2")];
    for effect in labels {
        for minibar in labels {
            for like in labels {
                let flags: Vec<_> = [(EFFECT, effect), (MINIBAR, minibar), (LIKE, like)]
                    .into_iter()
                    .filter_map(|(name, group)| group.map(|group| (name, group)))
                    .collect();
                let groups = PcExperimentGroups::refreshed(&assignment(&flags)).unwrap();
                let expected = matches!(
                    (effect, minibar, like),
                    (Some("t1"), Some("t1"), _) | (Some("t1"), Some("c"), Some("t1"))
                );
                assert_eq!(
                    groups.effect_catalogue_new_assigned(),
                    expected,
                    "{flags:?}"
                );
            }
        }
    }
}

#[tokio::test]
async fn default_legacy_uses_exact_endpoints_and_preserves_order_and_empty_semantics() {
    let f = Fixture::new(
        true,
        [
            catalogue(&[1, 2, 3, 4, 5, 6, 7, 8]).into(),
            catalogue(&[8, 3, 99, 7, 1, 10, 10]).into(),
            catalogue(&[51]).into(),
            catalogue(&[]).into(),
            catalogue(&[52]).into(),
            json!({"code":503}).into(),
            json!({"code":503}).into(),
            catalogue(&[53, 54]).into(),
        ],
    );
    let initial = overview(&f).await;
    assert_eq!(initial["layout"], "legacy");
    assert_eq!(initial["groups"][0]["key"], "legacy_recommended");
    assert_eq!(initial["groups"][1]["key"], "legacy_all");
    assert_eq!(ids(&initial["groups"][0]), [1, 2, 3, 4, 5, 6]);
    assert_eq!(ids(&initial["groups"][1]), [8, 99, 7, 10, 10]);
    assert_eq!(initial["groups"][0]["items"][1]["newly_added"], true);
    assert_eq!(initial["groups"][0]["items"][2]["membership"], "vip");
    assert!(!initial.to_string().contains("private"));
    for _ in 0..2 {
        let empty = overview(&f).await;
        assert_eq!(empty["partial"], false);
        assert!(ids(&empty["groups"][0]).is_empty() && ids(&empty["groups"][1]).is_empty());
    }
    let no_recommendation = overview(&f).await;
    assert!(ids(&no_recommendation["groups"][0]).is_empty());
    assert_eq!(ids(&no_recommendation["groups"][1]), [53, 54]);
    assert_eq!(
        f.paths(),
        [PROMOTE, ALL, PROMOTE, ALL, PROMOTE, ALL, PROMOTE, ALL]
    );
    for call in f.http.calls() {
        let mut args = call.payload;
        let fields = args.as_object_mut().unwrap();
        fields.remove("header");
        fields.remove("e_r");
        assert!(
            fields.is_empty(),
            "{} must send empty business arguments",
            call.path
        );
        assert!(
            call.cookie
                .is_some_and(|cookie| cookie.contains("MUSIC_U=synthetic-presentation"))
        );
    }
}

#[tokio::test(start_paused = true)]
async fn live_modern_transition_fences_held_legacy_and_unrelated_flags_keep_cohort_revision() {
    let (held, release) = gate();
    let assigned = [(EFFECT, "t1"), (MINIBAR, "c"), (LIKE, "t1")];
    let mut unrelated = assigned.to_vec();
    unrelated.push(("PH-PC-search-result", "t1"));
    let f = Fixture::new(
        true,
        [
            held,
            assignment(&assigned).into(),
            catalogue(&[1, 2]).into(),
            catalogue(&[3, 4, 5, 6, 7, 8]).into(),
            catalogue(&[9]).into(),
            assignment(&unrelated).into(),
            catalogue(&[1]).into(),
            json!({"code":503}).into(),
            catalogue(&[10]).into(),
        ],
    );
    let before = f.client.pc_presentation().unwrap();
    assert!(!before.effect_catalogue_new);
    let client = f.client.clone();
    let epoch = f.epoch();
    let pending = tokio::spawn(async move { client.effect_directory_overview(&epoch).await });
    f.http.wait(1).await;
    assert_eq!(f.paths(), [PROMOTE]);
    // The same native credential gate serializes transport. Queue an
    // assignment before releasing promote so it wins over a queued all read.
    let owner = f.client.clone();
    let refresh = tokio::spawn(async move { owner.refresh_pc_presentation().await });
    tokio::task::yield_now().await;
    assert_eq!(f.paths(), [PROMOTE]);
    assert!(release.send(Ok(reply(catalogue(&[82])))).is_ok());
    let modern = refresh.await.unwrap().unwrap();
    assert!(modern.effect_catalogue_new);
    assert_ne!(
        modern.effect_catalogue_revision,
        before.effect_catalogue_revision
    );
    assert!(matches!(pending.await.unwrap(), Err(Error::StaleResponse)));
    assert_eq!(f.paths(), [PROMOTE, ASSIGNMENT_PATH]);
    let projected = overview(&f).await;
    assert_eq!(
        projected["cohortRevision"],
        modern.effect_catalogue_revision
    );
    assert_eq!(projected["layout"], "modern");
    assert_eq!(ids(&projected["groups"][0]), [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(ids(&projected["groups"][1]), [9]);
    tokio::time::advance(Duration::from_millis(1)).await;
    let unrelated = f.client.refresh_pc_presentation().await.unwrap();
    assert_ne!(modern.revision, unrelated.revision);
    assert_eq!(
        modern.effect_catalogue_revision,
        unrelated.effect_catalogue_revision
    );
    let partial = overview(&f).await;
    assert_eq!(partial["partial"], true);
    assert_eq!(partial["groups"].as_array().unwrap().len(), 1);
    assert_eq!(partial["groups"][0]["key"], "more");
    assert_eq!(ids(&partial["groups"][0]), [10]);
    let calls = f.http.calls();
    assert_eq!(
        f.paths(),
        [
            PROMOTE,
            ASSIGNMENT_PATH,
            MODERN,
            MODERN,
            MODERN,
            ASSIGNMENT_PATH,
            MODERN,
            MODERN,
            MODERN
        ]
    );
    assert_batch(&f.http);
    for batch in [&calls[2..5], &calls[6..9]] {
        assert_eq!(
            batch
                .iter()
                .map(|call| call.payload["exposeType"].clone())
                .collect::<Vec<_>>(),
            [json!("5"), json!("6"), json!("7")]
        );
    }
}

#[tokio::test]
async fn rotated_credential_discards_inflight_overview_and_same_user_cache_never_crosses_owner() {
    let (held, release) = gate();
    let f = Fixture::new(
        true,
        [
            assignment(&[(EFFECT, "t1"), (MINIBAR, "t1")]).into(),
            held,
            catalogue(&[31]).into(),
            catalogue(&[31, 32]).into(),
        ],
    );
    let cohort = f.client.refresh_pc_presentation().await.unwrap();
    assert!(cohort.effect_catalogue_new);
    let reopened = MusicClient::with_session(f.session.clone(), f.registry.clone());
    assert!(reopened.pc_presentation().unwrap().effect_catalogue_new);
    let old_epoch = f.epoch();
    let old_client = f.client.clone();
    let pending =
        tokio::spawn(async move { old_client.effect_directory_overview(&old_epoch).await });
    f.http.wait(2).await;
    let target = ApiOrigin::Pc.url();
    let old = f.credentials.request_credentials(&target).unwrap();
    f.credentials
        .record_response(
            &target,
            &["MUSIC_U=synthetic-presentation-rotated; Path=/; Secure".into()],
            None,
            &old.revision,
        )
        .unwrap();
    assert!(release.send(Ok(reply(catalogue(&[21])))).is_ok());
    assert!(matches!(
        pending.await.unwrap(),
        Err(Error::StaleResponse | Error::CredentialChanged)
    ));
    assert_eq!(
        f.paths(),
        [ASSIGNMENT_PATH, MODERN],
        "queued modern siblings cannot dispatch under rotated credentials"
    );
    let reset = f.client.pc_presentation().unwrap();
    assert!(!reset.effect_catalogue_new);
    assert_ne!(
        reset.effect_catalogue_revision,
        cohort.effect_catalogue_revision
    );
    f.session
        .adopt(
            f.native.clone(),
            json!({"userId":72}),
            &f.session.token().unwrap(),
        )
        .unwrap();
    let other = overview(&f).await;
    assert_eq!(other["layout"], "legacy");
    assert_eq!(ids(&other["groups"][0]), [31]);
    assert_eq!(ids(&other["groups"][1]), [32]);
    assert_eq!(f.paths(), [ASSIGNMENT_PATH, MODERN, PROMOTE, ALL]);
}
