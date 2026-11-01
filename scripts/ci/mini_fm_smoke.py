"""Two production WebViews share FM mode/recovery, including an empty channel.

Account, endpoints and PCM are synthetic; SQLite, actor and libmpv/null are real.
No physical keyboard, soundcard, Windows device or official pixel-match claim.
"""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch


LABELS = ['personal_infinite_light', 'personal_single_light', 'personal_infinite_dark',
          'failed_dark', 'failed_light', 'dismissed_light', 'retried_light',
          'expanded_light', 'second_page_dark', 'mode_on_second_page_dark',
          'returned_main', 'reopened_single_dark', 'source_replaced_empty_light',
          'empty_single_light', 'empty_expanded_dark', 'empty_failed_dark',
          'failed_empty_returned_main', 'reopened_empty_failure_dark',
          'empty_retried_dark', 'returned_empty_main', 'reopened_empty_light',
          'ordinary_replaced_light']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/mini_fm_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MINI_FM_PROOF='1', CUMULUS_MINI_SCRIPT=str(source))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    verify(root, code, args.screenshots)


def verify(root, code, screenshots):
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'mini FM exited {code}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['fmMini'] and complete['stageCount'] == len(LABELS)
    assert complete['staleEmptyRejected'] and complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    modes = ['sequential', 'single_loop', 'sequential', 'sequential', 'sequential',
             'sequential', 'single_loop', 'single_loop', 'single_loop', 'sequential',
             'sequential', 'single_loop', 'sequential', 'single_loop', 'single_loop',
             'single_loop', 'single_loop', 'single_loop', 'sequential', 'sequential', 'sequential', 'order_loop']
    old_play_id = None
    assert len(modes) == len(LABELS)
    for index, (label, mode) in enumerate(zip(LABELS, modes), 1):
        path = root / 'first' / f'ux_{index:02}_{label}.json'
        value = json.loads(path.read_text(encoding='utf-8'))
        native, mini, fm = value['nativePlayer'], value['nativeMini'], value['nativeFm']
        empty = 13 <= index <= 21
        assert native['mode'] == mini['mode'] == mode
        assert native['fm'] == (index != 22)
        assert mini['modeFamily'] == ('ordinary' if index == 22 else 'fm')
        assert not native['error'] and native['queueLen'] == (0 if empty else 3 if index == 22 else 40)
        assert native['playId'] == value['expectedPlayId']
        pending = index in [4, 5, 6, 16, 17, 18]
        assert fm['modePending'] == pending
        assert bool(mini['fmModePending']) == pending
        assert value['factoryCount'] == 1
        # Enter's preload and playable pass each read the empty zone once.
        # Every subsequent repeat/recovery must leave both read counts fixed.
        assert value['syntheticFm'] == dict(personalReads=1, zoneReads=2 if index >= 13 else 0,
                                           skipWrites=0, trashWrites=0, reasonReads=0,
                                           artistWrites=0, remoteTrash=[], remoteArtists=[], corrupted=False)
        if empty:
            assert native['playId'] is None and native['state'] == 'idle'
        else:
            assert native['state'] == 'paused' and abs(native['position'] - 10) < .2
            if index <= 12:
                old_play_id = old_play_id or native['playId']
                assert native['playId'] == old_play_id
            else:
                assert native['playId'] and native['playId'] != old_play_id
        if value['windowKind'] == 'mini':
            assert not value['mainVisible'] and len(value['miniWindows']) == 1
            assert value['width'] == 420 and value['height'] == (516 if index in [8, 9, 10, 15, 16, 18, 19] else 206)
            assert not value['overflow'] and not value['genericError'] and not value['pending']
            assert bool(value['problem']) == (index in [4, 5, 16, 18])
            assert not value['queueEditable']
            assert all(opacity == '1' for opacity in value['queueTrackOpacities'])
            if empty:
                assert value['title'] == '暂时没有漫游推荐' and value['transportDisabled']
            if label.endswith('_dark'):
                assert value['theme'] == 'dark'
            if label.endswith('_light'):
                assert value['theme'] == 'light'
        else:
            assert value['mainVisible'] and not value['miniWindows']
            assert value['mainPlayerDom']['state'] == native['state']
            assert (value['mainPlayerDom'].get('playId') or None) == native['playId']
        if index in [8, 9, 10]:
            assert value['rows'] == (30 if index == 8 else 10)
        if screenshots:
            assert path.with_suffix('.png').is_file()
    result = dict(ok=True, actualWebViews=2, states=len(LABELS),
                  screenshots=len(LABELS) if screenshots else 0,
                  sharedAudioFactoryCount=1, fmModesAndRecoveryVerified=True,
                  emptyFmVerified=True, realMusicRequests=0, realAccountWrites=0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False,
                  officialMiniPixelMatchVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
