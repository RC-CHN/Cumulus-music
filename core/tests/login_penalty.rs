use cumulus_core::{
    Error,
    account::login::penalty::{
        Attempt, MAX_STEP_TEXT_BYTES, MAX_STEPS, MAX_TICKET_BYTES, PenaltyBoundary, PenaltyPhase,
        PenaltyStatusOutcome, PenaltyStepKind, STATUS_PATH,
    },
};
use serde_json::{Value, json};

fn login(steps: Value) -> Value {
    json!({"code":8831,"data":{"steps":steps,"musicTuP":"synthetic-penalty-secret"}})
}

fn status(steps: Value) -> Value {
    json!({"code":200,"data":{"steps":steps}})
}

fn pending() -> Value {
    json!([{"type":"IDENTITY_VERIFY","state":"PENDING"}])
}

#[test]
fn audited_cases_distinguish_entry_dispatch_from_status_continuation() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/penalty-login-cases.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mut attempt = Attempt::from_login_response(&login(case["steps"].clone())).unwrap();
        let entry = serde_json::to_value(attempt.view()).unwrap();
        assert_eq!(
            entry["currentStep"], case["entryCurrent"],
            "{}",
            case["name"]
        );
        assert_eq!(entry["phase"], case["entryPhase"], "{}", case["name"]);
        attempt.begin_status_check().unwrap();
        let outcome = attempt
            .apply_status(&status(case["steps"].clone()))
            .unwrap();
        let expected = match case["statusOutcome"].as_str().unwrap() {
            "identity_pending" => PenaltyStatusOutcome::IdentityPending,
            "next_step" => PenaltyStatusOutcome::NextStep,
            "all_passed" => PenaltyStatusOutcome::AllPassed,
            _ => panic!("invalid synthetic fixture outcome"),
        };
        assert_eq!(outcome, expected, "{}", case["name"]);
        assert_eq!(
            serde_json::to_value(attempt.view()).unwrap()["currentStep"],
            case["statusCurrent"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn strict_codes_and_response_shapes_cannot_create_or_complete_penalty_checks() {
    for code in [json!(200), json!("8831"), json!(null), json!(8831.0)] {
        let mut response = login(pending());
        response["code"] = code;
        assert!(Attempt::from_login_response(&response).is_err());
    }
    for steps in [json!(null), json!({}), json!("PASSED"), json!([null])] {
        assert!(Attempt::from_login_response(&login(steps)).is_err());
    }
    let mut attempt = Attempt::from_login_response(&json!({"body":login(pending())})).unwrap();
    let previous = attempt.view().steps;
    for response in [
        json!({"code":"200","data":{"steps":[]}}),
        json!({"code":200}),
        json!({"code":200,"data":{}}),
        json!({"code":200,"data":{"steps":null}}),
        json!({"body":null}),
    ] {
        attempt.begin_status_check().unwrap();
        assert!(attempt.apply_status(&response).is_err());
        assert_eq!(attempt.view().phase, PenaltyPhase::ReadFailed);
        assert_eq!(attempt.view().steps, previous);
    }
}

#[test]
fn bounds_and_malformed_steps_fail_without_truncating_or_marking_passed() {
    let maximum = vec![json!({"type":"REAL_NAME","state":"PASSED"}); MAX_STEPS];
    assert!(Attempt::from_login_response(&login(json!(maximum))).is_ok());
    let excessive = vec![json!({"type":"REAL_NAME","state":"PASSED"}); MAX_STEPS + 1];
    assert!(Attempt::from_login_response(&login(json!(excessive))).is_err());
    for step in [
        json!({"type":"REAL_NAME"}),
        json!({"state":"PASSED"}),
        json!({"type":"REAL_NAME","state":true}),
        json!({"type":12,"state":"PASSED"}),
        json!({"type":"","state":"PASSED"}),
        json!({"type":"REAL_NAME\n","state":"PASSED"}),
        json!({"type":"REAL_NAME","state":"PASSED\0"}),
        json!({"type":"REAL_NAME","state":"PASSED","minVersion":{}}),
        json!({"type":"x".repeat(MAX_STEP_TEXT_BYTES + 1),"state":"PASSED"}),
        json!({"type":"REAL_NAME","state":"x".repeat(MAX_STEP_TEXT_BYTES + 1)}),
        json!({"type":"REAL_NAME","state":"PASSED","minVersion":"x".repeat(MAX_STEP_TEXT_BYTES + 1)}),
    ] {
        assert!(Attempt::from_login_response(&login(json!([step]))).is_err());
    }
    let exact_text = "x".repeat(MAX_STEP_TEXT_BYTES);
    let attempt = Attempt::from_login_response(&login(json!([
        {"type":exact_text,"state":"WAITING"}
    ])))
    .unwrap();
    assert_eq!(attempt.view().steps[0].kind, PenaltyStepKind::Unknown);
}

#[test]
fn only_exact_passed_is_success_and_versions_retain_the_official_requirement() {
    for state in ["passed", "Passed", "PASSED ", "PENDING", "UNKNOWN"] {
        let attempt = Attempt::from_login_response(&login(json!([
            {"type":"UPGRADE_VERSION","state":state,"minVersion":1782977798999_u64}
        ])))
        .unwrap();
        assert_eq!(attempt.view().phase, PenaltyPhase::UpgradeRequired);
        assert!(!attempt.view().steps[0].passed);
        assert_eq!(
            attempt.view().steps[0].min_version.as_deref(),
            Some("1782977798999")
        );
    }
    let attempt = Attempt::from_login_response(&login(json!([
        {"type":"IDENTITY_VERIFY","state":"PASSED","minVersion":"official-resource-v2"},
        {"type":"UPGRADE_VERSION","state":"PASSED","minVersion":0}
    ])))
    .unwrap();
    assert_eq!(attempt.view().phase, PenaltyPhase::Required);
    assert_eq!(attempt.view().current_step, None);
    assert_eq!(
        attempt.view().steps[0].min_version.as_deref(),
        Some("official-resource-v2")
    );
    assert_eq!(attempt.view().steps[1].min_version, None);
}

#[test]
fn unknown_actions_remain_display_text_and_cannot_select_a_network_destination() {
    let steps = json!([{"type":"FUTURE_SYNTHETIC_ACTION","state":"WAITING"}]);
    let mut attempt = Attempt::from_login_response(&login(steps.clone())).unwrap();
    let request = attempt.begin_status_check().unwrap();
    assert_eq!(request.path, STATUS_PATH);
    assert_eq!(request.method, "POST");
    assert_eq!(request.payload, json!({}));
    assert_eq!(
        attempt.apply_status(&status(steps)).unwrap(),
        PenaltyStatusOutcome::NextStep
    );
    let view = attempt.view();
    assert_eq!(view.boundary, PenaltyBoundary::UnsupportedStep);
    assert_eq!(view.steps[0].kind, PenaltyStepKind::Unknown);
    assert_eq!(view.steps[0].type_name, "FUTURE_SYNTHETIC_ACTION");
    for url in [
        "https://example.test/login?ticket=synthetic-secret",
        "orpheus://private/account-action",
    ] {
        let unsafe_steps = json!([{"type":url,"state":"WAITING"}]);
        assert!(Attempt::from_login_response(&login(unsafe_steps.clone())).is_err());
        let previous = attempt.view().steps;
        attempt.begin_status_check().unwrap();
        assert!(attempt.apply_status(&status(unsafe_steps)).is_err());
        assert_eq!(attempt.view().steps, previous);
        assert!(
            !serde_json::to_string(&attempt.view())
                .unwrap()
                .contains("synthetic-secret")
        );
    }
}

#[test]
fn safe_projection_omits_tickets_credentials_and_original_authorization_routes() {
    let mut response = login(pending());
    response["data"]["password"] = json!("synthetic-password-secret");
    response["data"]["phone"] = json!("13800000000");
    response["data"]["routeUrl"] = json!("orpheus://synthetic-private-route");
    response["header"] = json!({"Set-Cookie":"MUSIC_TU=synthetic-cookie-secret"});
    let mut attempt = Attempt::from_login_response(&response).unwrap();
    let view = serde_json::to_string(&attempt.view()).unwrap();
    for private in [
        "synthetic-penalty-secret",
        "synthetic-password-secret",
        "13800000000",
        "orpheus://synthetic-private-route",
        "synthetic-cookie-secret",
        "musicTuP",
        "routeUrl",
        "Set-Cookie",
    ] {
        assert!(!view.contains(private));
    }
    let request = attempt.begin_status_check().unwrap();
    assert_eq!(request.payload, json!({}));
    for ticket in [
        json!("t".repeat(MAX_TICKET_BYTES + 1)),
        json!("synthetic\r\ninvalid"),
        json!(123),
    ] {
        response["data"]["musicTuP"] = ticket;
        assert!(Attempt::from_login_response(&response).is_err());
    }
    response["data"]["musicTuP"] = json!("t".repeat(MAX_TICKET_BYTES));
    assert!(Attempt::from_login_response(&response).is_ok());
}

#[test]
fn cancelled_and_overlapping_checks_do_not_accept_stale_success() {
    let mut attempt = Attempt::from_login_response(&login(pending())).unwrap();
    assert!(attempt.apply_status(&status(json!([]))).is_err());
    attempt.begin_status_check().unwrap();
    assert!(matches!(
        attempt.begin_status_check(),
        Err(Error::MutationBusy)
    ));
    assert!(!attempt.view().can_check_status);
    attempt.cancel();
    assert_eq!(attempt.view().phase, PenaltyPhase::Cancelled);
    assert_eq!(attempt.view().current_step, None);
    assert!(!attempt.view().can_check_status);
    assert!(matches!(
        attempt.apply_status(&status(json!([]))),
        Err(Error::StaleResponse)
    ));
    assert!(matches!(
        attempt.begin_status_check(),
        Err(Error::StaleResponse)
    ));
    attempt.status_failed();
    assert_eq!(attempt.view().phase, PenaltyPhase::Cancelled);
}

#[test]
fn failed_reads_retain_validated_steps_and_valid_success_only_reports_status() {
    let mut attempt = Attempt::from_login_response(&login(pending())).unwrap();
    attempt.begin_status_check().unwrap();
    assert!(
        attempt
            .apply_status(&json!({"code":500,"data":{"steps":[]}}))
            .is_err()
    );
    assert_eq!(attempt.view().phase, PenaltyPhase::ReadFailed);
    assert!(!attempt.view().steps[0].passed);
    assert_eq!(attempt.view().code, Some(500));
    attempt.begin_status_check().unwrap();
    attempt.status_failed();
    assert_eq!(attempt.view().phase, PenaltyPhase::ReadFailed);
    assert!(attempt.view().can_check_status);
    attempt.begin_status_check().unwrap();
    assert_eq!(
        attempt
            .apply_status(&json!({"body":status(json!([]))}))
            .unwrap(),
        PenaltyStatusOutcome::AllPassed
    );
    assert_eq!(attempt.view().phase, PenaltyPhase::StatusPassed);
    assert_eq!(attempt.view().current_step, None);
    assert!(attempt.view().can_check_status);
}
