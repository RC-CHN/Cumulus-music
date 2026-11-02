use cumulus_core::{
    account::login::presentation::{LoginDocument, login_countries, valid_phone_number},
    client::MusicClient,
};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct Vector {
    phone: String,
    country: String,
    valid: bool,
}
#[derive(Deserialize)]
struct Vectors {
    cases: Vec<Vector>,
}
#[test]
fn official_country_and_phone_vectors_are_preserved() {
    let table = login_countries();
    assert_eq!(table.len(), 190);
    assert_eq!(table[0].alpha, "CN");
    assert_eq!(table[1].alpha, "HK");
    assert_eq!(table.last().unwrap().alpha, "ZM");
    assert_eq!(
        table
            .iter()
            .map(|row| &row.alpha)
            .collect::<BTreeSet<_>>()
            .len(),
        190
    );
    let vectors: Vectors =
        serde_json::from_str(include_str!("fixtures/login-entry-official.json")).unwrap();
    assert_eq!(vectors.cases.len(), 2475);
    for case in vectors.cases {
        assert_eq!(
            valid_phone_number(&case.phone, &case.country),
            case.valid,
            "official predicate for calling {} and {} digits",
            case.country,
            case.phone.len()
        );
    }
    // Common shared calling codes use the first official entry's strict rule.
    assert!(
        table
            .iter()
            .filter(|row| row.calling == "1")
            .all(|row| row.allowed_lengths == [10])
    );
    assert!(valid_phone_number("1111", "247"));
    assert!(!valid_phone_number(&"1".repeat(33), "247"));
    for country in ["", "+86", "８６", "12345"] {
        assert!(!valid_phone_number("13800000000", country));
    }
}
#[test]
fn selected_region_survives_restart_and_keeps_shared_code_identity() {
    let root = tempfile::tempdir().unwrap();
    {
        let client = MusicClient::open(root.path(), None).unwrap();
        assert_eq!(client.login_presentation().unwrap().selected_country, "CN");
        client.select_login_country("CA").unwrap();
        assert!(client.select_login_country("../US").is_err());
        assert!(client.select_login_country("1").is_err());
    }
    let client = MusicClient::open(root.path(), None).unwrap();
    assert_eq!(client.login_presentation().unwrap().selected_country, "CA");
    assert_ne!(
        client.account().unwrap().status,
        cumulus_core::account::SessionStatus::Authenticated
    );
}
#[test]
fn browser_destinations_are_a_closed_public_set() {
    for (kind, suffix) in [
        ("service", "/service"),
        ("privacy", "/privacy"),
        ("children", "/children"),
        ("email_recovery", "/getpasswd/RetakePassword.jsp"),
    ] {
        let target: LoginDocument = serde_json::from_value(serde_json::json!(kind)).unwrap();
        let url = url::Url::parse(target.url()).unwrap();
        assert_eq!(url.scheme(), "http");
        assert!(url.username().is_empty() && url.password().is_none() && url.query().is_none());
        assert!(url.path().ends_with(suffix));
    }
    assert!(serde_json::from_str::<LoginDocument>("\"https://example.test/?cookie=bad\"").is_err());
}
