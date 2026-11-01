"""Actual mini-window pin state, saved preference, bounded explicit recovery."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['pinned_light', 'unapplied_light', 'unapplied_dark', 'reapplied_dark',
          'unpinned_dark', 'unpin_unapplied_light', 'unpin_reapplied_light',
          'save_rejected_light', 'save_retried_dark', 'stale_intent_light',
          'stale_intent_healed_light', 'reopened_dark', 'damaged_light', 'repaired_light']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--unmanaged', action='store_true', help='Linux bare Xvfb: unknown state permits only reads')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/mini_pin_probe.js'))
    if args.unmanaged:
        assert os.name != 'nt' and not os.environ.get('CUMULUS_TEST_WINDOW_MANAGER')
        env['CUMULUS_MINI_PIN_UNMANAGED_PROOF'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root/'first', args.screenshots)
    failed = root/'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'mini pin exited {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    if args.unmanaged:
        assert complete['ok'] and complete['miniPinUnmanaged'] and complete['stageCount'] == 2
        revisions = []
        for index, theme in enumerate(['light', 'dark'], 1):
            path = root/'first'/f'ux_{index:02}_unmanaged_{theme}.json'
            row = json.loads(path.read_text(encoding='utf-8'))
            assert row['pinShown']['pinApplied'] is None and row['pinPressed'] is None
            assert row['pinFocus'] and row['pinHit'] and not row['overflow'] and row['alert']
            assert row['theme'] == theme and row['pinLabel'] == '重新读取置顶状态'
            assert row['factoryCount'] == 1 and row['nativePlayer']['playId'] == row['expectedPlayId']
            assert row['nativePlayer']['state'] == 'paused' and abs(row['nativePlayer']['position']-10) < .2
            revisions.append(row['pinShown']['preferences']['revision'])
            if args.screenshots:
                assert path.with_suffix('.png').is_file()
        assert revisions == ['initial', 'initial']
        report = dict(ok=True, actualWebViews=2, states=2, screenshots=2 if args.screenshots else 0,
                      unmanagedReadonlyVerified=True, nativeWindowPropertyVerified=False,
                      realMusicRequests=0, realAccountWrites=0, physicalDisplayVerified=False, physicalAudioOutputVerified=False)
        (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
        print(json.dumps(report))
        return
    assert complete['ok'] and complete['miniPinUi'] and complete['stageCount'] == len(LABELS)
    assert complete['readonlyApplyVerified'] and complete['staleIntentRejected'] and complete['ipcIsolationVerified']
    selected = [True, False, False, True, False, True, False, False, True, True, False, False, False, True]
    saved = [True, True, True, True, False, False, False, False, True, False, False, False, False, True]
    revisions = []
    for index, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{index:02}_{label}.json'
        row = json.loads(path.read_text(encoding='utf-8'))
        native, shown, player = row['pinNative'], row['pinShown'], row['nativePlayer']
        assert row['pinSelected'] == native['applied'] == shown['pinApplied'] == selected[index-1]
        assert native['preferences']['alwaysOnTop'] == shown['preferences']['alwaysOnTop'] == saved[index-1]
        assert row['pinHit'] and not row['overflow'] and row['width'] == 420 and row['height'] == 206
        if index in [4, 5, 7, 8, 9, 11, 14]:
            assert row['pinFocus']
        assert bool(row['alert']) == (index in [2, 3, 6, 8, 10, 13])
        assert row['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert not row['mainVisible'] and len(row['miniWindows']) == row['factoryCount'] == 1
        assert player['state'] == 'paused' and abs(player['position']-10) < .2
        assert player['playId'] == row['expectedPlayId'] and player['track']['id'] == '1' and not player['error']
        assert shown['preferences']['damaged'] == (index == 13)
        revisions.append(shown['preferences']['revision'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert len(set(revisions[:4])) == len(set(revisions[4:8])) == len(set(revisions[9:12])) == 1
    assert revisions[0] != revisions[4] != revisions[8] != revisions[9]
    assert revisions[12] is None and revisions[13] not in revisions[:13]
    report = dict(ok=True, actualWebViews=2, states=len(LABELS), screenshots=len(LABELS) if args.screenshots else 0,
                  readonlyApplyVerified=True, realMusicRequests=0, realAccountWrites=0,
                  nativeWindowPropertyVerified=True, physicalDisplayVerified=False, physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
