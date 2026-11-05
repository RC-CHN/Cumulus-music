"""Verify the real desktop search row chooses an authorized local publication first."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

from playback_power_smoke import windows_runtime
from window_manager import launch

STATES = ('published_local', 'same_local_paused', 'corrupt_online_fallback')
SEARCH = {'keyword': '合成下载优先', 'scene': 'normal', 'needCorrect': True,
          'channel': 'typing', 'cursor': '{"page":0,"ignoreBlocks":[],"traceId":""}', 'bizQueryInfo': ''}
DOWNLOAD = {'id': '881', 'level': 'lossless', 'immerseType': 'c51'}
ONLINE = {'ids': '["881"]', 'level': 'standard', 'encodeType': 'aac', 'immerseType': 'c51', 'trialMode': 42}


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--local-marker', action='store_true',
                        help='require the verified local-source footer label and disabled quality changes')
    args = parser.parse_args()
    windows = sys.platform == 'win32'
    if sys.platform not in {'linux', 'win32'} or windows != bool(args.runtime) or (windows and args.screenshots):
        parser.error('Windows needs --runtime; screenshots are Linux only')
    binary, root = args.binary.resolve(strict=True), args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    repo = Path(__file__).resolve().parents[2]
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_')) and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_SEARCH_DOWNLOAD_ROW_PROOF='1',
               CUMULUS_UX_SCRIPT=str(repo / 'desktop/examples/search_download_row_ui_probe.js'))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if windows:
        stage, _ = windows_runtime(binary, args.runtime)
        env['MPV_DLL_DIR_CUMULUS'] = str(stage)
        dirs = ('APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP')
    else:
        dirs = ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR')
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    for key in dirs:
        folder = root / key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)

    out = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(binary, env, log, out, args.screenshots)
    failed = out / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    complete = read(out / 'complete.json')
    assert result == 0 and complete['ok'] and complete['stageCount'] == len(STATES)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    rows = {p.stem.split('_', 2)[2]: read(p) for p in sorted(out.glob('ux_*.json'))}
    assert tuple(rows) == STATES
    seed = (repo / 'core/tests/fixtures/stereo-pcm-source.flac').read_bytes()
    traces = [rows[name]['searchDownloadFixture'] for name in STATES]
    traces.append(complete['searchDownloadFixture'])
    publication = traces[0]['publication']
    assert publication['verified'] and publication['taskId'] and publication['bytes'] == len(seed)
    assert publication['sha256'] == hashlib.sha256(seed).hexdigest()
    for index, trace in enumerate(traces):
        fallback = index >= 2
        assert trace['search'] == [SEARCH] and trace['download'] == [DOWNLOAD] and trace['mediaReads'] == 1
        assert trace['publication'] == publication and trace['corrupted'] is fallback
        assert trace['online'] == ([ONLINE] if fallback else [])
        assert len(trace['loads']) == (2 if fallback else 1)
        local = trace['loads'][0]
        assert local['kind'] == 'published_flac' and local['codec'] == 'flac'
        assert local['nativeEntry'] > 0 and 1.9 <= local['duration'] <= 2.1 and local['start'] == 0
        if fallback:
            online = trace['loads'][1]
            assert online['kind'] == 'authorized_url' and online['codec'].startswith('pcm_')
            assert online['nativeEntry'] > 0 and online['nativeEntry'] != local['nativeEntry']
            assert online['duration'] == 30 and online['start'] == 0
    assert traces[0]['loads'] == traces[1]['loads'] == traces[2]['loads'][:1]
    assert traces[2]['loads'] == traces[3]['loads']
    for index, state in enumerate(STATES):
        row = rows[state]
        current = row['observed']['current']
        native = row['nativePlayer']
        assert native['epoch'] == current['epoch'] and native['state'] == ('paused' if index == 1 else 'playing')
        assert native['resourceKind'] == 'catalog' and native['track']['id'] == '881' and not native['track']['local']
        assert not native.get('downloadTaskId') and native['queueLen'] == 1 and native['error'] is None
        assert native['requestedQuality'] == 'standard' and native['actualQuality'] == ('lossless' if index < 2 else 'standard')
        assert (native['playId'], native['entryId'], native['queueRevision']) == (
            current['playId'], current['entryId'], current['queueRevision'])
        queue = row['nativeQueue']
        assert queue['current'] == native['entryId'] and [item['track']['id'] for item in queue['tracks']] == ['881']
        if index != 1:
            first = row['observed']['first']
            assert native['playId'] == first['playId'] and native['entryId'] == first['entryId']
            assert current['position'] > first['position'] + .14
        if args.local_marker:
            assert row['qualitySource'] == ('本地' if index < 2 else None)
            assert native.get('catalogFileSource', False) is (index < 2)
            assert native['canChangeQuality'] is (index == 2)
    local, paused, fallback = (rows[state]['nativePlayer'] for state in STATES)
    assert (local['playId'], local['entryId'], local['queueRevision']) == (paused['playId'], paused['entryId'], paused['queueRevision'])
    assert fallback['playId'] != local['playId'] and complete['nativePlayer']['state'] == 'idle'
    [target] = (root / 'search-download-media').glob('*.flac')
    assert target.read_bytes() == bytes(len(seed)), 'only the fixture publication was corrupted, with unchanged byte count'
    locale = None if windows else read(out / 'locale.json')
    if locale:
        assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}, locale
    screenshots = len(list(out.glob('ux_*.png')))
    assert screenshots == (len(STATES) if args.screenshots else 0)
    report = dict(ok=True, actualWebView=True, nativeNullPlaybackVerified=True, platform='windows' if windows else 'linux',
                  states=STATES, screenshots=screenshots, locale=locale, localMarkerVerified=args.local_marker,
                  localUrlAttempts=0, fallbackUrlAttempts=1, publication=publication, nativeLoads=traces[-1]['loads'],
                  realMusicRequests=0, realAccountWrites=0, physicalAudioOutputVerified=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
