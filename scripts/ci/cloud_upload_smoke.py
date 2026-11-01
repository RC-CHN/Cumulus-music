"""Cloud registration form with production Rust upload, file bytes and isolated service."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch
LABELS = ['empty_light', 'choosing_light', 'same_names_light', 'attachments_light',
          'partial_error_dark', 'corrected_attachment_dark', 'completed_tasks_dark',
          'unknown_receipt_light', 'receipt_recovered_light', 'many_first_small_dark',
          'many_last_small_dark', 'limit_small_dark', 'picker_error_small_light',
          'cancelled_picker_small_light', 'attachments_small_light', 'account_replaced_dark']


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
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_CLOUD_UPLOAD_UI_PROOF='1',
               CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/cloud_upload_probe.js'))
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
    assert code == 0, f'cloud upload process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cloudUploadUi'] and complete['stageCount'] == len(LABELS)
    states = {}
    for i, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{i:02}_{label}.json'
        state = json.loads(path.read_text(encoding='utf-8')); states[label] = state
        assert state['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert not state['overflow'] and state['focusInside']
        assert all(b['hit'] for b in state['buttons'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert len(states['same_names_light']['files']) == 2
    assert len(states['partial_error_dark']['files']) == 1 and states['partial_error_dark']['files'][0]['error']
    assert states['unknown_receipt_light']['uncertain'] == 'true'
    assert len(states['many_first_small_dark']['files']) == 20 and len(states['many_last_small_dark']['files']) == 20
    assert '500' in states['limit_small_dark']['dialogText']
    fixture = complete['syntheticUpload']
    assert fixture['realRequests'] == 0 and fixture['probe'] == 'ffprobe'
    assert len(fixture['publishedIds']) == 3 and len(fixture['objects']) == 7
    assert all(o['matchesFixture'] and o['bytes'] for o in fixture['objects'])
    assert fixture['calls']['/api/cloud/upload/check'] == 3 and fixture['calls']['/api/cloud/pub/v2'] == 3
    report = dict(ok=True, states=len(states), screenshots=len(states) if args.screenshots else 0,
                  actualWebViews=1, nativeRegisteredUploads=3, actualSyntheticObjects=7,
                  typedOsPickerInjected=True, serviceAndObjectStorageSynthetic=True,
                  actualFfprobe=True, lostReplyInjected=True, realAccountWrites=0,
                  realMusicRequests=0, physicalAudioOutputVerified=False, physicalDisplayVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
