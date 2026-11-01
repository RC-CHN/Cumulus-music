"""Cloud full-library batch UI, native queue, journaled writes and isolated FLACs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['batch_initial_light', 'batch_all_light', 'download_all_light', 'batch_last_dark', 'batch_range_dark', 'batch_resized_dark', 'batch_played_dark', 'batch_queued_dark', 'collect_rejected_dark', 'collect_confirmed_dark', 'download_subset_light', 'download_completed_light', 'delete_confirm_light', 'delete_partial_light', 'delete_remaining_light', 'delete_unknown_dark', 'delete_reopened_dark', 'delete_reconciled_dark', 'batch_small_dark', 'collect_small_dark', 'download_small_dark', 'batch_returned_dark', 'batch_restored_dark', 'batch_failed_dark', 'batch_recovered_dark', 'batch_changed_dark', 'batch_revision_dark', 'batch_empty_dark', 'account_replaced_light']


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
               CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/cloud_batch_probe.js'))
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
    assert code == 0, f'cloud batch process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cloudBatchUi'] and complete['stageCount'] == len(LABELS)
    states = {}
    for i, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{i:02}_{label}.json'
        state = json.loads(path.read_text(encoding='utf-8')); states[label] = state
        assert state['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert state['factoryCount'] == 1 and not state['nativePlayer']['error']
        assert not state['overflow'] and state['focusInside']
        assert all(b['hit'] for b in state['buttons'])
        assert len(state['rows']) <= 32 and all(r['height'] == 64 for r in state['rows'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert states['batch_initial_light']['rowCount'] == '406'
    assert states['batch_all_light']['selection'] == '已选 405 首歌曲'
    assert any(r['id'] == '1405' for r in states['batch_last_dark']['rows'])
    assert states['batch_range_dark']['selection'] == '已选 2 首歌曲'
    assert states['delete_partial_light']['phase'] == 'partial'
    assert states['delete_unknown_dark']['phase'] == states['delete_reopened_dark']['phase'] == 'uncertain'
    assert states['delete_remaining_light']['selection'] == '已选 1 首歌曲'
    assert states['batch_restored_dark']['normalScroll'] == states['batch_restored_dark']['savedScroll'] > 0
    assert not states['account_replaced_light']['batch']
    fixture = complete['syntheticCloud']
    assert fixture['realRequests'] == 0 and fixture['writes'] == [['1001', '1002'], ['1002']]
    assert fixture['collection'][-2:] == ['1001', '1002']
    assert len(fixture['collectWrites']) == 2
    assert fixture['downloadAuths'] == ['1001', '1002'] or fixture['downloadAuths'] == ['1002', '1001']
    assert fixture['mediaRequests'] == 2
    files = sorted((root/'batch-downloads').rglob('*.flac'))
    assert len(files) == 2
    assert all(hashlib.md5(p.read_bytes()).hexdigest() == '6b1c7d8698e1e941ad6f2046faa0b611' for p in files)
    report = dict(ok=True, states=len(states), screenshots=len(states) if args.screenshots else 0,
                  actualWebViews=1, syntheticOriginalFiles=len(files), syntheticDeleteRequests=2,
                  syntheticCollectRequests=2, realAccountWrites=0, realMusicRequests=0,
                  physicalAudioOutputVerified=False, physicalDisplayVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
