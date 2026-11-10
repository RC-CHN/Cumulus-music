"""Actual Linux WebView playback from a sealed cached playlist and native download ledger."""
import argparse
import json
import os
from pathlib import Path
import re
import sys

from motion_smoke import launch


STAGES = (
    'ux_01_live', 'ux_02_eligible', 'ux_03_downloaded', 'ux_04_cue',
    'ux_05_digest_rejected', 'ux_06_size_disabled', 'ux_07_restored',
    'ux_08_play_available', 'ux_09_next_downloaded',
    'ux_10_header_later_page', 'ux_11_header_download_playing', 'ux_12_header_empty_preserves_play',
    'ux_13_queue_empty_preserves_play', 'ux_14_queue_filtered_insert', 'ux_15_queue_starts_empty',
)
LEGACY = 'a10c0000000000000000000000000001'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--expected-app', help='exact Vite App asset name for the embedded bundle')
    args = parser.parse_args()
    if not sys.platform.startswith('linux') or not os.environ.get('DISPLAY'):
        parser.error('requires an isolated Xvfb display on Linux')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    repo = Path(__file__).resolve().parents[2]
    script = repo / 'desktop/examples/offline_playlist_download_ui_probe.js'
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        path = root / 'profile' / key
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_OFFLINE_PLAYLIST_DOWNLOAD_PROOF='1',
               CUMULUS_OFFLINE_PLAYLIST_DOWNLOAD_SCRIPT=str(script))
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, run, args.screenshots)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'offline playlist download proof exited {code}'
    assert tuple(p.stem for p in sorted(run.glob('ux_*.json'))) == STAGES
    images = tuple(p.stem for p in sorted(run.glob('ux_*.png')))
    assert images == (STAGES if args.screenshots else ())
    states = [read(run / f'{stage}.json') for stage in STAGES]
    done = read(run / 'complete.json')
    assert done['ok'] and 1 <= done['setupAttempts'] <= 4
    local = done['local']
    assert local not in {'701', '702', LEGACY}
    order = [local, '701', LEGACY, '702']
    eligible = [local, '701']
    original = states[:9]
    header_states = states[9:12]
    assert all(state['rows'] == order for state in original)
    assert {'701', '702'} <= set(original[0]['enabled'])
    assert [state['enabled'] for state in original[1:]] == [
        eligible, eligible, eligible, eligible, [local], eligible, eligible, eligible,
    ]
    started = [states[index]['player'] for index in (2, 3, 7, 8)]
    assert [(v['id'], v['resourceKind']) for v in started] == [
        ('701', 'downloaded'), (local, 'local'), (local, 'local'), ('701', 'downloaded'),
    ]
    assert all(v['queue'] == eligible and v['position'] > .10 and v['playId'] for v in started)
    assert len({v['playId'] for v in started}) == len(started)
    assert all(7900 <= v['durationMs'] <= 8100 for v in (started[1], started[2]))
    rejected = states[4]['rejected']
    assert rejected['id'] == '701' and rejected['resourceKind'] == 'downloaded'
    assert rejected['state'] == 'error' and rejected['error']
    assert rejected['position'] is None or 0 <= rejected['position'] <= .10

    assert all(state['loadedRows'] == 100 and state['rows'] and not state['enabled']
               and all(801 <= int(row) <= 900 for row in state['rows']) for state in header_states)
    later = header_states[1]['player']
    assert later['id'] == '701' and later['resourceKind'] == 'downloaded'
    assert later['queue'] == ['701'] and later['position'] > .10 and later['playId'] not in {
        state['playId'] for state in started}
    empty = header_states[2]
    assert empty['message'] == '歌单中没有可离线播放的曲目'
    assert empty['incumbent'] == empty['preserved']
    assert empty['incumbent']['state'] == 'paused' and empty['incumbent']['queue'] == ['701']
    assert empty['incumbent']['playId'] == later['playId'] and empty['incumbent']['queueRevision']

    menu_empty, menu_added, menu_start = states[12:]
    assert menu_empty['incumbent'] == menu_empty['afterRejectedAdd'] == empty['incumbent']
    assert menu_added['rows'] == ['701'] and menu_added['added']['queue'] == ['701', local]
    assert menu_added['added']['state'] == 'paused'
    assert menu_added['added']['playId'] == empty['incumbent']['playId']
    assert menu_added['added']['position'] == empty['incumbent']['position']
    assert menu_start['rows'] == order and menu_start['player']['queue'] == eligible
    assert menu_start['player']['id'] == local and menu_start['player']['resourceKind'] == 'local'
    assert menu_start['player']['position'] > .10 and menu_start['player']['playId'] != later['playId']

    receipts = [state['nativeReceipt'] for state in states]
    assert [r['unavailable'] for r in receipts] == [0, 1, 1, 1, 1, 2, 3, 3, 3, 3, 3, 3, 3, 4, 4]
    header_receipts = receipts[9:12]
    assert [r['headerTransportCuts'] for r in header_receipts] == [1, 1, 2]
    assert [r['headerReads'] for r in header_receipts] == [2, 2, 4]
    assert [r['headerLastId'] for r in header_receipts] == ['701', '701', '702']
    assert all(r['headerRows'] == 101 and r['headerWithoutUrls'] for r in header_receipts)
    for index, r in enumerate((*receipts, done['nativeReceipt'])):
        assert r['cachedRows'] == 4 and r['cachedWithoutUrls'] and r['cueUnchanged']
        assert r['denied'] == r['downloadAuthorizationAttempts'] == 0
        mixed = r['mixed']
        assert mixed['generation'] == 1 and mixed['localMembers'] == 1 and mixed['localId'] == local
        assert mixed['originalUnchanged'] and mixed['localPosition'] == 0
        assert mixed['remoteMutations'] == mixed['unsafeAudioAttempts'] == 0
        assert not mixed['catalogAudioChecks'], 'offline-only playback attempted a player URL request'
        extra_reads = [] if index < 9 else [['701']] if index < 11 else [['701'], ['702']]
        assert mixed['catalogReads'] == receipts[0]['mixed']['catalogReads'] + extra_reads
        assert mixed['detailReads'] == receipts[0]['mixed']['detailReads']
        downloads = r['downloads']
        assert downloads['sameAccountLedgerSongs'] == ['701']
        assert downloads['foreignAccountLedgerSongs'] == ['702']
        assert downloads['foreignAccountIntact']
        assert downloads['sameAccountIntact'] is (index not in (4, 5))
        assert downloads['sameAccountSizeIntact'] is (index != 5)
    locale = read(run / 'native-startup.json')
    assert locale['numericLocale'] == 'C' and locale['messageLocale'] not in ('C', 'POSIX')
    entry = done['frontendEntry']
    assert re.fullmatch(r'index-[a-zA-Z0-9_-]+\.js', entry)
    if args.expected_app:
        assets = repo / 'frontend/dist/assets'
        imports = set(re.findall(r'"\./(App-[a-zA-Z0-9_-]+\.js)"',
                                 (assets / entry).read_text(encoding='utf-8')))
        assert imports == {args.expected_app}, ('wrong embedded frontend entry', entry, imports)
        assert (assets / args.expected_app).is_file()
    report = dict(ok=True, actualWebView=True, nativeDownloadedPlayback=True, nativeCuePlayback=True,
                  sameAccountLedgerRequired=True, damagedDigestRejected=True, damagedLengthDisabled=True,
                  restoredPublicationPlayable=True, headerAndNextUseOfflineQueue=True,
                  headerPlaysBeyondLoadedPage=True, emptyHeaderPreservesIncumbent=True,
                  menuQueuesWholeCache=True, menuPreservesIncumbent=True, menuStartsEmptyQueue=True,
                  realMusicRequests=0, playbackAuthorizationAttempts=0, remotePlaylistMutations=0,
                  frontendEntry=entry, frontendApp=args.expected_app, stages=len(STAGES),
                  screenshots=len(images))
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
