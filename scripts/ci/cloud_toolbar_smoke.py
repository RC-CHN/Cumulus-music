"""Official cloud toolbar, full filtered results, operation fences and real isolated downloads."""
import argparse
import hashlib
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['toolbar_menu_light', 'download_all_light', 'queued_all_light', 'empty_search_menu_dark', 'batch_all_dark', 'batch_returned_dark', 'filtered_menu_dark', 'download_filter_dark', 'download_confirmed_dark', 'queue_reading_dark', 'queue_intent_changed_dark', 'queue_retried_dark', 'read_failed_dark', 'read_retried_dark', 'download_limit_dark', 'queue_limit_dark', 'snapshot_changed_dark', 'snapshot_refreshed_dark', 'toolbar_small_light', 'download_small_light', 'toolbar_small_dark', 'query_replaced_dark', 'account_replaced_light', 'empty_library_light']


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
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_CLOUD_BATCH_UI_PROOF='1',
               CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/cloud_toolbar_probe.js'))
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
    assert code == 0, f'cloud toolbar process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cloudToolbarUi'] and complete['stageCount'] == len(LABELS)
    states = {}
    for i, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{i:02}_{label}.json'
        state = json.loads(path.read_text(encoding='utf-8')); states[label] = state
        assert state['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert state['factoryCount'] == 1 and not state['nativePlayer']['error']
        assert not state['overflow'] and state['focusInside']
        assert all(b['hit'] for b in state['buttons'])
        if not state['batch']:
            assert all(r['height'] == 64 for r in state['rows'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert [i['key'] for i in states['toolbar_menu_light']['menu']] == ['queue', 'batch', 'download']
    assert [i['label'] for i in states['toolbar_menu_light']['menu']] == ['添加全部至播放列表', '批量操作', '下载全部']
    assert '405 首云盘原文件' in states['download_all_light']['dialogText']
    ids = states['queued_all_light']['queuedIds']
    assert ids == ['1'] + [str(i) for i in range(1405, 1000, -1)] + [str(i) for i in range(2, 41)]
    assert states['queued_all_light']['nativePlayer']['state'] == 'paused'
    assert abs(states['queued_all_light']['nativePlayer']['position'] - 10) < .2
    assert '9 首云盘原文件' in states['download_filter_dark']['dialogText']
    assert states['queue_reading_dark']['toolbarBusy']
    assert '405 首' in states['batch_all_dark']['batchText']
    assert states['batch_returned_dark']['keyword'] == 'no-result'
    assert all(i['disabled'] for i in states['empty_search_menu_dark']['menu'] if i['key'] != 'batch')
    fixture = complete['syntheticCloud']
    assert fixture['realRequests'] == 0 and fixture['writes'] == [] and fixture['collectWrites'] == []
    assert sorted(fixture['downloadAuths']) == [str(i) for i in range(1001, 1010)] and fixture['mediaRequests'] == 9
    files = list((root/'batch-downloads').rglob('*.flac'))
    assert len(files) == 9 and all(hashlib.md5(p.read_bytes()).hexdigest() == '6b1c7d8698e1e941ad6f2046faa0b611' for p in files)
    report = dict(ok=True, states=len(states), screenshots=len(states) if args.screenshots else 0,
                  actualWebViews=1, syntheticOriginalFiles=len(files), osFileManagerVisuallyVerified=False, syntheticDeleteRequests=0,
                  syntheticCollectRequests=0, realAccountWrites=0, realMusicRequests=0,
                  physicalAudioOutputVerified=False, physicalDisplayVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
