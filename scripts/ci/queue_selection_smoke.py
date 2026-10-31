"""Production queue selection, atomic batch removal and pointer reordering.

All HTTP, account and audio sources are synthetic. Vue, WebView, IPC, Rust
queue persistence and libmpv/null output are real; no real soundcard claim.
"""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


LABELS = ['single_selected', 'control_selected', 'shift_range', 'subtract_range',
          'collapsed_selection', 'group_preview', 'group_reordered',
          'escape_cancelled', 'pointer_cancelled', 'outside_cancelled',
          'revision_cancelled', 'remove_failed', 'batch_removed', 'all_selected',
          'selection_cleared', 'autoscroll_preview', 'autoscroll_cancelled']
LABELS += [f'selection_{width}_{theme}' for width, theme in
           [(1120, 'light'), (760, 'light'), (760, 'dark'), (1120, 'dark')]]
LABELS += ['double_click_played', 'explicit_played', 'queue_replaced']


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
               CUMULUS_QUEUE_SELECTION_UI_PROOF='1', CUMULUS_PLAYER_LAYOUT_PROOF='1')
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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/queue_selection_ui_probe.js'
    env['CUMULUS_UX_SCRIPT'] = str(source)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'queue selection exited {code}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['queueSelectionUi'] and complete['stageCount'] == len(LABELS)
    assert complete['realMusicRequests'] == complete['accountWrites'] == 0
    phases = {}
    for index, label in enumerate(LABELS, 1):
        path = root / 'first' / f'ux_{index:02}_{label}.json'
        row = json.loads(path.read_text(encoding='utf-8'))
        native, queue = row['nativePlayer'], row['nativeQueue']
        assert row['rendered'] < 40 and queue['revision'] == native['queueRevision']
        assert len(queue['tracks']) == native['queueLen'] == (40 if index <= 12 else 3 if label == 'queue_replaced' else 38)
        assert not native['error'] and native['state'] == 'paused'
        assert native['playId'] == row['playId'] and abs(native['position'] - 10) < .2
        assert phases.setdefault(row['phase'], native['playId']) == native['playId']
        if label == 'all_selected':
            assert '38' in row['selectionText'] and len(row['selection']) < 38
        if label == 'group_preview':
            assert row['dragging'] == len(row['selection']) == 2 and row['marker']
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert len(set(phases.values())) == 4
    report = dict(ok=True, actualWebView=True, states=len(LABELS),
                  screenshots=len(LABELS) if args.screenshots else 0,
                  queueSelectionUi=True, realMusicRequests=0, realAccountWrites=0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False,
                  syntheticPointerEvents=True)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
