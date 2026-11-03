"""Official task table/control scopes with native uploads, removal and isolated service."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch
LABELS = ['empty_light', 'first_page_light', 'last_page_dark', 'all_queued_dark',
          'all_paused_dark', 'clear_confirm_dark', 'single_remove_light',
          'single_removed_light', 'clear_snapshot_light', 'new_task_retained_light',
          'upload_complete_dark', 'history_dark', 'history_menu_dark', 'history_keyboard_menu_dark',
          'directory_error_dark', 'directory_retried_dark', 'history_remove_dark',
          'history_cleared_dark', 'encoding_failed_dark', 'encoding_rechecked_dark', 'allocation_inflight_light', 'clear_rejected_light',
          'allocation_unknown_light', 'tasks_small_light', 'tasks_small_dark',
          'confirm_small_dark', 'account_replaced_dark']


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
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_CLOUD_UPLOAD_UI_PROOF='1',
               CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/cloud_task_probe.js'))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
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
    assert code == 0, f'cloud task process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cloudTaskUi'] and complete['stageCount'] == len(LABELS)
    states = {}
    for i, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{i:02}_{label}.json'
        state = json.loads(path.read_text(encoding='utf-8')); states[label] = state
        assert state['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert not state['overflow'] and state['focusInside']
        assert all(b['hit'] for b in state['buttons'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert len(states['first_page_light']['taskRows']) == 200
    assert len(states['last_page_dark']['taskRows']) == 5
    assert '205 个任务' in states['clear_confirm_dark']['dialogText']
    assert '204 个任务' in states['clear_snapshot_light']['dialogText']
    assert len(states['new_task_retained_light']['taskRows']) == 1
    assert not states['upload_complete_dark']['taskRows']
    assert len(states['history_dark']['taskRows']) == 1
    assert [row['key'] for row in states['history_menu_dark']['menu']] == ['play', 'next', 'directory']
    assert all(row['disabled'] for row in states['history_menu_dark']['menu'][:2])
    fixture = complete['syntheticUpload']
    assert fixture['seededInputs'] == 208 and fixture['seededInputsIntact']
    assert fixture['realRequests'] == 0 and fixture['probe'] == 'ffprobe'
    assert len(fixture['publishedIds']) == 2 and len(fixture['objects']) == 2
    assert fixture['objects'][0]['matchesFixture']
    assert fixture['calls']['/api/cloud/upload/check'] == 4
    assert fixture['calls']['/api/whale/token/alloc'] == 3
    assert fixture['calls']['/api/cloud/pub/v2'] == 2
    assert complete['nativeDirectoryChecks'] == 1
    physical = root/'first/physical.json'
    physical_verified = False
    if physical.is_file():
        capture = json.loads(physical.read_text(encoding='utf-8'))
        assert capture['ok'] and capture['states'] == len(states)
        physical_verified = True
    report = dict(ok=True, states=len(states), screenshots=len(states) if args.screenshots else 0,
                  actualWebViews=1, nativeInitialPendingTasks=205, retainedInputFiles=208,
                  actualSyntheticObjects=2, osDirectoryLauncherInjected=True,
                  serviceAndObjectStorageSynthetic=True, actualFfprobe=True,
                  realAccountWrites=0, realMusicRequests=0, physicalAudioOutputVerified=False,
                  physicalDisplayVerified=physical_verified)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
