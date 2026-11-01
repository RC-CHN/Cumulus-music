"""Cloud/local deletion choices, CUE retention and durable retry in the real WebView."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch

LABELS = ['local_keep_light', 'local_checked_dark', 'preflight_changed_light', 'cue_all_light', 'cue_mixed_dark',
          'write_unknown_dark', 'source_retained_dark', 'local_failed_light', 'local_retry_dark', 'receipt_reopened_light']


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
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_CLOUD_UI_PROOF='1', CUMULUS_CLOUD_BATCH_UI_PROOF='1',
               CUMULUS_CLOUD_MENU_UI_PROOF='1', CUMULUS_MINI_SCRIPT=str(Path(__file__).resolve().parents[2]/'desktop/examples/cloud_local_delete_probe.js'))
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
    assert code == 0, f'cloud/local delete process exit {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['cloudLocalDeleteUi'] and complete['stageCount'] == len(LABELS)
    states = {}
    for number, label in enumerate(LABELS, 1):
        path = root/'first'/f'ux_{number:02}_{label}.json'
        state = json.loads(path.read_text(encoding='utf-8')); states[label] = state
        assert state['theme'] == ('dark' if label.endswith('_dark') else 'light')
        assert not state['overflow'] and state['focusInside'] and state['localRows'] <= 20 and state['optionInline']
        assert all(b['hit'] for b in state['buttons'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert states['local_keep_light']['checked'] is False
    assert states['local_checked_dark']['checked'] is True
    assert '均来自' in states['cue_all_light']['cue'] and states['cue_all_light']['checked'] is None
    assert '其中 1 首' in states['cue_mixed_dark']['cue']
    assert states['write_unknown_dark']['optionDisabled'] and states['write_unknown_dark']['injectedReplyLoss']
    assert states['source_retained_dark']['phase'] == 'confirmed' and '保留 1 项' in states['source_retained_dark']['summary']
    assert states['local_failed_light']['phase'] == 'partial' and '未完成 1 项' in states['local_failed_light']['summary']
    assert '已清理 1 项' in states['local_retry_dark']['summary']
    assert states['receipt_reopened_light']['phase'] == 'partial' and '已清理 1 项' in states['receipt_reopened_light']['summary']
    assert '未执行 1 项' in states['preflight_changed_light']['summary']
    assert states['preflight_changed_light']['heading'] == '云盘删除结果' and '重新确认' in states['preflight_changed_light']['text']
    assert complete['syntheticWrites'] == [['1005'], ['1006', '1007']]
    report = dict(ok=True, states=len(LABELS), screenshots=len(LABELS) if args.screenshots else 0,
                  remoteDeleteRequests=2, localFilesDeleted=1, changedSourceRetained=True, cueSourceRetained=True,
                  realAccountWrites=0, realMusicRequests=0, physicalAudioOutputVerified=False)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
