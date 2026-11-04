use super::*;
use crate::{catalog::vertical_zone::ZoneTab, protocol::api::ApiFuture};
use serde_json::Value;

struct Sealed;
impl Api for Sealed {
    fn request<'a>(&'a self, _: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        unreachable!("tab-cache proof must not touch a transport")
    }
}

fn owner(generation: u64) -> SessionToken {
    SessionToken {
        generation,
        uid: Some("10".into()),
    }
}

fn page(key: &str, name: &str) -> VerticalZonePage {
    let tab = ZoneTab {
        key: key.into(),
        name: name.into(),
    };
    VerticalZonePage {
        layout: ZoneLayout::Tabs,
        tabs: vec![tab.clone()],
        active_tab: Some(tab),
        blocks: Vec::new(),
    }
}

#[test]
fn reused_tabs_fence_account_generation_transport_privacy_and_route_order() {
    let host = ZoneHost::default();
    let api: Arc<dyn Api> = Arc::new(Sealed);
    let alternative: Arc<dyn Api> = Arc::new(Sealed);
    let first = host.enter(&owner(1), &api).unwrap();
    let later = host.enter(&owner(1), &api).unwrap();
    let item = page("default", " 热门");
    assert!(matches!(
        host.finish(first, "jazz", true, &item),
        Err(Error::StaleResponse)
    ));
    host.finish(later, "jazz", true, &item).unwrap();
    assert!(
        host.load(later, "jazz", &ZoneEntry::SearchTabName("热门"), true, true)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        host.load(
            later,
            "jazz",
            &ZoneEntry::SearchTabName(" 热门"),
            true,
            true
        )
        .unwrap(),
        Some(item.clone())
    );
    assert!(
        host.load(later, "jazz", &ZoneEntry::Tab(""), false, true)
            .unwrap()
            .is_none()
    );
    let replaced = host.enter(&owner(1), &alternative).unwrap();
    assert!(
        host.load(replaced, "jazz", &ZoneEntry::Tab(""), true, true)
            .unwrap()
            .is_none()
    );
    host.finish(replaced, "jazz", true, &item).unwrap();
    let same_uid_new_generation = host.enter(&owner(2), &alternative).unwrap();
    assert!(
        host.load(
            same_uid_new_generation,
            "jazz",
            &ZoneEntry::Tab(""),
            true,
            true
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn eight_tabs_refresh_recency_and_classic_disables_cache() {
    let host = ZoneHost::default();
    let api: Arc<dyn Api> = Arc::new(Sealed);
    for key in 0..8 {
        let sequence = host.enter(&owner(1), &api).unwrap();
        host.finish(sequence, "jazz", true, &page(&key.to_string(), "标签"))
            .unwrap();
    }
    let sequence = host.enter(&owner(1), &api).unwrap();
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("0"), true, true)
            .unwrap()
            .is_some()
    );
    host.finish(sequence, "jazz", true, &page("new", "新标签"))
        .unwrap();
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("1"), true, true)
            .unwrap()
            .is_none()
    );
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("0"), true, true)
            .unwrap()
            .is_some()
    );
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("0"), true, false)
            .unwrap()
            .is_none()
    );
    assert!(
        host.load(sequence, "jazz", &ZoneEntry::Tab("0"), true, true)
            .unwrap()
            .is_none()
    );
}
