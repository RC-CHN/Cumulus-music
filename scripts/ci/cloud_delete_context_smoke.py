"""Cloud deletion warning classes and immutable bounded confirmation in a real WebView."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['ordinary_light', 'off_store_dark', 'private_light', 'mixed_priority_dark',
          'unknown_light', 'bulk_first_light', 'bulk_second_light', 'bulk_last_dark',
          'bulk_small_dark', 'bulk_small_light', 'bulk_pending_light', 'bulk_partial_light', 'bulk_partial_dark',
          'remaining_dark', 'remaining_confirm_dark', 'account_replaced_light']


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
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_CLOUD_UI_PROOF='1', CUMULUS_CLOUD_BATCH_UI_PROOF='1',
               CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/cloud_delete_context_probe.js'))
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
    assert code == 0, f'cloud delete context process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cloudDeleteContextUi'] and complete['stageCount'] == len(LABELS)
    states = {}
    for i, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{i:02}_{label}.json'
        state = json.loads(path.read_text(encoding='utf-8')); states[label] = state
        assert state['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert not state['overflow'] and state['focusInside'] and len(state['titles']) <= 20
        assert all(button['hit'] for button in state['buttons'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert states['ordinary_light']['warning'] == '确定将选中的歌曲从网盘删除？'
    assert '无法再试听下载' in states['off_store_dark']['warning']
    assert all('其他端将无法同步' in states[label]['warning'] for label in ['private_light', 'mixed_priority_dark'])
    assert states['unknown_light']['legacyDtoInjected'] and '类型尚未核实' in states['unknown_light']['warning']
    assert len(states['bulk_first_light']['titles']) == len(states['bulk_second_light']['titles']) == 20
    assert len(states['bulk_last_dark']['titles']) == 5
    assert all(button['disabled'] for button in states['bulk_pending_light']['buttons'])
    assert states['bulk_partial_dark']['phase'] == 'partial' and '405' in states['bulk_partial_dark']['scope']
    assert not states['bulk_partial_dark'].get('warning') and '已删除 1 首' in states['bulk_partial_dark']['result']
    assert states['bulk_partial_dark']['titles'] == states['bulk_last_dark']['titles']
    assert '404' in states['remaining_confirm_dark']['scope']
    assert all('本地文件保留' in state['localNote'] for state in states.values() if state.get('localNote'))
    assert complete['syntheticCloud']['writes'] == [[str(i) for i in range(1001, 1406)]]
    assert complete['syntheticCloud']['rows'] == 404 and complete['syntheticCloud']['realRequests'] == 0
    physical = root/'first/physical.json'
    if physical.is_file():
        value = json.loads(physical.read_text(encoding='utf-8'))
        assert value['ok'] and value['states'] == len(LABELS)
    report = dict(ok=True, states=len(LABELS), screenshots=len(LABELS) if args.screenshots else 0,
                  actualWebViews=1, syntheticDeleteRequests=1, fixedCloudTargets=405, remainingCloudResources=404,
                  localFilesDeleted=0, legacyDtoLossInjected=True, realAccountWrites=0, realMusicRequests=0,
                  physicalDisplayVerified=physical.is_file(), physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
