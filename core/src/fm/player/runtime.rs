use super::*;
pub(super) async fn watch(inner: Arc<Inner>) {
    let mut receiver = inner.controller.subscribe();
    let mut handled = None;
    loop {
        if receiver.changed().await.is_err() {
            break;
        }
        let view = receiver.borrow_and_update().clone();
        if !view.fm || view.state != PlaybackStatus::Ended {
            continue;
        }
        let key = (view.account.clone(), view.fence());
        if handled.as_ref() == Some(&key) {
            continue;
        }
        // The official end path shares the 250ms skip guard. Wait without
        // blocking the player, then re-read the fence before doing any work.
        tokio::time::sleep(std::time::Duration::from_millis(260)).await;
        let _gate = inner.gate.lock().await;
        let now = inner.controller.snapshot();
        if now.account != view.account
            || now.fence() != view.fence()
            || now.state != PlaybackStatus::Ended
        {
            continue;
        }
        handled = Some(key);
        let intent = inner.intents.load(Ordering::Acquire);
        if inner
            .step(&view.account, FmStep::Ended, intent)
            .await
            .is_err()
            && inner.session.check_generation(&view.account).is_ok()
        {
            let _ = inner.failure(&view.account, Some("advance_failed"));
        }
    }
}
