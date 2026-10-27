use cumulus_core::cloud::upload::select_audio_sources;
use std::fs;

#[test]
fn recursive_selection_deduplicates_roots_excludes_ncm_and_non_audio_and_has_a_hard_batch_limit() {
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let first = nested.join("one.FLAC");
    let second = root.path().join("two.mp3");
    for path in [
        &first,
        &second,
        &nested.join("three.ncm"),
        &nested.join("cover.jpg"),
    ] {
        fs::write(path, b"fixture").unwrap();
    }
    let paths = select_audio_sources(&[root.path().into(), first.clone(), nested.clone()]).unwrap();
    let mut expected = vec![
        first.canonicalize().unwrap(),
        second.canonicalize().unwrap(),
    ];
    expected.sort();
    assert_eq!(paths, expected);
    for index in 0..499 {
        fs::write(nested.join(format!("{index}.wav")), b"fixture").unwrap();
    }
    assert!(select_audio_sources(&[root.path().into()]).is_err());
    assert!(select_audio_sources(&[root.path().join("missing")]).is_err());
}

#[cfg(unix)]
#[test]
fn recursive_selection_never_follows_symlink_cycles_or_links_outside_the_selected_directory() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    fs::write(other.path().join("outside.flac"), b"fixture").unwrap();
    fs::write(root.path().join("inside.flac"), b"fixture").unwrap();
    std::os::unix::fs::symlink(root.path(), root.path().join("cycle")).unwrap();
    std::os::unix::fs::symlink(other.path(), root.path().join("external")).unwrap();
    std::os::unix::fs::symlink(
        other.path().join("outside.flac"),
        root.path().join("linked.flac"),
    )
    .unwrap();
    assert_eq!(
        select_audio_sources(&[root.path().into()]).unwrap(),
        vec![root.path().join("inside.flac").canonicalize().unwrap()]
    );
}
