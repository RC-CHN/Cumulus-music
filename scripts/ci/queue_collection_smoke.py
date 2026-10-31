"""Full-queue collection, cancellation, readback, voice exclusion and discovery."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


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
               CUMULUS_PLAYLIST_COLLECT_PROOF='1', CUMULUS_QUEUE_COLLECTION_UI_PROOF='1')
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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/queue_collection_ui_probe.js'
    script = root / 'queue-collection.js'
    script.write_text(source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    labels = ['queue']
    labels += [f'{stage}_{width}_{theme}' for width, theme in
               [(1120, 'light'), (760, 'light'), (760, 'dark'), (1120, 'dark')]
               for stage in ['list', 'collection', 'closed']]
    labels += ['preparing', 'closed_during_prepare', 'late_preparation_discarded',
               'preparation_failed', 'preparation_retried', 'stale_preparation',
               'lists_failed', 'lists_retried', 'collecting', 'collected', 'empty',
               'empty_read_failed', 'empty_read_retried', 'discovered', 'voices',
               'voices_unsupported', 'fm_collection',
               'fm_collection_closed', 'fm_queue_closed']
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'queue collection UI exited {code}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['queueCollectionUi'] and complete['stageCount'] == len(labels)
    assert complete['fullQueueReadbackVerified'] and complete['voiceExclusionVerified']
    assert complete['syntheticPlaylistWrites'] == complete['syntheticCollection']['writes'] == 1
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == complete['syntheticCollection']['creates'] == 0
    for i, label in enumerate(labels, 1):
        path = root / 'first' / f'ux_{i:02}_{label}.json'
        row = json.loads(path.read_text(encoding='utf-8'))
        native, queue = row['nativePlayer'], row['nativeQueue']
        assert row['rendered'] < 40 and len(queue['tracks']) == native['queueLen']
        assert native['fm'] == (row['family'] == 'fm') and not native['error']
        if row['empty']:
            assert native['state'] == 'idle' and not queue['tracks']
        else:
            assert native['state'] == 'paused' and native['playId'] == row['playId']
            assert abs(native['position'] - 10) < .2
        if row['family'] == 'voice':
            assert all(t['track'].get('voiceId') for t in queue['tracks'])
        if row['selection']:
            assert row['selection']['ids'] == [t['track']['id'] for t in queue['tracks']]
            assert row['selection']['total'] == 40 and row['selection']['unsupported'] == row['selection']['duplicates'] == 0
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    report = dict(ok=True, states=len(labels), actualWebView=True,
                  screenshots=len(labels) if args.screenshots else 0,
                  syntheticPlaylistWrites=1, realMusicRequests=0, realAccountWrites=0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
