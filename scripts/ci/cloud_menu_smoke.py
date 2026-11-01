"""Cloud resource menu identity, selection, focus, themes and recovery in a real WebView."""
import argparse
import hashlib
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['public_menu_light', 'share_light', 'private_menu_dark', 'cloud_played_dark', 'unavailable_menu_dark', 'local_menu_dark', 'local_launch_failed_dark', 'read_failed_dark', 'read_recovered_dark', 'selected_menu_dark', 'collect_dark', 'collect_failed_dark', 'delete_cancel_dark', 'delete_rejected_dark', 'one_selected_dark', 'queued_dark', 'menu_small_dark', 'share_small_dark', 'menu_small_light', 'collect_small_light', 'read_loading_light', 'query_replaced_light', 'account_replaced_light']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_CLOUD_BATCH_UI_PROOF='1', CUMULUS_CLOUD_MENU_UI_PROOF='1',
               CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/cloud_menu_probe.js'))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root/'first', args.screenshots)
    failed = root/'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'cloud menu process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cloudMenuUi'] and complete['stageCount'] == len(LABELS)
    states = {}
    for i, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{i:02}_{label}.json'
        state = json.loads(path.read_text(encoding='utf-8')); states[label] = state
        assert state['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert state['factoryCount'] == 1 and not state['nativePlayer']['error']
        assert not state['overflow'] and state['focusInside']
        assert all(b['hit'] for b in state['buttons'])
        assert all(r['height'] == 64 for r in state['rows'])
        assert all(c['appearance'] == 'none' and c['width'] == 16 for c in state['checkboxes'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    def keys(label):
        return [i['key'] for i in states[label]['menu']]
    assert keys('public_menu_light') == ['play', 'next', 'collect', 'share', 'remove']
    assert keys('private_menu_dark') == keys('unavailable_menu_dark') == ['play', 'next', 'collect', 'remove']
    assert keys('local_menu_dark') == ['play', 'next', 'collect', 'share', 'open', 'remove']
    assert keys('selected_menu_dark') == keys('one_selected_dark') == ['play', 'next', 'collect', 'remove']
    assert states['share_light']['share'] == 'https://music.163.com/song?id=900001'
    assert states['cloud_played_dark']['nativePlayer']['resourceKind'] == 'cloud'
    assert states['local_launch_failed_dark']['injectedShellFailure']
    assert complete['nativeOriginalVerified'] and not complete['osFileManagerVisuallyVerified']
    fixture = complete['syntheticCloud']
    assert fixture['realRequests'] == 0 and fixture['writes'] == [['1003', '1001']]
    assert fixture['collection'][-2:] == ['1003', '1001'] and len(fixture['collectWrites']) == 2
    assert fixture['downloadAuths'] == ['1004'] and fixture['mediaRequests'] == 1
    files = list((root/'menu-originals').rglob('*.flac'))
    assert len(files) == 1 and hashlib.md5(files[0].read_bytes()).hexdigest() == '6b1c7d8698e1e941ad6f2046faa0b611'
    report = dict(ok=True, states=len(states), screenshots=len(states) if args.screenshots else 0,
                  actualWebViews=1, syntheticOriginalFiles=len(files), osFileManagerVisuallyVerified=False, syntheticDeleteRequests=1,
                  syntheticCollectRequests=2, realAccountWrites=0, realMusicRequests=0,
                  physicalAudioOutputVerified=False, physicalDisplayVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
