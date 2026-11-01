"""Real native paging, bounded FM lists, offscreen selection and restore UI."""
import json
from fm_proof import arguments, run

LABELS = ['artist_start_light', 'artist_end_light', 'artist_removed_dark',
          'songs_start_dark', 'songs_selection_dark', 'more_failed_dark', 'more_retried_dark',
          'all_loaded_dark', 'far_end_dark', 'reverse_selected_light', 'row_restored_light',
          'refresh_failed_light', 'refreshed_dark', 'narrow_dark', 'selection_limit_dark',
          'limit_recycled_dark', 'batch_restored_dark']


def main():
    args = arguments(__doc__)
    root, complete = run(args, 'fm_blacklist_probe.js')
    assert complete['fmBlacklistUi'] and complete['stageCount'] == len(LABELS)
    identity = None
    for index, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{index:02}_{label}.json'
        row = json.loads(path.read_text(encoding='utf-8'))
        player, api = row['nativePlayer'], row['syntheticFmFeedback']
        assert row['fmBlacklistUi'] and not row.get('nativeFmFailure') and not row['stateUnavailable']
        assert player['fm'] and player['mode'] == 'sequential' and not player['error']
        assert player['state'] == 'paused' and abs(player['position'] - 10) < .2
        assert player['playId'] == row['playId'] and row['audioFactoryCount'] == 1
        identity = identity or player['playId']
        assert identity == player['playId']
        assert api['skipPosts'] == api['trashPosts'] == api['artistPosts'] == 0
        assert api['artistDeletes'] == (0 if index < 3 else 1)
        assert len(api['remoteArtists']) == (120 if index < 3 else 119)
        assert api['restorePosts'] == (0 if index < 11 else 1 if index < 17 else 2)
        assert len(api['remoteTrash']) == (1001 if index < 11 else 1000 if index < 17 else 900)
        assert row['selectionCount'] == (0 if index < 5 else 2 if index < 9 else 3 if index < 15 else 100 if index < 17 else 0)
        assert bool(row['listError']) == (index in [6, 12])
        assert row['loaded'] == (50 if index <= 6 or 11 <= index <= 14 or index == 17 else 100 if index == 7 else 150 if index in [15, 16] else 1001)
        assert len(row['lists']) == 2
        for value in row['lists']:
            assert value['rendered'] <= 24
            indices = value['indices']
            if indices:
                assert indices == list(range(indices[0], indices[-1]+1))
        if index == 2:
            assert row['activeRow'] == '119'
        if index >= 11 and index < 17:
            assert api['lastRestore'] == ['100900']
        if index == 17:
            assert len(api['lastRestore']) == len(set(api['lastRestore'])) == 100
            assert {'100000', '100049', '101000'}.issubset(api['lastRestore'])
            assert '100900' not in api['lastRestore']
        assert row['theme'] == ('dark' if label.endswith('_dark') else 'light')
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    report = dict(ok=True, actualWebView=True, states=len(LABELS),
                  screenshots=len(LABELS) if args.screenshots else 0, rows=1001, artists=120,
                  realMusicRequests=0, realAccountWrites=0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False)
    (root/'audit.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
