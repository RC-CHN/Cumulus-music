"""Synthetic WebView/native proof of the local song double-click queue preference."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
from motion_smoke import launch


LABELS = ['default_setting', 'default_replaced', 'add_setting', 'playlist_added',
          'duplicate_selected', 'catalog_added', 'explicit_replace']
EXPECTED = {'default_replaced': ('3', 40), 'playlist_added': ('3', 3),
            'duplicate_selected': ('3', 3), 'catalog_added': ('4', 4),
            'explicit_replace': ('1', 11)}


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
               CUMULUS_QUEUE_UI_PROOF='1', CUMULUS_QUEUE_CLICK_PROOF='1', CUMULUS_SEARCH_UI_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/queue_click_ui_probe.js'
    for phase, labels in [('first', LABELS), ('reopen', ['reopened_add'])]:
        script = root / (phase + '.js')
        reopened = phase == 'reopen'
        script.write_text(f'window.__queueClickReopened = {json.dumps(reopened)};\n' +
                          source.read_text(encoding='utf-8'), encoding='utf-8')
        env['CUMULUS_UX_SCRIPT'] = str(script)
        if reopened:
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        with (root / (phase + '-host.log')).open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['reopened'] == reopened and complete['count'] == len(labels)
        assert complete['realMusicRequests'] == 0
        for index, label in enumerate(labels, 1):
            path = output / f'ux_{index:02}_{label}.json'
            stage = json.loads(path.read_text(encoding='utf-8'))
            assert stage['preference'] == label.startswith('default_')
            assert len(stage['paintSamples']) == 2
            assert [sample['source'] for sample in stage['paintSamples']] == ['.sidebar-caption', '.playerbar']
            if label in EXPECTED:
                target, count = EXPECTED[label]
                queue, player = stage['nativeQueue'], stage['nativePlayer']
                current = next((item for item in queue['tracks'] if item['entryId'] == queue['current']), None)
                observed = stage['observedPlayback']
                actual = {'state': player['state'], 'track': (player['track'] or {}).get('id'),
                          'queueLength': player['queueLen'], 'queueRows': len(queue['tracks']),
                          'playId': player['playId'], 'position': player['position'],
                          'playerRevision': player['queueRevision'], 'queueRevision': queue['revision'],
                          'playerEntryId': player['entryId'], 'entryId': queue['current'],
                          'selectedTrack': current and current['track']['id']}
                detail = (label, {'track': target, 'queueLength': count}, observed, actual)
                assert actual['state'] == 'playing' and actual['queueLength'] == actual['queueRows'] == count, detail
                assert actual['track'] == actual['selectedTrack'] == target, detail
                assert actual['playerRevision'] == actual['queueRevision'] == observed['queueRevision'], detail
                assert actual['playId'] == observed['playId'] and actual['playerEntryId'] == actual['entryId'] == observed['entryId'], detail
                assert observed['position'] > .2 and actual['position'] >= observed['position'] - .05, detail
            if args.screenshots:
                assert path.with_suffix('.png').is_file()
                paint = json.loads(path.with_name('paint-' + path.name).read_text(encoding='utf-8'))
                assert len(paint['samples']) == 2 and all(sample['ok'] for sample in paint['samples']), paint
    with sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        rows = dict(db.execute(
            'SELECT key,value FROM state WHERE owner=? AND kind=?',
            ('local-device', 'playback-queue-click-preference-v1'),
        ))
    assert json.loads(rows['current']) == {'revision': 1, 'replace_queue': False}
    assert json.loads(rows['revision']) == 1
    report = {'ok': True, 'actualWebView': True, 'processLifetimes': 2,
              'states': len(LABELS) + 1, 'realMusicRequests': 0, 'nullAudio': True,
              'sqlitePreferenceVerified': True}
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
