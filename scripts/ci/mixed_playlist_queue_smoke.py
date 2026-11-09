"""Actual Linux WebView mixed playlist queue menus and trusted X11 drag."""

import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch
from search_queue_drop_smoke import TrustedDrag


STAGES = (
    'ux_01_paused_baseline', 'ux_02_local_next_menu', 'ux_03_local_next',
    'ux_04_online_next', 'ux_05_entire_playlist', 'ux_06_drag_target',
    'ux_07_drag_boundary', 'ux_08_drag_inserted',
)
LEGACY = 'a10c0000000000000000000000000001'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--expected-app', help='exact Vite App asset name for the embedded bundle')
    args = parser.parse_args()
    if not sys.platform.startswith('linux') or not os.environ.get('DISPLAY'):
        parser.error('requires a dedicated Xvfb display on Linux')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        path = root / 'profile' / key
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/mixed_playlist_queue_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_MIXED_PLAYLIST_QUEUE_PROOF='1',
               CUMULUS_MIXED_PLAYLIST_QUEUE_SCRIPT=str(source))

    with ExitStack() as stack:
        driver = TrustedDrag()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))

        def drag(path, payload, _pid):
            if path.stem == STAGES[5]:
                assert not driver.down
                driver.move(payload['pointer']['from'])
                driver.button(True)
                driver.move(payload['pointer']['to'])
            elif path.stem == STAGES[6]:
                assert driver.down
                driver.button(False)

        code = launch(args.binary.resolve(strict=True), env, log, run, True, before_ack=drag)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'mixed queue WebView exited {code}'
    assert tuple(p.stem for p in sorted(run.glob('ux_*.json'))) == STAGES
    assert tuple(p.stem for p in sorted(run.glob('ux_*.png'))) == STAGES
    states = [read(run / f'{label}.json') for label in STAGES]
    done = read(run / 'complete.json')
    assert done['ok'] and done['states'] == len(STAGES)
    if args.expected_app:
        assert done.get('frontendApp') == args.expected_app, ('wrong embedded frontend', done.get('frontendApp'))
    assert states[6]['trustedDown'] and states[6]['insertBefore'] == 3 and states[7]['trustedUp']
    assert done['trustedDown'] and done['trustedUp']
    paint = read(run / f'paint-{STAGES[6]}.json')
    assert len(paint['samples']) == 1 and paint['samples'][0]['ok']
    local, anchor = done['localId'], done['anchorId']
    assert local != anchor and local not in {'701', '702', LEGACY} and anchor not in {'701', '702', LEGACY}
    assert states[5]['selected'] == [local, '702']
    before = states[0]['player']
    assert before['state'] == 'paused' and before['id'] == anchor and before['position'] > .10
    assert before['resourceKind'] == 'local'
    expected = ([anchor], [anchor], [anchor, local], [anchor, '702', local],
                [anchor, local, '701', '702'], [anchor, local, '701', '702'],
                [anchor, local, '701', '702'], [anchor, '701', '702', local])
    revisions = []
    for row, order in zip((*states, done), (*expected, expected[-1]), strict=True):
        assert row['player'] == before, 'native paused playback was not preserved'
        if 'playlist' in row:
            assert row['playlist'] == [local, '701', LEGACY, '702'], 'queue action reordered source playlist'
        queue = row['queue']
        assert queue['current'] == before['entryId'] == queue['rows'][0]['entryId']
        assert [entry['id'] for entry in queue['rows']] == order
        assert [entry['local'] for entry in queue['rows']] == [item in (anchor, local) for item in order]
        assert all(entry['origin']['kind'] == 'playlist' and entry['origin']['id'] == '11'
                   for entry in queue['rows'][1:])
        assert all(entry['localMenu'] for entry in queue['rows'] if entry['id'] == local)
        revisions.append(queue['revision'])
        native = row['nativeReceipt']
        assert native['generation'] == 1 and native['localMembers'] == 1
        assert native['localId'] == local and native['localPosition'] == 0
        assert native['originalUnchanged'] and native['queueAnchorUnchanged']
        assert native['remoteMutations'] == native['unsafeAudioAttempts'] == 0
        assert all(set(ids) <= {'701', '702'} for ids in native['catalogReads'])
        assert all(audio['route'] == '/api/song/enhance/player/url/v1' and audio['id'] in {'701', '702'}
                   for audio in native['catalogAudioChecks'])

    assert len(set(revisions)) == 5
    assert revisions[0] == revisions[1] and revisions[4] == revisions[5] == revisions[6]
    assert revisions[7] == revisions[8]
    counts = [row['nativeReceipt']['catalogReads'] for row in states]
    assert counts[2] == counts[1], 'local next unnecessarily requested catalog metadata'
    assert counts[3][len(counts[2]):] == [['702']], 'catalog next fetched the wrong resource'
    assert counts[4][len(counts[3]):] == [['701', '702']], 'whole playlist hydrated the wrong catalog slice'
    assert counts[7][len(counts[6]):] == [['702']], 'mixed drag fetched anything but its selected catalog member'
    locale = read(run / 'native-startup.json')
    assert locale['numericLocale'] == 'C' and locale['messageLocale'] not in ('C', 'POSIX')
    result = dict(ok=True, actualWebView=True, actualNativePlayback=True,
                  trustedPointer=True, states=len(STAGES), screenshots=len(STAGES),
                  frontendApp=done.get('frontendApp'),
                  pausedPlaybackPreserved=True, originalFilesPreserved=True,
                  menuQueueOrders=[['anchor', 'local'], ['anchor', '702', 'local'],
                                   ['anchor', 'local', '701', '702']],
                  dropBeforeIndex=3, dropOrder=['anchor', '701', '702', 'local'],
                  remotePlaylistMutations=0, realMusicRequests=0)
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
