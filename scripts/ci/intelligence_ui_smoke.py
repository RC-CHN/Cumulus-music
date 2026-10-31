"""Production AI restore, distinct modes, native exit and visible queue projections.

All HTTP, account and audio sources are synthetic. Vue, WebView, IPC, Rust
queue persistence and libmpv/null output are real; no real soundcard claim.
"""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


LABELS = ['ownership_blocked','ownership_resumed'] + [f'{phase}_{width}_{theme}' for phase in ['seeded','active'] for width,theme in [(1120,'light'),(760,'light'),(760,'dark'),(1120,'dark')]]
LABELS += ['exit_failed','exit_retried','ordinary_cycle','exit_pending','exit_burst']
LABELS += [f'fallback_{width}_{theme}' for width,theme in [(1120,'light'),(760,'light'),(760,'dark'),(1120,'dark')]]
LABELS += ['hidden_reordered','hidden_empty_held','hidden_empty_reopened','hidden_transport','hidden_ordinary','stale_waiting','stale_replaced']


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
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_MODE_UI_PROOF='1', CUMULUS_QUEUE_UI_PROOF='1',
               CUMULUS_INTELLIGENCE_UI_PROOF='1')
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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/intelligence_ui_probe.js'
    env['CUMULUS_UX_SCRIPT'] = str(source)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'intelligence UI exited {code}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['intelligenceUi'] and complete['stageCount'] == len(LABELS)
    assert complete['realMusicRequests'] == complete['accountWrites'] == 0
    assert complete['productionStartVerified'] is False
    for index, label in enumerate(LABELS, 1):
        path = root / 'first' / f'ux_{index:02}_{label}.json'
        row = json.loads(path.read_text(encoding='utf-8'))
        native, queue = row['nativePlayer'], row['nativeQueue']
        expected_count = 1 if (label.startswith('seeded_') or label.startswith('ownership_')) else 0 if label in ['hidden_empty_held','hidden_empty_reopened','hidden_transport','hidden_ordinary'] else 2 if label == 'stale_replaced' else 3
        expected_phase = 'seeded' if (label.startswith('seeded_') or label.startswith('ownership_')) else 'active' if label.startswith('active_') or label in ['exit_failed','exit_pending','stale_waiting'] else 'fallback' if label.startswith('fallback_') or label in ['hidden_reordered','hidden_empty_held','hidden_empty_reopened','hidden_transport'] else None
        assert native['queueLen'] == len(queue['tracks']) == row['total'] == expected_count
        assert native['intelligence'] == row['phase'] == expected_phase
        if label.startswith('ownership_') or label.startswith('seeded_'):
            expected_ids = ['8']
        elif label.startswith('active_') or label in ['exit_failed', 'exit_pending', 'stale_waiting']:
            expected_ids = ['8', '10', '11']
        elif label == 'hidden_reordered':
            expected_ids = ['8', '7', '9']
        elif label == 'stale_replaced':
            expected_ids = ['20', '21']
        elif not expected_count:
            expected_ids = []
        else:
            expected_ids = ['7', '8', '9']
        assert [track['track']['id'] for track in queue['tracks']] == expected_ids
        if label != 'exit_pending':
            assert native['mode'] == {'心动模式': 'order_loop', '列表循环': 'order_loop',
                                      '单曲循环': 'single_loop', '随机播放': 'random',
                                      '顺序播放': 'sequential'}[row['mode']]
        if label.startswith('fallback_') or label.startswith('hidden_'):
            assert queue['current'] is None and native['track']['id'] == '90' and row['outside']
        assert queue['revision'] == native['queueRevision']
        assert native['state'] == 'paused' and native['playId'] == row['playId'] and abs(native['position'] - 10) < .2
        assert not native['error'] and all(track['track']['id'] != '90' for track in queue['tracks'])
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    report = dict(ok=True, actualWebView=True, states=len(LABELS),
                  screenshots=len(LABELS) if args.screenshots else 0,
                  intelligenceUi=True, productionStartVerified=False, realMusicRequests=0, realAccountWrites=0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False,
                  syntheticPointerEvents=True)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
