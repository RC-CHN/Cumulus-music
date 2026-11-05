use super::{PlayerOrigin, PlayerResourceKind, RecommendationOrigin, identity};
use crate::protocol::api::identifier;
use serde_json::Value;

pub(super) fn project(source: &Value) -> Option<PlayerOrigin> {
    let id = || identifier(&source["id"]).ok();
    Some(match source["kind"].as_str()? {
        "playlist" => PlayerOrigin::Playlist { id: id()? },
        "album" => PlayerOrigin::Album { id: id()? },
        "artist" => PlayerOrigin::Artist { id: id()? },
        "listened_book" => PlayerOrigin::Podcast { id: id()? },
        "podcast" if source["scope"]["kind"] == "radio" => PlayerOrigin::Podcast {
            id: identifier(&source["scope"]["id"]).ok()?,
        },
        _ => return None,
    })
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
