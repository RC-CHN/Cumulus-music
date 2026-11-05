"""Run the album queue drop through actual Tauri, GTK and null libmpv."""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch
from search_queue_drop_smoke import TrustedDrag


LABELS = (
    'paused_baseline', 'reverse_selection', 'ordered_drag_target',
    'insertion_boundary', 'ordered_drop', 'escape_cancelled',
    'filter_changed', 'tab_left',
)
TRACK_REQUESTS = {
    '/api/album/v3/detail', '/api/album/v4/detail', '/api/album/privilege',
    '/api/album/detail/dynamic', '/api/v3/song/detail', '/api/song/enhance/privilege',
}
SOURCE = '/api/song/enhance/player/url/v1'


def relevant_reads(record):
    return [
        row for row in record['albumQueueDropFixture']['calls'] if row['path'] in TRACK_REQUESTS
    ]


def source_reads(record):
    return [row['args'] for row in record['albumQueueDropFixture']['calls'] if row['path'] == SOURCE]


def expected_sources(*ids):
    return [dict(ids=json.dumps([id]), level='standard', encodeType='aac',
                 immerseType='c51', trialMode=-1) for id in ids]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--trusted-pointer', action='store_true', help='use XTest on an isolated Xvfb')
    args = parser.parse_args()
    if args.trusted_pointer and (not sys.platform.startswith('linux') or not args.screenshots
                                 or not os.environ.get('DISPLAY')):
        parser.error('--trusted-pointer requires --screenshots and a dedicated Linux Xvfb DISPLAY')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_ALBUM_PROOF='1',
               CUMULUS_ALBUM_QUEUE_DROP_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/album_queue_drop_ui_probe.js'
    script = root / 'album-queue-drop.js'
    script.write_text('window.__cumulusAlbumDropTrusted = ' + json.dumps(args.trusted_pointer)
                      + ';\n' + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    output = root / 'run/first'
    with ExitStack() as stack:
        hook = None
        if args.trusted_pointer:
            driver = TrustedDrag()
            stack.callback(driver.close)
            hook = driver.at_capture
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots,
                        before_ack=hook)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'album queue probe exit {status}'
    result = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert result['ok'] and result['actualNativePlayback'] and result['stageCount'] == len(LABELS)
    assert result['trustedPointer'] == args.trusted_pointer and result['dropCommands'] == 1
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    initial = updated = reads = None
    for index, label in enumerate(LABELS, 1):
        path = output / f'ux_{index:02}_{label}.json'
        record = json.loads(path.read_text(encoding='utf-8'))
        player, queue = record['checkedPlayer'], record['checkedQueue']
        if index == 1:
            initial, reads = player, relevant_reads(record)
            assert any(r['path'] == '/api/album/v3/detail' and r['args']['id'] == '7' for r in reads)
        if index == 5:
            updated = queue['revision']
            assert updated != initial['queueRevision']
            assert record['typed']['rows'] == [
                {'track_id': '11', 'track_index': 10}, {'track_id': '10', 'track_index': 9},
            ]
            assert record['typed']['id'] == '7'
        assert player['state'] == 'paused' and abs(player['position'] - 10) < .2
        assert all(player[k] == initial[k] for k in ('playId', 'entryId', 'position'))
        assert queue['current'] == initial['entryId'] and player['queueRevision'] == queue['revision']
        assert queue['revision'] == (initial['queueRevision'] if index < 5 else updated)
        ids = [row['track']['id'] for row in queue['tracks']]
        assert ids == (['701', '702'] if index < 5 else ['701', '11', '10', '702'])
        assert relevant_reads(record) == reads, f'unexpected album/song read during {label}'
        expected = expected_sources('701', '702') if index < 5 else expected_sources('701', '702', '11')
        assert source_reads(record) == expected, f'unexpected source authorization during {label}'
        if 2 <= index <= 4:
            assert record['selected'] == ['10', '11']
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    assert relevant_reads(result) == reads
    assert source_reads(result) == expected_sources('701', '702', '11')
    if args.screenshots:
        paint = json.loads((output / 'paint-ux_04_insertion_boundary.json').read_text(encoding='utf-8'))
        assert len(paint['samples']) == 1 and paint['samples'][0]['ok']
    if sys.platform.startswith('linux'):
        locale = json.loads((output / 'locale.json').read_text(encoding='utf-8'))
        assert locale['numeric'] == 'C' and locale['messages'] not in ('C', 'POSIX')
    report = dict(ok=True, actualWebView=True, actualNativePlayback=True,
                  trustedPointer=args.trusted_pointer, states=len(LABELS),
                  screenshots=len(LABELS) if args.screenshots else 0, dropCommands=1,
                  orderedQueue=['701', '11', '10', '702'], pausedPosition=initial['position'],
                  realMusicRequests=0, realAccountWrites=0, dropAlbumOrSongReads=0,
                  ordinaryNextPrefetches=1,
                  physicalAudioOutputVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
