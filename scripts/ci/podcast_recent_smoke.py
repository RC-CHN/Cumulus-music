"""Run a sealed My Podcasts flow in the actual desktop WebView."""
import argparse
import json
import math
import os
import sys
from pathlib import Path

from window_manager import launch

PROGRESS = '/api/dj/playrecord/upload'
PROGRESS_STAGES = {
    'started': 0,
    'first_paused_seek': 0,
    'episode_departure': 1,
    'second_paused_seek': 1,
    'song_departure': 2,
    'before_eof': 2,
    'natural_eof': 3,
    'opt_out': 3,
    'private_paused_seek': 3,
    'private_stopped': 3,
}


def check_progress(output, stages, complete):
    assert set(stages) == set(PROGRESS_STAGES), stages
    first = stages['first_paused_seek']['player']
    second = stages['second_paused_seek']['player']
    end = stages['natural_eof']['player']
    assert first['state'] == second['state'] == 'paused'
    assert (first['voice'], first['track']) == ('501', '1501')
    assert (second['voice'], second['track']) == ('503', '1503')
    assert stages['episode_departure']['player']['state'] == 'playing'
    assert stages['song_departure']['player']['track'] == '1'
    assert (end['state'], end['voice'], end['duration']) == ('ended', '504', 30)
    assert stages['before_eof']['player']['position'] < end['duration']
    expected = [
        [PROGRESS, {'programId': 501, 'listenLocation': 7000, 'isListened': False}],
        [PROGRESS, {'programId': 503, 'listenLocation': 12000, 'isListened': False}],
        [PROGRESS, {'programId': 504, 'listenLocation': 30000, 'isListened': True}],
    ]
    assert [math.floor(p['position']) * 1000 for p in (first, second)] == [7000, 12000], (first, second)
    for stage, count in PROGRESS_STAGES.items():
        assert stages[stage]['progressUploads'] == expected[:count], (stage, stages[stage])
    assert complete['progressUploads'] == expected, complete
    for stage in ('started', 'first_paused_seek', 'second_paused_seek', 'before_eof', 'private_paused_seek'):
        assert stages[stage]['player']['timelineMode'] == 'full'
    assert stages['opt_out']['syncEnabled'] is False
    private = stages['private_stopped']
    assert private['player']['state'] == 'idle' and private['syncEnabled'] is False
    assert private['localVoice'] == '506'
    assert abs(private['localPosition'] - stages['private_paused_seek']['player']['position']) < .12
    if sys.platform == 'linux':
        locale = json.loads((output / 'locale.json').read_text(encoding='utf-8'))
        assert locale['numeric'] == 'C'
        assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
                   for key in ('messages', 'characters')), locale



def check_badges(output, stages, complete, screenshots):
    route = '/api/djradio/newcount/clear/byradio'
    detail = '/api/djradio/v3/get'
    listing = '/api/my/podcast/page/list'
    initial = [
        {'id': '81', 'text': '99+', 'label': '108 集新节目'},
        {'id': '82', 'text': '4', 'label': '4 集新节目'},
    ]
    expected = {
        # stage: (badge layout, visible detail, native reads, native clears)
        'initial_badges': (initial, None, 0, 0),
        'ordinary_detail': ([], '合成确认播客', 1, 0),
        'ordinary_refresh': ([], '合成确认播客', 2, 0),
        'confirmed_detail': ([], '合成确认播客', 3, 1),
        'confirmed_back': (initial[1:], None, 3, 1),
        'lost_detail': ([], '合成待核对播客', 4, 2),
        'lost_back': (initial[1:], None, 4, 2),
        'no_replay': (initial[1:], None, 5, 2),
    }
    assert set(stages) == set(expected)
    known_clears = [[route, {'radioId': '81'}], [route, {'radioId': '82'}]]
    known_details = [{'id': id_} for id_ in ['81', '81', '81', '82', '82']]
    native_trace = complete['podcastBadgeTrace']
    assert [row for row in native_trace if row[0] == route] == known_clears
    assert [body for target, body in native_trace if target == detail] == known_details
    assert [body for target, body in native_trace if target == listing] == [
        {'selectType': 1, 'sortType': 1},
    ] * 5
    assert all(row[0] in {route, detail, listing} for row in native_trace)
    assert complete['detailReads'] == ['81', '81', '81', '82', '82']
    clear_outcomes = [['81', 'acknowledged'], ['82', 'network'], ['82', 'stale']]
    assert complete['clearOutcomes'] == clear_outcomes
    for stage, (badges, name, reads, clears) in expected.items():
        row = stages[stage]
        assert (row['badges'], row['detail'], row['readCount']) == (badges, name, reads), stage
        assert [body for target, body in row['podcastBadgeTrace'] if target == detail] == known_details[:reads]
        assert [call for call in row['podcastBadgeTrace'] if call[0] == route] == known_clears[:clears]
        assert row['clearOutcomes'] == clear_outcomes[:3 if stage == 'no_replay' else clears]
    check_library_capture(output, screenshots)


def check_library_capture(output, screenshots):
    if sys.platform == 'linux':
        locale = json.loads((output / 'locale.json').read_text(encoding='utf-8'))
        assert locale['numeric'] == 'C'
        assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
                   for key in ('messages', 'characters')), locale
    if screenshots:
        names = {path.stem for path in output.glob('ux_*.json')}
        assert {path.stem for path in output.glob('ux_*.png')} == names
        for stage in names:
            paint = json.loads((output / f'paint-{stage}.json').read_text(encoding='utf-8'))
            assert len(paint['samples']) == 2 and all(sample['ok'] for sample in paint['samples']), stage

def check_recommendations(output, stages, complete, screenshots):
    assert set(stages) == {
        'recommended_first', 'recommended_next', 'recommended_playing', 'recommended_paused',
        'recommended_resumed', 'recommended_detail', 'recommended_back',
        'recommendation_menu_collectible', 'recommendation_menu_collect_confirm',
        'recommendation_menu_subscribed',
        'recommendation_menu_share', 'recommendation_menu_copy', 'recommendation_menu_next',
    }, stages
    assert complete['detailReads'] == ['81']
    assert stages['recommended_detail']['detail'] == '合成确认播客'
    assert complete['first'][0] == '81' and complete['lastGroup'][-1] == '86'
    assert len(complete['first']) == len(complete['lastGroup']) < 6
    trace = complete['podcastBadgeTrace']
    before = stages['recommended_back']['podcastBadgeTrace']
    assert trace[:len(before)] == before, trace
    assert sorted(row[0] for row in before) == sorted([
        '/api/my/podcast/page/list', '/api/my/podcast/page/list',
        '/api/djradio/my/radio/recommend', '/api/djradio/v3/get',
        '/api/v6/dj/program/byradio', '/api/dj/program/detail', '/api/dj/play/url/check',
        '/api/song/enhance/player/url/v1',
    ]), before
    tail = trace[len(before):]
    assert tail[0] == ['/api/v6/dj/program/byradio', {'radioId':'82', 'offset':0, 'limit':250}], tail
    # The unchanged ordinary actor may authorize/prefetch the new next row
    # once. It cannot become the active decoder while the old row is paused.
    assert [row[0] for row in tail[1:]] == [
        '/api/dj/program/detail', '/api/dj/play/url/check', '/api/song/enhance/player/url/v1',
    ][:len(tail) - 1], tail
    for path, args in tail[1:]:
        if path == '/api/dj/program/detail':
            assert args == {'id':'9082'}, tail
        elif path == '/api/dj/play/url/check':
            assert args['voiceId'] == '9082', tail
        else:
            assert list(map(str, json.loads(args['ids']))) == ['10082'], tail
    next_player = stages['recommendation_menu_next']['player']
    assert (next_player['state'], next_player['queueLen'], next_player['track']['voiceId']) == (
        'paused', 2, '9081',
    )
    assert next_player['playId'] == stages['recommended_playing']['player']['playId']
    for suffix, state in [('playing', 'playing'), ('paused', 'paused'), ('resumed', 'playing')]:
        row = stages['recommended_' + suffix]
        player = row['player']
        assert player['state'] == state and player['track']['voiceId'] == '9081'
        assert player['origin'] == {'kind': 'podcast', 'id': '81'}
        assert player['mode'] == 'sequential' and player['queueLen'] == 1 and row['detail'] is None
        assert len([call for call in row['podcastBadgeTrace']
                    if call[0] == '/api/v6/dj/program/byradio']) == 1
    assert [row[1] for row in trace if row[0] == '/api/djradio/my/radio/recommend'] == [{}]
    check_library_capture(output, screenshots)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--progress', action='store_true', help='run the sealed episode progress proof')
    modes.add_argument('--badges', action='store_true', help='run the sealed subscription badge proof')
    modes.add_argument('--recommendations', action='store_true', help='run the sealed recommendation shelf proof')
    args = parser.parse_args()
    library_proof = args.badges or args.recommendations
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    run = root / 'run'
    name = ('podcast_progress_ui_probe.js' if args.progress else 'podcast_badges_ui_probe.js'
            if library_proof else 'podcast_recent_ui_probe.js')
    script = Path(__file__).resolve().parents[2] / 'desktop/examples' / name
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_UX_SCRIPT=str(script))
    if args.progress:
        env['CUMULUS_PODCAST_PROGRESS_PROOF'] = '1'
    if args.badges:
        env['CUMULUS_PODCAST_BADGES_PROOF'] = '1'
    if args.recommendations:
        env['CUMULUS_PODCAST_RECOMMENDATIONS_PROOF'] = '1'
    if (args.progress or library_proof) and sys.platform == 'linux':
        env['LC_ALL'] = 'en_US.UTF-8'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    output = run / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exited {status}'
    host = (root / 'host.log').read_text(encoding='utf-8')
    assert 'panicked at' not in host, host
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['realMusicRequests'] == complete['remoteWrites'] == 0
    stages = {path.stem.split('_', 2)[2]: json.loads(path.read_text(encoding='utf-8'))
              for path in output.glob('ux_*.json')}
    assert len(stages) == complete['stageCount'] and all(s['actual'] for s in stages.values())
    if args.progress:
        check_progress(output, stages, complete)
    elif args.recommendations:
        check_recommendations(output, stages, complete, args.screenshots)
    elif args.badges:
        check_badges(output, stages, complete, args.screenshots)
    else:
        assert list(stages['first_page']['ids']) == list(stages['back']['ids'])
        assert stages['next_error']['size'] == 18 and stages['next_page']['size'] == 20
        assert abs(stages['scrolled_back']['scroll'] - stages['next_page']['scroll']) < 2
        assert '520' in stages['scrolled_back']['ids']
        assert stages['playing']['state'] == 'playing' and stages['paused']['state'] == 'paused'
        calls = complete['calls']
        requests = lambda route: [body for target, body in calls if target == route]
        cursor = {'updateTime': 1_700_000_000_123}
        assert requests('/api/social/my/recent/update/voice/list/v1') == [{}, {}, cursor, cursor, {}], calls
        detail = '/api/dj/program/detail'
        permission = '/api/dj/play/url/check'
        media = '/api/song/enhance/player/url/v1'
        by_voice = {}
        for route, body in calls:
            if route == detail:
                id_ = body['id']
            elif route == permission:
                id_ = body['voiceId']
            elif route == media:
                id_ = str(int(json.loads(body['ids'])[0]) - 1000)
                assert body['level'] == 'standard', body
            else:
                continue
            by_voice.setdefault(id_, []).append(route)
        assert by_voice['501'] == [detail, permission, media], calls
        assert by_voice['503'] == [detail, detail, permission, media], calls
        assert by_voice['504'] == [detail, permission, media], calls
        # Normal native next-track prefetch can overlap the final screenshot.
        assert set(by_voice) <= {'501', '503', '504', '506'}, calls
        if '506' in by_voice:
            assert by_voice['506'] == [detail, permission, media][:len(by_voice['506'])], calls
    pictures = len(list(output.glob('ux_*.png')))
    assert not args.screenshots or pictures == len(stages)
    runtime = {} if args.badges else {'decoder': 'libmpv', 'audioOutput': 'null'}
    result = {'actualWebView': True, 'screenshots': pictures, 'states': list(stages), **runtime, **complete}
    (root / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n',
                                    encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
