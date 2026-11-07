use super::{PlayerOrigin, PlayerResourceKind, QueueOrigin, RecommendationOrigin, identity};
use crate::{
    Error, Result,
    account::SessionToken,
    catalog::details::{CatalogDetails, CatalogQuery},
    playback::QueueEntry,
    protocol::api::identifier,
};
use serde_json::Value;

fn label(name: &str) -> Option<&str> {
    (!name.trim().is_empty() && name.len() <= 4096).then_some(name)
}

/// The display label comes from the source object itself, never its song rows.
pub(in crate::client) fn resource_name<'a>(resource: &'a Value, id: &str) -> Option<&'a str> {
    (identifier(&resource["id"]).ok()? == id)
        .then(|| resource["name"].as_str().and_then(label))
        .flatten()
}

pub(in crate::client) fn album_name<'a>(album: &'a Value, id: &str) -> Option<&'a str> {
    (crate::catalog::album_identifier(album).ok()? == id)
        .then(|| album["name"].as_str().and_then(label))
        .flatten()
}

pub(in crate::client) fn named(mut source: Value, name: Option<&str>) -> Value {
    if let Some(name) = name.and_then(label) {
        source["source_name"] = Value::from(name);
    }
    source
}

pub(in crate::client) fn cached_artist_name(
    catalog: &CatalogDetails,
    token: &SessionToken,
    id: &str,
) -> Result<Option<String>> {
    match catalog.cached_for(token, &CatalogQuery::Artist { id: id.to_owned() }) {
        Ok(page) => Ok(page
            .as_ref()
            .and_then(|page| resource_name(&page.response["artist"], id))
            .map(str::to_owned)),
        Err(Error::StaleResponse) => Err(Error::StaleResponse),
        // Optional display metadata must not block a playable selection.
        Err(_) => Ok(None),
    }
}

pub(super) fn display_name(entry: &QueueEntry) -> Option<&str> {
    let name = entry.source["source_name"].as_str().and_then(label)?;
    if identity::kind(&entry.resource) != PlayerResourceKind::Catalog
        || !entry.resource["voice"].is_null()
        || !entry.resource["song"]["voice_id"].is_null()
        || !entry.resource["download_voice_id"].is_null()
        || !matches!(
            project(&entry.source)?,
            PlayerOrigin::Album { .. }
                | PlayerOrigin::Artist { .. }
                | PlayerOrigin::Playlist { .. }
        )
    {
        return None;
    }
    Some(name)
}

pub(super) fn project(source: &Value) -> Option<PlayerOrigin> {
    let id = || identifier(&source["id"]).ok();
    Some(match source["kind"].as_str()? {
        "playlist" => PlayerOrigin::Playlist { id: id()? },
        "album" => PlayerOrigin::Album { id: id()? },
        "artist" => PlayerOrigin::Artist { id: id()? },
        "listened_book" | "podcast_subscription_recommendation" | "search_podcast_card" => {
            PlayerOrigin::Podcast { id: id()? }
        }
        "podcast" if source["scope"]["kind"] == "radio" => PlayerOrigin::Podcast {
            id: identifier(&source["scope"]["id"]).ok()?,
        },
        _ => return None,
    })
}

/// Official queue provenance is the source of this row, independent of
/// the loaded entry and the song's artist/album metadata. Only ordinary
/// typed destinations are projected; never forward URL, path or toState.
pub(super) fn queue(entry: &crate::playback::QueueEntry) -> Option<QueueOrigin> {
    use PlayerResourceKind as Kind;
    if entry.resource.get("voice_id").is_some() {
        return None;
    }
    let source = &entry.source;
    let id = || identifier(&source["id"]).ok();
    Some(
        match (source["kind"].as_str()?, identity::kind(&entry.resource)) {
            ("album", Kind::Catalog) => QueueOrigin::Album { id: id()? },
            ("artist", Kind::Catalog) => QueueOrigin::Artist { id: id()? },
            ("playlist", Kind::Catalog) => QueueOrigin::Playlist { id: id()? },
            ("local", Kind::Local) if source["nativeOpen"] != true => QueueOrigin::Local,
            ("cloud", Kind::Cloud) => QueueOrigin::Cloud,
            ("downloads", Kind::Downloaded) => QueueOrigin::Downloads,
            _ => return None,
        },
    )
}

/// Only the native selected catalog entry can supply the search label.
/// Do not project the original query, context, or a fabricated destination.
pub(super) fn search(entry: &crate::playback::QueueEntry) -> bool {
    identity::kind(&entry.resource) == PlayerResourceKind::Catalog
        && entry.source["kind"] == "songs"
        && entry.source["scene"] == "search"
}

pub(super) fn recommendation(source: &Value) -> Option<RecommendationOrigin> {
    // A Daily double-click in keep-queue mode has Daily provenance and trial
    // mode, but it did not select the recommendation collection.
    if source["single_row"] == true {
        return None;
    }
    let revision = source["revision"].as_str()?;
    let (prefix, id) = revision.rsplit_once('-')?;
    uuid::Uuid::parse_str(id).ok()?;
    let (kind, request) = prefix.rsplit_once('-')?;
    request.parse::<u64>().ok()?;
    match (source["kind"].as_str()?, kind) {
        ("daily_recommendations", "daily") => Some(RecommendationOrigin::Daily {
            revision: revision.to_owned(),
        }),
        ("style_recommendations", "style" | "seeded-style") => Some(RecommendationOrigin::Style {
            revision: revision.to_owned(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn named_origins_use_only_the_matched_source_and_never_infer_legacy_labels() {
        let raw = json!({"id":7,"name":"  真正的来源  ","href":"PRIVATE","token":"PRIVATE"});
        assert_eq!(resource_name(&raw, "8"), None);
        assert_eq!(resource_name(&raw, "7"), Some("  真正的来源  "));
        assert_eq!(album_name(&raw, "7"), Some("  真正的来源  "));
        for kind in ["album", "artist", "playlist"] {
            let mut entry = QueueEntry {
                entry_id: "entry".into(),
                resource: json!({"id":"1","song":{"album":{"id":7,"name":"曲目标记"}}}),
                source: json!({"kind":kind,"id":"7","history_resource":{"id":"7","name":"旧对象"}}),
            };
            assert_eq!(display_name(&entry), None);
            entry.source = named(entry.source, resource_name(&raw, "7"));
            assert_eq!(display_name(&entry), Some("  真正的来源  "));
            assert!(!entry.source.to_string().contains("PRIVATE"));
            entry.resource["song"]["voice_id"] = json!(9);
            assert_eq!(display_name(&entry), None);
            entry.resource["song"]
                .as_object_mut()
                .unwrap()
                .remove("voice_id");
            entry.source["id"] = json!("https://not-a-destination.invalid");
            assert_eq!(display_name(&entry), None);
        }
        assert!(named(json!({}), Some(" \t"))["source_name"].is_null());
        assert!(named(json!({}), Some(&"x".repeat(4097)))["source_name"].is_null());
    }
    #[test]
    fn a_search_scene_only_labels_an_ordinary_catalog_song() {
        use crate::playback::QueueEntry;

        let entry = |resource, source| QueueEntry {
            entry_id: "entry".into(),
            resource,
            source,
        };
        let source = json!({"kind":"songs","scene":"search","href":"PRIVATE","keyword":"PRIVATE"});
        assert!(search(&entry(json!({"id":"1"}), source.clone())));
        for kind in ["download_task_id", "local_id", "cloud_song_id", "voice_id"] {
            let mut resource = json!({"id":"1"});
            resource[kind] = json!("2");
            assert!(!search(&entry(resource, source.clone())));
        }
        for source in [
            json!({"kind":"playlist","scene":"search","id":"1"}),
            json!({"kind":"songs","scene":"searchBox","label":"搜索"}),
            json!({"kind":"songs","label":"搜索"}),
        ] {
            assert!(!search(&entry(json!({"id":"1"}), source)));
        }
    }

    #[test]
    fn queue_origin_uses_provenance_and_never_emits_payloads_or_unsafe_destinations() {
        use crate::playback::QueueEntry;
        let view = |resource, source| {
            queue(&QueueEntry {
                entry_id: "row".into(),
                resource,
                source,
            })
            .map(|target| serde_json::to_value(target).unwrap())
        };
        let catalog = json!({"id":"11", "song":{"album":{"id": 99}}});
        for kind in ["album", "artist", "playlist"] {
            assert_eq!(
                view(
                    catalog.clone(),
                    json!({"kind":kind,"id":"09007199254740993",
                "href":"PRIVATE_URL","toState":{"value":"PRIVATE"}})
                ),
                Some(json!({"kind":kind,"id":"09007199254740993"}))
            );
        }
        for (kind, resource) in [
            ("local", json!({"local_id":"10"})),
            ("cloud", json!({"cloud_song_id":"20"})),
            ("downloads", json!({"download_task_id":"30"})),
        ] {
            assert_eq!(
                view(
                    resource,
                    json!({"kind":kind,"path":"PRIVATE","query":"PRIVATE"})
                ),
                Some(json!({"kind":kind}))
            );
        }
        for (resource, source) in [
            (
                catalog.clone(),
                json!({"kind":"album","id":"https://example.invalid/private"}),
            ),
            (catalog.clone(), json!({"kind":"album","id":"0"})),
            (catalog.clone(), json!({"kind":"songs","scene":"search"})),
            (catalog.clone(), json!({"kind":"queue_more"})),
            (catalog.clone(), json!({"kind":"local"})),
            (
                json!({"local_id":"10"}),
                json!({"kind":"local","nativeOpen":true}),
            ),
            (
                json!({"download_task_id":"30","voice_id":"7"}),
                json!({"kind":"downloads"}),
            ),
            (json!({"voice_id":"7"}), json!({"kind":"album","id":42})),
        ] {
            assert!(view(resource, source).is_none());
        }
    }

    #[test]
    fn only_known_public_resource_identity_is_exposed() {
        for kind in ["playlist", "artist", "album"] {
            let value =
                project(&json!({"kind":kind,"id":42,"authorization":"PRIVATE","url":"PRIVATE"}))
                    .unwrap();
            assert_eq!(
                serde_json::to_value(value).unwrap(),
                json!({"kind":kind,"id":"42"})
            );
        }
        assert_eq!(
            serde_json::to_value(project(
                &json!({"kind":"podcast","scope":{"kind":"radio","id":"9","secret":"PRIVATE"}})
            ))
            .unwrap(),
            json!({"kind":"podcast","id":"9"})
        );
        for source in [
            json!({"kind":"songs","id":1}),
            json!({"kind":"playlist","id":"https://invalid.example/private"}),
            json!({"kind":"podcast","scope":{"kind":"liked"}}),
        ] {
            assert!(project(&source).is_none());
        }
    }
}
