"""Real Linux WebKitGTK VoicePage shelf, scoped local paging and stale native reply."""
import argparse
import json
import os
from pathlib import Path

from podcast_scene_home_smoke import RESUME, read, requests
from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from voice_podcast_collect_smoke import LIKED, PROGRAMS, RADIO, verify_playback_wire
from window_manager import launch

LABELS = ('closed', 'first_page', 'short_page', 'wrapped', 'linked_detail', 'new_episode',
          'old_rejected', 'nested_detail', 'nested_back')
ENDPOINT = '/api/voice/play/page/rcmd'
FIRST = [str(value) for value in range(90, 96)]
SHORT = [str(value) for value in range(96, 100)]
NEW = [str(value) for value in range(120, 124)]
REQUESTS = [[ENDPOINT, {'voiceId': voice}] for voice in ('601', '601', '602')]


def assert_page(stage, number, total, expected):
    shelf = stage['shelf']
    assert shelf['visible'] and shelf['page'] == str(number) and shelf['pageCount'] == str(total), shelf
    assert shelf['ids'] == expected and '听了该声音的人也在听' in shelf['title'], shelf


def verify(stages, complete):
    assert tuple(stages) == LABELS and complete['ok'] and complete['stageCount'] == len(LABELS), complete
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0, complete
    assert complete['physicalAudioVerified'] is False
    assert not stages['closed']['shelf']['visible'] and not stages['closed']['reads']
    assert_page(stages['first_page'], 1, 2, FIRST)
    assert_page(stages['short_page'], 2, 2, SHORT)
    assert_page(stages['wrapped'], 1, 2, FIRST)
    assert stages['short_page']['shelf']['nextFocused'] and stages['wrapped']['shelf']['nextFocused']
    assert not stages['linked_detail']['shelf']['visible']
    assert stages['linked_detail']['detail'] == '合成推荐播单 98'
    assert stages['linked_detail']['wrongPage'] == stages['linked_detail']['wrongRadio'] == 'stale'
    assert_page(stages['new_episode'], 1, 1, NEW)
    assert_page(stages['old_rejected'], 1, 1, NEW)
    assert stages['old_rejected']['stalePage'] == stages['old_rejected']['staleTarget'] == 'stale'
    assert not stages['nested_detail']['shelf']['visible'] and not stages['nested_back']['shelf']['visible']
    assert stages['nested_detail']['detail'] == '合成推荐播单 121'
    assert stages['nested_back']['detail'] == '合成推荐播单 98'
    parked = stages['nested_detail']['beforeNavigation']
    assert parked['state'] == 'paused' and not parked['error'] and not parked['seeking'], parked
    assert parked['position'] >= stages['old_rejected']['player']['position'], parked
    assert stages['nested_detail']['player'] == parked == stages['nested_back']['player']
    assert stages['old_rejected']['queue'] == stages['nested_detail']['queue'] == stages['nested_back']['queue']
    baseline = stages['closed']['player']
    assert baseline['state'] == 'paused' and baseline['position'] > .2, baseline
    assert baseline['voice'] == '601' and baseline['song'] == '1601' and baseline['playId'], baseline
    queue = stages['closed']['queue']
    assert [entry[1] for entry in queue['entries']] == ['601', '602'], queue
    for label in LABELS:
        stage = stages[label]
        assert stage['actual'] and not stage['player']['error'], stage
        assert stage['queue']['entries'] == queue['entries'], stage
        if label in LABELS[:5]:
            assert stage['queue'] == queue and stage['player'] == baseline, stage
            assert not stage['player']['seeking']
        else:
            current = stage['player']
            assert current['state'] == ('playing' if label in LABELS[5:7] else 'paused'), current
            assert current['position'] > .2, current
            assert current['voice'] == '602' and current['song'] == '1602', current
            assert current['playId'] != baseline['playId'] and current['entryId'] != baseline['entryId'], current
            assert current['entryId'] == queue['entries'][1][0], stage
    assert all(stages[label]['player']['playId'] == parked['playId'] for label in LABELS[5:])
    traces = [stages[label]['voiceRecommendationsTrace'] for label in LABELS]
    assert [trace['requests'] for trace in traces] == [[], REQUESTS[:1], REQUESTS[:1],
                                                      REQUESTS[:1], REQUESTS[:1], *[REQUESTS] * 4], traces
    assert [trace['oldPending'] for trace in traces] == [False] * 5 + [True] * 4, traces
    def destinations(trace):
        return [row for row in trace['routes'] if row[0] != LIKED]
    assert all(not destinations(trace) for trace in traces[:4]), traces
    def assert_destinations(trace, ids):
        route = destinations(trace)
        assert len(route) == 2 * len(ids), route
        assert [body for path, body in route if path == RADIO] == [{'id': id_} for id_ in ids], route
        assert [body for path, body in route if path == PROGRAMS] == [
            {'radioId': id_, 'limit': 200, 'offset': 0, 'asc': True} for id_ in ids
        ], route
    for trace in traces[4:7]:
        assert_destinations(trace, ['98'])
    assert_destinations(traces[7], ['98', '121'])
    assert_destinations(traces[8], ['98', '121', '98'])
    assert traces[-1] == complete['voiceRecommendationsTrace'], traces
    liked_bootstrap = [[LIKED, {'limit': 100, 'likeTime': 0}]]
    # The account bootstrap reads likes once; each radio navigation or Back
    # refreshes that same read once alongside detail and programs.
    assert all([row for row in trace['routes'] if row[0] == LIKED] == liked_bootstrap * count
               for trace, count in zip(traces, [1] * 4 + [2] * 3 + [3, 4])), traces
    calls = stages['old_rejected']
    first, stale, second = calls['reads']
    old = first['result']
    new = second['result']
    epoch = first['args'][0]
    assert calls['targets'] == stages['linked_detail']['targets'] == [{
        'target': {'epoch': epoch, 'playId': old['playId'], 'voiceId': '601',
                   'receipt': old['receipt'], 'page': 2, 'radioId': '98'},
        'result': '98',
    }], calls
    nested_target = {'target': {'epoch': epoch, 'playId': new['playId'], 'voiceId': '602',
                                'receipt': new['receipt'], 'page': 1, 'radioId': '121'},
                     'result': '121'}
    for label in LABELS[7:]:
        assert stages[label]['targets'] == [*calls['targets'], nested_target], stages[label]
        assert stages[label]['reads'] == calls['reads'] and stages[label]['pages'] == calls['pages'], stages[label]
    assert [call['args'] for call in (first, stale, second)] == [
        [epoch, old['playId']], [epoch, old['playId']], [epoch, new['playId']],
    ], calls
    assert stale['error'] == 'stale' and 'result' not in stale, stale
    assert old['playId'] == baseline['playId'] and old['voiceId'] == '601', old
    assert new['playId'] == calls['player']['playId'] and new['voiceId'] == '602', new
    assert old['receipt'] and new['receipt'] and old['receipt'] != new['receipt'], (old, new)
    assert [call['args'] for call in calls['pages']] == [
        [epoch, old['playId'], old['receipt']],
        [epoch, old['playId'], old['receipt']],
        [epoch, old['playId'], old['receipt']],
        [epoch, new['playId'], new['receipt']],
    ], calls
    replies = [call['result'] for call in calls['pages']]
    assert [reply['page'] for reply in replies] == [2, 1, 2, 1], replies
    for reply, ids, bound in zip([old, new, *replies], [FIRST, NEW, SHORT, FIRST, SHORT, NEW],
                                 [old, new, old, old, old, new]):
        assert all(reply[key] == bound[key] for key in ('voiceId', 'playId', 'receipt', 'pageCount')), reply
        assert [card['id'] for card in reply['cards']] == ids, reply
        assert all(card == {'id': id_, 'name': f'合成推荐播单 {id_}', 'playCount': 10000 + int(id_),
                            'keys': ['name', 'playCount', 'radioId']}
                   for card, id_ in zip(reply['cards'], ids)), reply
    verify_playback_wire(complete['calls'], resume_radios=('80', '98', '121'))
    detail_resume = [row for row in requests(complete['calls'], RESUME) if row['radioId'] != '80']
    assert detail_resume == [{'radioId': id_} for id_ in ('98', '121', '98')], detail_resume


def verify_results(root, screenshots):
    output = root / 'run/first'
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    stages = {p.stem.split('_', 2)[2]: read(p) for p in sorted(output.glob('ux_*.json'))}
    verify(stages, read(output / 'complete.json'))
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    pngs = list(output.glob('ux_*.png'))
    assert len(pngs) == (len(LABELS) if screenshots else 0), pngs
    return {'actualWebView': True, 'actualNullAudio': True, 'stages': len(LABELS), 'screenshots': len(pngs),
            'nativeSignedFixtureReads': 3, 'localPageActions': 4, 'linkedRadio': '98',
            'contextualRadioBack': ['98', '121', '98'],
            'preservedCurrentPlayback': True, 'lateReadRejected': True, 'stalePageRejected': True,
            'wrongPageTargetRejected': True, 'offPageTargetRejected': True, 'staleTargetRejected': True,
            'realMusicRequests': 0, 'realAccountWrites': 0, 'physicalAudioVerified': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--script', type=Path, help='explicit probe-only script for an existing exact binary')
    args = parser.parse_args()
    require_xvfb()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_RECOMMENDATIONS_PROOF='1',
               CUMULUS_PODCAST_SCENE_HOME_PROOF='automatic',
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
               LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if args.script:
        env['CUMULUS_UX_SCRIPT'] = str(args.script.resolve(strict=True))
    isolated_profile(env, root)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, (root / 'host.log').read_text(encoding='utf-8')
    report = verify_results(root, args.screenshots)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
