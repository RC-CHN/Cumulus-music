use super::{PlayerOrigin, PlayerResourceKind, QueueOrigin, RecommendationOrigin, identity};
use crate::protocol::api::identifier;
use serde_json::Value;

pub(super) fn project(source: &Value) -> Option<PlayerOrigin> {
    let id = || identifier(&source["id"]).ok();
    Some(match source["kind"].as_str()? {
        "playlist" => PlayerOrigin::Playlist { id: id()? },
        "album" => PlayerOrigin::Album { id: id()? },
        "artist" => PlayerOrigin::Artist { id: id()? },
        "listened_book" | "podcast_subscription_recommendation" => {
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
