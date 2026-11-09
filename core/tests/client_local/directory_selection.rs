use super::*;

fn choices(
    directories: &DirectorySelection,
    pick: impl Fn(&str, bool) -> bool,
) -> Vec<DirectoryChoice> {
    directories
        .entries
        .iter()
        .map(|entry| DirectoryChoice {
            id: entry.id.clone(),
            checked: pick(&entry.path, entry.checked),
        })
        .collect()
}

async fn index_ready(music: &MusicClient, count: usize) -> LocalPageView {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = music.local_page(LocalQuery::default()).await.unwrap();
            if page.total == count
                && page
                    .roots
                    .iter()
                    .filter(|root| root.enabled)
                    .all(|root| root.status == WatchStatus::Ready)
            {
                return page;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("all checked roots finish native scanning")
}

#[tokio::test]
async fn checked_directories_require_confirm_preserve_overlaps_and_keep_loaded_playback() {
    let root = tempfile::tempdir().unwrap();
    let music_dir = root.path().join("album");
    let child = music_dir.join("inside");
    let prefix = root.path().join("album-sibling");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir_all(&prefix).unwrap();
    let samples = [
        music_dir.join("Parent-only.wav"),
        child.join("Nested.wav"),
        prefix.join("Adjacent.wav"),
    ];
    for (index, path) in samples.iter().enumerate() {
        std::fs::write(path, [index as u8]).unwrap();
    }

    let state = root.path().join("state");
    let (_, music, library) = client(&state, Arc::new(Probe::default()));
    // A native index can already contain imports outside any watch roots.
    library.scan(&music_dir).unwrap();
    let original = music.local_directory_selection().await.unwrap();
    let staged = music
        .stage_local_directory(music_dir, original.revision.clone())
        .await
        .unwrap();
    assert!(!staged.entries[0].checked && !staged.entries[0].paused);
    assert_eq!(
        music.local_page(LocalQuery::default()).await.unwrap().total,
        2
    );
    assert!(matches!(
        music
            .apply_local_directories(original.revision, vec![])
            .await,
        Err(Error::QueueChanged)
    ));
    let staged = music
        .stage_local_directory(child, staged.revision)
        .await
        .unwrap();
    let staged = music
        .stage_local_directory(prefix, staged.revision)
        .await
        .unwrap();
    let no_op = music
        .apply_local_directories(
            staged.revision.clone(),
            choices(&staged, |_, checked| checked),
        )
        .await
        .unwrap();
    assert_eq!(no_op.revision, staged.revision);
    assert_eq!(
        music.local_page(LocalQuery::default()).await.unwrap().total,
        2
    );
    let active = music
        .apply_local_directories(staged.revision.clone(), choices(&staged, |_, _| true))
        .await
        .unwrap();
    assert_ne!(active.revision, staged.revision);
    let page = index_ready(&music, 3).await;
    let parent = page
        .tracks
        .iter()
        .find(|row| row.track.name == "Parent-only")
        .unwrap();
    let loaded = music
        .play_selection(
            "0",
            PlaySelection::Local {
                query: LocalQuery::default(),
                revision: page.revision.clone(),
                start_id: Some(parent.track.id.clone()),
            },
        )
        .await
        .unwrap();
    let paused = music
        .player_action("0", PlayerAction::Pause { paused: true })
        .await
        .unwrap();
    assert_eq!(loaded.play_id, paused.play_id);

    let selected_root = active.entries.first().unwrap();
    music
        .local_command(LocalCommand::Disable {
            id: selected_root.id.clone(),
        })
        .await
        .unwrap();
    let paused_dir = music.local_directory_selection().await.unwrap();
    assert!(paused_dir.entries[0].paused && paused_dir.entries[0].checked);
    assert_eq!(
        music.local_page(LocalQuery::default()).await.unwrap().total,
        3
    );
    let rejected = vec![
        DirectoryChoice {
            id: paused_dir.entries[0].id.clone(),
            checked: false,
        },
        DirectoryChoice {
            id: paused_dir.entries[0].id.clone(),
            checked: true,
        },
        DirectoryChoice {
            id: paused_dir.entries[2].id.clone(),
            checked: true,
        },
    ];
    assert!(matches!(
        music
            .apply_local_directories(paused_dir.revision.clone(), rejected)
            .await,
        Err(Error::QueueChanged)
    ));
    let removed = music
        .apply_local_directories(
            paused_dir.revision.clone(),
            choices(&paused_dir, |path, checked| {
                checked
                    && Path::new(path)
                        .file_name()
                        .is_none_or(|name| name != "album")
            }),
        )
        .await
        .unwrap();
    assert!(!removed.entries[0].checked && !removed.entries[0].paused);
    let remaining = music.local_page(LocalQuery::default()).await.unwrap();
    assert_ne!(remaining.revision, page.revision);
    let titles = remaining
        .tracks
        .iter()
        .map(|r| r.track.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(titles, ["Adjacent", "Nested"].into_iter().collect());
    let held = music.player_snapshot("0").await.unwrap();
    assert_eq!(
        (
            held.play_id,
            held.entry_id,
            held.queue_revision,
            held.position,
            held.state
        ),
        (
            paused.play_id,
            paused.entry_id,
            paused.queue_revision,
            paused.position,
            paused.state
        )
    );
    for (index, path) in samples.iter().enumerate() {
        assert_eq!(std::fs::read(path).unwrap(), [index as u8]);
    }
    drop(music);
    let rescanning = Arc::new(Probe::default());
    rescanning.hold.store(true, Ordering::SeqCst);
    let (_, reopened, _) = client(&state, rescanning.clone());
    let saved = reopened.local_directory_selection().await.unwrap();
    assert_eq!(saved.revision, removed.revision);
    assert_eq!(
        reopened
            .local_page(LocalQuery::default())
            .await
            .unwrap()
            .total,
        2
    );
    let new_selection = reopened
        .apply_local_directories(saved.revision.clone(), choices(&saved, |_, _| true))
        .await;
    let before_ready = reopened.local_page(LocalQuery::default()).await;
    // The still-unverified old row must not flash back before a fresh scan.
    rescanning.hold.store(false, Ordering::SeqCst);
    let new_selection = new_selection.unwrap();
    assert_eq!(before_ready.unwrap().total, 2);
    index_ready(&reopened, 3).await;
    assert_eq!(
        reopened.local_directory_selection().await.unwrap().revision,
        new_selection.revision
    );
}

#[tokio::test]
async fn unchecking_during_scan_invalidates_the_held_scan_and_keeps_the_file() {
    let root = tempfile::tempdir().unwrap();
    let media = root.path().join("new");
    std::fs::create_dir(&media).unwrap();
    let file = media.join("Held.wav");
    std::fs::write(&file, [12]).unwrap();
    let probe = Arc::new(Probe::default());
    probe.hold.store(true, Ordering::SeqCst);
    let (_, music, _) = client(&root.path().join("state"), probe.clone());
    let original = music.local_directory_selection().await.unwrap();
    let staged = music
        .stage_local_directory(media, original.revision)
        .await
        .unwrap();
    let selected = music
        .apply_local_directories(staged.revision.clone(), choices(&staged, |_, _| true))
        .await
        .unwrap();
    let entered = tokio::time::timeout(Duration::from_secs(4), async {
        while !probe.entered.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let unselected = music
        .apply_local_directories(selected.revision.clone(), choices(&selected, |_, _| false))
        .await;
    // Always release the held probe before asserting, including on failure.
    probe.hold.store(false, Ordering::SeqCst);
    entered.expect("native scanner holds the selected source");
    let unselected = unselected.unwrap();
    assert!(!unselected.entries[0].checked);
    let page = music.local_page(LocalQuery::default()).await.unwrap();
    assert_eq!(page.total, 0);
    assert_eq!(page.roots[0].status, WatchStatus::Disabled);
    assert_eq!(std::fs::read(file).unwrap(), [12]);
}
