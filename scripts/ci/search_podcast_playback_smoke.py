"""Sealed Linux WebView search podcast resume and native queue preservation."""
import argparse
import json
import os
from pathlib import Path
import sys

from window_manager import launch

STATES = ('best_resumes', 'regular_pauses', 'denied_keeps_queue', 'regular_continues')
V3_STATES = ('regular_resumes', 'hot_core_pauses', 'denied_keeps_queue', 'hot_core_continues')
SEARCH = {'keyword': '合成播单续播', 'scene': 'normal', 'needCorrect': True,
          'channel': 'typing', 'cursor': '{"page":0,"ignoreBlocks":[],"traceId":""}', 'bizQueryInfo': ''}
DETAIL = "/api/dj/program/detail"
BYRADIO = '/api/v6/dj/program/byradio'
CHECK = '/api/dj/play/url/check'
URL = '/api/song/enhance/player/url/v1'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--v3', action='store_true', help='use signed search V3 and its hot-core AudioCard')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'linux':
        parser.error('this real WebView recipe is Linux only')
    assert len(Path('/proc/net/route').read_text().splitlines()) == 1, 'run in the sealed no-route network namespace'
    stages = V3_STATES if args.v3 else STATES
    binary, root = args.binary.resolve(strict=True), args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    repo = Path(__file__).resolve().parents[2]
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_')) and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_SEARCH_UI_PROOF='1',
               CUMULUS_SEARCH_PODCAST_PLAYBACK_PROOF='1',
               CUMULUS_UX_SCRIPT=str(repo / 'desktop/examples/search_podcast_playback_ui_probe.js'),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
               LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if args.v3:
        env['CUMULUS_SEARCH_V3_PROOF'] = '1'
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        folder = root / key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)

    out = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(binary, env, log, out, args.screenshots)
    failed = out / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    complete = read(out / 'complete.json')
    assert code == 0 and complete['ok'] and complete['stageCount'] == len(stages)
    rows = {p.stem.split('_', 2)[2]: read(p) for p in sorted(out.glob('ux_*.json'))}
    assert tuple(rows) == stages
    identities = set()
    states = ('playing', 'paused', 'paused', 'playing')
    positions = []
    search_path = '/api/search/pc/complex/page' + ('/v3' if args.v3 else '')
    result_paths = (['/api/search/pc/result/tab'] if args.v3 else []) + [search_path]
    search_args = {key: value for key, value in SEARCH.items() if not args.v3 or key != 'cursor'}
    best_section = 'search_block_hot_result_core_resources:0' if args.v3 else 'search_block_best_match:0'
    for index, name in enumerate(stages):
        row = rows[name]
        native = row['nativePlayer']
        dom = row['observed']['current']
        identities.add((native['epoch'], native['playId'], native['entryId'], native['queueRevision']))
        assert native['state'] == dom['state'] == states[index] and native['error'] is None
        assert (native['playId'], native['entryId'], native['queueRevision']) == (
            dom['playId'], dom['entryId'], dom['queueRevision'])
        assert native['track']['voiceId'] == '1' and native['track']['id'] == '1001'
        assert native['queueLen'] == 251 and native['mode'] == 'sequential'
        assert native['origin'] == {'kind': 'podcast', 'id': '7'}
        assert row['bestSection'] == best_section
        assert row['matchingBest'] == row['matchingRegular'] == (
            ('播放' if states[index] == 'paused' else '暂停') + ' 合成续播播单')
        positions.append(dom['position'])
        calls = row['searchPodcastCalls']
        assert [call['path'] for call in calls] == result_paths + [
            BYRADIO, BYRADIO, DETAIL, CHECK, URL
        ] + ([BYRADIO] if index >= 2 else [])
        if args.v3:
            assert calls[0]['args'] == {'keyword': SEARCH['keyword']}
        assert calls[len(result_paths) - 1]['args'] == search_args
        playback = calls[len(result_paths):]
        assert [call['args'] for call in playback[:2]] == [
            {'radioId': '7', 'offset': offset, 'limit': 250, 'asc': False} for offset in (0, 250)
        ]
        assert playback[2]['args'] == {'id': '1'} and not playback[2]['cached']
        assert playback[3]['args']['voiceId'] == '1' and not playback[3]['cached']
        assert playback[4]['args'] == {'ids': '["1001"]', 'level': 'standard', 'encodeType': 'aac',
                                       'immerseType': 'c51', 'trialMode': -1} and not playback[4]['cached']
        if index == 2:
            assert row['observed']['message'] == '网络请求失败，请稍后重试'
        if index >= 2:
            assert playback[5]['args'] == {'radioId': '8', 'offset': 0, 'limit': 250}
        if index in (0, 3):
            first = row['observed']['first']
            assert first['playId'] == dom['playId'] and first['entryId'] == dom['entryId']
            assert 18.8 <= first['position'] < dom['position'] - .12
    assert len(identities) == 1 and positions[1] == positions[2] and positions[3] > positions[2]
    assert complete['nativePlayer']['state'] == 'idle'
    assert complete['searchPodcastCalls'] == rows[stages[-1]]['searchPodcastCalls']
    signed = complete['signedSearch']
    assert signed['transport'] == 'MusicHttpClient' and signed['syntheticOnly'] and signed['sameProfileAccountRestore']
    signed_paths = ['/api/w/nuser/account/get']
    if args.v3:
        signed_paths.append('/api/rtrs/abt/front/expinfo/list')
    assert [call['path'] for call in signed['calls']] == signed_paths
    assert all(call['signedPcContextVerified'] and call['sameAccountCredential'] and call['responseCode'] == 200
               for call in signed['calls'])
    locale = read(out / 'locale.json')
    assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}, locale
    screenshots = len(list(out.glob('ux_*.png')))
    assert screenshots == (len(stages) if args.screenshots else 0)
    report = dict(ok=True, actualWebView=True, actualProductionUiAndIpc=True, platform='linux',
                  searchVersion='v3' if args.v3 else 'v1',
                  nullOutput=True, states=stages, screenshots=screenshots, locale=locale,
                  sameNativeIdentity=True, queueLength=251, positions=positions, wire=complete['searchPodcastCalls'],
                  signedAssignmentReads=len(signed_paths) - 1,
                  networkNamespaceWithoutRoutes=True, syntheticAccount=True, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
