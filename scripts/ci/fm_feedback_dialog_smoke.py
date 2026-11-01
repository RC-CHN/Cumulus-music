"""FM feedback dialog and blacklist UI with synthetic service writes only."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch

LABELS = ['initial_light', 'reason_loading_light', 'reason_cancelled_light',
          'ready_light', 'ready_dark', 'ready_narrow_dark', 'ready_narrow_light',
          'reason_error_light', 'reason_retry_light', 'reason_empty_light',
          'rejected_light', 'rejected_dark', 'rejected_cancelled_dark', 'pending_dark',
          'submitted_dark', 'changed_dark', 'changed_closed_dark', 'artist_partial_light',
          'recovery_light', 'artist_retried_light', 'blacklist_light', 'restore_selected_light',
          'restore_rejected_light', 'restore_rejected_dark', 'restore_uncertain_dark', 'restore_success_dark',
          'artist_unblocked_dark', 'returned_fm_dark', 'uncertain_dark',
          'uncertain_review_dark', 'reconciled_dark']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_FM_FEEDBACK_UI_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] / 'desktop/examples/fm_feedback_dialog_probe.js'))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'FM feedback exited {code}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['fmFeedbackUi'] and complete['stageCount'] == len(LABELS)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    groups, previous = set(), None
    dialogs = {2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 16, 18, 29}
    for index, label in enumerate(LABELS, 1):
        path = root / 'first' / f'ux_{index:02}_{label}.json'
        value = json.loads(path.read_text(encoding='utf-8'))
        native, fm, api = value['nativePlayer'], value['nativeFm'], value['syntheticFmFeedback']
        assert value['audioFactoryCount'] == 1 and native['fm'] and fm['active']
        assert native['mode'] == 'sequential' and not native['error']
        assert native['state'] == 'paused' and abs(native['position'] - 10) < .2
        assert native['playId'] == value['playId']
        if previous is not None:
            assert (native['playId'] != previous) == (index in [15, 16, 18, 31])
        previous = native['playId']
        groups.add(previous)
        assert bool(value['dialog']) == (index in dialogs)
        assert value['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert api['reasonReads'] == value['expectedReasonReads']
        assert api['trashPosts'] == (0 if index < 11 else 1 if index < 15 else 2 if index < 18 else 3 if index < 29 else 4)
        assert api['trashAttempts'] == api['trashPosts'] + (1 if index == 14 else 0)
        assert api['restorePosts'] == (0 if index < 23 else 1 if index < 25 else 2)
        assert api['artistPosts'] == (0 if index < 18 else 1 if index < 20 else 2)
        assert api['artistDeletes'] == (0 if index < 27 else 1)
        assert api['skipPosts'] == (0 if index < 16 else 1)
        assert api['remoteTrash'] == ([] if index < 15 or 25 <= index < 29 else ['101'] if index < 18 else ['101', '103'] if index < 25 else ['104'])
        assert api['remoteArtists'] == (['1'] if 20 <= index < 27 else [])
        assert value['selected'] == (2 if 22 <= index <= 25 else 0)
        if index in [11, 12, 18, 29]:
            assert value['dialog']['issue']
        if index == 18:
            assert any('歌曲已减少推荐' in message and '歌手屏蔽失败' in message for message in value['dialog']['issue'])
        if index in [16, 18, 29]:
            assert not value['dialog']['canSubmit']
        if index == 14:
            assert value['dialog']['busy'] and value['active']['tag'] == 'DIALOG'
        if index in [29, 30]:
            assert any(op['kind'] == 'trash' and op['phase'] == 'uncertain' for op in fm['operations'])
        if index == 31:
            assert not fm['operations']
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert len(groups) == 5
    result = dict(ok=True, actualWebView=True, states=len(LABELS),
                  screenshots=len(LABELS) if args.screenshots else 0,
                  syntheticServiceWrites=True, realMusicRequests=0, realAccountWrites=0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False,
                  officialPixelMatchVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
