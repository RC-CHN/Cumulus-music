use super::*;
use super::{insertion::append, restore::restore_current};
use std::collections::HashSet;

pub(crate) fn transition(
    current: &QueueSnapshot,
    session: Option<&IntelligenceSession>,
    action: IntelligenceAction,
) -> Result<Plan> {
    match action {
        IntelligenceAction::Begin(input) => begin(current, *input),
        IntelligenceAction::Complete {
            session_id,
            resources,
            recommendations,
        } => {
            let mut session = require(current, session, &session_id)?.clone();
            if session.context.phase != IntelligencePhase::Seeded {
                return Err(Error::StaleResponse);
            }
            let mut queue = append(current, &session, resources)?;
            session.context.phase = IntelligencePhase::Active;
            session.revision = Uuid::new_v4().simple().to_string();
            session.recommendations = recommendations;
            queue.intelligence = Some(session.context.clone());
            session.validate(&queue)?;
            Ok(Plan {
                preserve_media: same_media(current.selected().as_ref(), queue.selected().as_ref()),
                queue,
                session: Some(session),
                start_seed: false,
                randomize: false,
            })
        }
        IntelligenceAction::Fail {
            session_id,
            failure,
        } => {
            let mut session = require(current, session, &session_id)?.clone();
            if session.context.phase != IntelligencePhase::Seeded {
                return Err(Error::StaleResponse);
            }
            let mut queue = restore_current(&session.fallback, current, true);
            if let Some(selected) = queue
                .items
                .iter_mut()
                .find(|e| Some(&e.entry_id) == queue.current.as_ref())
                && !session
                    .fallback
                    .items
                    .iter()
                    .any(|e| resource_id(&e.resource).ok() == resource_id(&selected.resource).ok())
                && let Some(source) = selected.source.as_object_mut()
            {
                source.remove(RETAINED);
                source.insert(TRANSIENT.into(), json!(true));
            }
            queue.mode = PlaybackMode::OrderLoop;
            queue.order = queue.items.iter().map(|e| e.entry_id.clone()).collect();
            session.context.phase = IntelligencePhase::Fallback;
            session.revision = Uuid::new_v4().simple().to_string();
            session.failure = Some(failure);
            queue.intelligence = Some(session.context.clone());
            session.validate(&queue)?;
            Ok(Plan {
                preserve_media: same_media(current.selected().as_ref(), queue.selected().as_ref()),
                queue,
                session: Some(session),
                start_seed: false,
                randomize: false,
            })
        }
        IntelligenceAction::Leave {
            session_id,
            mode,
            retain_missing_current,
        } => {
            let session = require(current, session, &session_id)?;
            let base = if session.backup.items.is_empty() {
                &session.fallback
            } else {
                &session.backup
            };
            let liked: HashSet<_> = session
                .fallback
                .items
                .iter()
                .filter_map(|e| resource_id(&e.resource).ok())
                .collect();
            let mut base = base.clone();
            base.items.retain(|e| {
                e.source[RETAINED] != true
                    || resource_id(&e.resource)
                        .ok()
                        .is_some_and(|id| liked.contains(&id))
            });
            for entry in &mut base.items {
                if let Some(source) = entry.source.as_object_mut() {
                    source.remove(RETAINED);
                }
            }
            let ids: HashSet<_> = base.items.iter().map(|e| e.entry_id.clone()).collect();
            base.order.retain(|id| ids.contains(id));
            if base.current.as_ref().is_some_and(|id| !ids.contains(id)) {
                base.current = base.order.first().cloned();
            }
            let mut queue = restore_current(&base, current, retain_missing_current);
            let randomize = mode == PlaybackMode::Random
                && (base.mode != PlaybackMode::Random || queue.items.len() != base.items.len());
            // The queue applies normal random ordering before this transaction
            // commits. All other modes use the restored display order.
            if mode != PlaybackMode::Random || base.mode != PlaybackMode::Random {
                queue.order = queue.items.iter().map(|e| e.entry_id.clone()).collect();
            }
            queue.mode = mode;
            queue.intelligence = None;
            queue.validate()?;
            Ok(Plan {
                preserve_media: same_media(current.selected().as_ref(), queue.selected().as_ref()),
                queue,
                session: None,
                start_seed: false,
                randomize,
            })
        }
    }
}

fn require<'a>(
    queue: &QueueSnapshot,
    session: Option<&'a IntelligenceSession>,
    id: &str,
) -> Result<&'a IntelligenceSession> {
    let session = session.ok_or(Error::StaleResponse)?;
    session.validate(queue)?;
    if session.context.id != id {
        return Err(Error::StaleResponse);
    }
    Ok(session)
}
fn begin(current: &QueueSnapshot, input: IntelligenceBegin) -> Result<Plan> {
    if current
        .intelligence
        .as_ref()
        .is_some_and(IntelligenceContext::active)
    {
        return Err(Error::State("intelligence mode is already active"));
    }
    if current.fm_source.is_some() || current.items.iter().any(|e| e.source["kind"] == "fm") {
        return Err(Error::State("leave FM before intelligence playback"));
    }
    input.request.payload()?;
    if !input.seed_source.is_object() {
        return Err(Error::InvalidInput("invalid intelligence seed source"));
    }
    if resource_id(&input.seed_resource)? != input.request.song_id {
        return Err(Error::InvalidInput("intelligence seed identity mismatch"));
    }
    liked(&input.liked)?;
    let seed = QueueEntry {
        entry_id: Uuid::new_v4().simple().to_string(),
        resource: input.seed_resource,
        source: input.seed_source,
    };
    let mut backup = if input.preserve_current_queue {
        current.clone()
    } else {
        input.liked.clone()
    };
    backup.intelligence = None;
    backup.items.retain(|e| e.source[TRANSIENT] != true);
    let kept: HashSet<_> = backup.items.iter().map(|e| e.entry_id.clone()).collect();
    backup.order.retain(|id| kept.contains(id));
    if backup.current.as_ref().is_some_and(|id| !kept.contains(id)) {
        backup.current = backup.order.first().cloned();
    }
    ordinary(&backup)?;
    let context = IntelligenceContext {
        id: Uuid::new_v4().simple().to_string(),
        phase: IntelligencePhase::Seeded,
    };
    let queue = QueueSnapshot {
        items: vec![seed.clone()],
        order: vec![seed.entry_id.clone()],
        current: Some(seed.entry_id.clone()),
        mode: PlaybackMode::OrderLoop,
        fm_source: None,
        intelligence: Some(context.clone()),
    };
    let session = IntelligenceSession {
        operation_id: input.operation_id,
        schema: 1,
        revision: Uuid::new_v4().simple().to_string(),
        context,
        request: input.request,
        seed_entry_id: seed.entry_id,
        last_mode: current.mode,
        backup,
        fallback: input.liked,
        recommendations: BTreeMap::new(),
        failure: None,
        liked_metadata: input.liked_metadata,
    };
    session.validate(&queue)?;
    Ok(Plan {
        queue,
        session: Some(session),
        preserve_media: false,
        start_seed: true,
        randomize: false,
    })
}
