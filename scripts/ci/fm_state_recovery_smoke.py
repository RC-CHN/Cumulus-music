"""Actual FM UI/IPC recovery after a failed write and unreadable local state."""
import json
from fm_proof import arguments, run

LABELS = ['ready_light', 'unobserved_light', 'unobserved_dark', 'unobserved_narrow_dark',
          'readonly_failed_dark', 'readonly_recovered_dark', 'journal_dark', 'confirmed_dark',
          'refused_unobserved_light', 'refused_observed_light', 'restore_unobserved_light',
          'restore_readonly_failed_light', 'restore_observed_dark', 'restore_confirmed_dark']


def main():
    args = arguments(__doc__)
    root, complete = run(args, 'fm_state_recovery_probe.js')
    assert complete['fmStateRecoveryUi'] and complete['stageCount'] == len(LABELS)
    unavailable = {2, 3, 4, 5, 9, 11, 12}
    identities = set()
    previous = None
    for index, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{index:02}_{label}.json'
        row = json.loads(path.read_text(encoding='utf-8'))
        player, api = row['nativePlayer'], row['syntheticFmFeedback']
        assert player['fm'] and player['mode'] == 'sequential' and not player['error']
        assert player['state'] == 'paused' and abs(player['position'] - 10) < .2
        assert player['playId'] == row['playId'] and row['audioFactoryCount'] == 1
        if previous is not None:
            assert (previous != player['playId']) == (index == 8)
        previous = player['playId']
        identities.add(previous)
        assert row['stateUnavailable'] == api['unreadableCache'] == (index in unavailable)
        assert bool(row.get('nativeFmFailure')) == (index in unavailable)
        assert api['trashPosts'] == (0 if index == 1 else 1 if index < 9 else 2)
        assert api['restorePosts'] == (0 if index < 11 else 1)
        assert api['trashAttempts'] == api['trashPosts']
        assert api['skipPosts'] == api['artistPosts'] == api['artistDeletes'] == 0
        assert api['remoteTrash'] == (['101'] if 2 <= index < 11 else [])
        assert row['selected'] == (1 if 11 <= index <= 13 else 0)
        if index in unavailable:
            assert row['panel'] and row['panel']['previous'] and row['panel']['readIssue']
            if row['dialogOpen']:
                assert row['submitDisabled'] and row['reasonDisabled']
            else:
                assert row['restoreDisabled']
        if index in [6, 7]:
            assert any(op['kind'] == 'trash' and op['phase'] == 'uncertain' and op['desired'] for op in row['nativeFm']['operations'])
        if index == 13:
            assert row['restoreDisabled']
            assert any(op['kind'] == 'trash' and op['phase'] == 'uncertain' and not op['desired'] for op in row['nativeFm']['operations'])
        if index in [8, 10, 14]:
            assert not row['nativeFm']['operations']
        assert row['theme'] == ('dark' if label.endswith('_dark') else 'light')
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert len(identities) == 2
    report = dict(ok=True, actualWebView=True, states=len(LABELS), screenshots=len(LABELS) if args.screenshots else 0,
                  realMusicRequests=0, realAccountWrites=0, physicalDisplayVerified=False,
                  physicalAudioOutputVerified=False, cacheRecoveredByReadonlyApi=True)
    (root/'audit.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
