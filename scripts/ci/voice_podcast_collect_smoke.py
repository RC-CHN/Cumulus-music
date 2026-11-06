"""Real Linux WebKitGTK vinyl podcast card and sealed mutation receipts."""
import argparse
import json
import os
from pathlib import Path
import sqlite3

from podcast_scene_home_smoke import CHECK, DETAIL, DIRECTORY, EXPERIMENT, MEDIA, RESUME, read, requests
from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

LABELS = (
    'ready', 'subscribed', 'uncollect_confirmation', 'cancel_kept', 'unsubscribed',
    'linked_detail', 'reopened', 'owned_after_switch',
)
MEMBERSHIP = '/api/djradio/get/subed'
RADIO = '/api/djradio/v3/get'
PROGRAMS = '/api/v6/dj/program/byradio'
LIKED = '/api/content/my/liked/voice'
SUB = '/api/djradio/sub'
UNSUB = '/api/djradio/unsub'
WRITES = (SUB, UNSUB)


def exact_writes(trace):
    return [row for row in trace if row['path'] in WRITES]


def verify_playback_wire(calls, resume_radios=('80', '81')):
    assert [path for path, _ in calls[:2]] == [EXPERIMENT, DIRECTORY], calls
    assert requests(calls, EXPERIMENT) == [{'expNames': 'podcast_tab_better'}], calls
    assert requests(calls, DIRECTORY) == [{'blockCode': 'VOICE_HOMEPAGE_BLOCK_SCENE_FM'}], calls
    assert all(path in (EXPERIMENT, DIRECTORY, CHECK, MEDIA, DETAIL, RESUME) for path, _ in calls), calls
    assert [str(row['id']) for row in requests(calls, DETAIL)] == ['601', '602'], calls
    assert [str(row['voiceId']) for row in requests(calls, CHECK)] == ['601', '602'], calls
    assert [str(json.loads(row['ids'])[0]) for row in requests(calls, MEDIA)] == ['1601', '1602'], calls
    for voice in ('601', '602'):
        permission = next(i for i, (path, row) in enumerate(calls)
                          if path == CHECK and str(row['voiceId']) == voice)
        media = next(i for i, (path, row) in enumerate(calls)
                     if path == MEDIA and str(json.loads(row['ids'])[0]) == str(1000 + int(voice)))
        assert permission < media, calls
    assert all(row['radioId'] in resume_radios for row in requests(calls, RESUME)), calls


def verify_trace(trace):
    expected_writes = [
        {'path': SUB, 'body': {'id': '80'}, 'before': False, 'after': True},
        {'path': UNSUB, 'body': {'id': '80'}, 'before': True, 'after': False},
    ]
    assert exact_writes(trace) == expected_writes, trace
    assert all(row['path'] in (MEMBERSHIP, RADIO, PROGRAMS, LIKED, *WRITES) for row in trace), trace
    memberships = [row for row in trace if row['path'] == MEMBERSHIP]
    assert memberships and all(row['body'] == {'targetUserId': '2', 'limit': 1000} for row in memberships), trace
    assert any(not row['before'] for row in memberships), trace
    assert all(row['body'] == {'id': '80'} for row in trace if row['path'] == RADIO), trace
    route_read = [row for row in trace if row['path'] == PROGRAMS]
    assert len(route_read) == 1 and route_read[0]['body']['radioId'] == '80', trace
    for expected in expected_writes:
        index = trace.index(expected)
        before = expected['before']
        after = expected['after']
        preceding = [r for r in trace[:index] if r['path'] == RADIO]
        assert preceding and preceding[-1]['before'] is before, trace
        following = [r for r in trace[index + 1:trace.index(expected_writes[1])
                     if expected['path'] == SUB else len(trace)] if r['path'] == RADIO]
        assert len(following) >= 2 and all(r['before'] is after for r in following[:2]), trace


def verify_journal(root):
    db = root / 'run/state/core.sqlite3'
    with sqlite3.connect(db.as_uri() + '?mode=ro', uri=True) as conn:
        rows = conn.execute(
            'SELECT owner,key,value FROM state WHERE kind=? ORDER BY owner,key',
            ('podcast-mutations-v1',),
        ).fetchall()
    assert len(rows) == 1 and rows[0][:2] == ('2', 'podcast:subscribe:80'), rows
    assert json.loads(rows[0][2]) == {
        'action': 'subscribe', 'id': '80', 'before': True, 'desired': False,
        'acknowledged': True, 'observed': False, 'phase': 'confirmed',
    }, rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    require_xvfb()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_PODCAST_COLLECT_PROOF='1',
               CUMULUS_PODCAST_SCENE_HOME_PROOF='automatic',
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve()),
               LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    isolated_profile(env, root)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, (root / 'host.log').read_text(encoding='utf-8')
    complete = read(output / 'complete.json')
    stages = {p.stem.split('_', 2)[2]: read(p) for p in sorted(output.glob('ux_*.json'))}
    assert tuple(stages) == LABELS and complete['ok'] and complete['stageCount'] == len(LABELS), complete
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['physicalAudioVerified'] is False
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    ready = stages['ready']
    baseline = ready['state']
    assert baseline['state'] == 'paused' and baseline['position'] > .2, baseline
    assert baseline['radio'] == {'id': '80', 'name': '合成来访播客',
                                 'ownedByViewer': False, 'subscriberCount': 42}, baseline
    traces = [stages[label]['voicePodcastCollectTrace'] for label in LABELS]
    assert all(before == after[:len(before)] for before, after in zip(traces, traces[1:])), traces
    assert traces[-1] == complete['voicePodcastCollectTrace'], complete
    assert [len(exact_writes(t)) for t in traces] == [0, 1, 1, 1, 2, 2, 2, 2], traces
    assert not any(row['path'] == RADIO for row in traces[0]), ready
    assert not any(row['path'] == PROGRAMS for row in traces[4]), stages['unsubscribed']
    assert stages['uncollect_confirmation']['card']['confirmCancelFocused'], stages['uncollect_confirmation']
    assert '确定不再收藏该播单？' in stages['uncollect_confirmation']['card']['confirm'], stages['uncollect_confirmation']
    assert stages['cancel_kept']['card']['confirm'] is None, stages['cancel_kept']
    assert [r for r in traces[1] if r['path'] == RADIO] == [r for r in traces[3] if r['path'] == RADIO], traces
    assert stages['linked_detail']['page'] == '合成来访播客' and not stages['linked_detail']['card']['shown']
    assert stages['reopened']['card']['openEnabled'], stages['reopened']
    for label in LABELS[:-1]:
        stage = stages[label]
        assert stage['actual'] and stage['queue'] == ready['queue'], stage
        assert all(stage['state'][key] == baseline[key] for key in
                   ('voice', 'song', 'state', 'playId', 'entryId', 'queueRevision', 'position')), stage
        assert not stage['state']['seeking'] and not stage['state']['error'], stage
        assert 'SYNTHETIC_RADIO_PRIVATE' not in json.dumps(stage, ensure_ascii=False), stage
    own = stages['owned_after_switch']
    assert own['actual'] and own['staleRejected'] == 'stale' and own['ownRejected'], own
    assert own['staleRouteObservedVoice'] == '602' and own['page'] == '合成来访播客', own
    assert own['state']['state'] == 'playing' and own['state']['voice'] == '602' and own['state']['song'] == '1602', own
    assert own['state']['playId'] != baseline['playId'] and own['state']['entryId'] != baseline['entryId'], own
    assert own['state']['position'] > .2 and not own['state']['error'], own
    assert own['state']['radio'] == {'id': '81', 'name': '合成自有播客',
                                   'ownedByViewer': True, 'subscriberCount': 7}, own
    assert own['card']['shown'] and own['card']['openEnabled'] and not own['card']['subscribeEnabled'], own
    assert own['queue']['entries'] == ready['queue']['entries'], own
    assert len(own['commands']) == 2 and [c['input']['command']['expected'] for c in own['commands']] == [False, True]
    verify_trace(complete['voicePodcastCollectTrace'])
    verify_playback_wire(complete['calls'])
    verify_journal(root)
    pngs = list(output.glob('ux_*.png'))
    assert len(pngs) == (len(LABELS) if args.screenshots else 0), pngs
    report = {'actualWebView': True, 'actualNullAudio': True, 'stages': len(LABELS), 'screenshots': len(pngs),
              'syntheticSubscribeWrites': 2, 'accountScopedReadback': True, 'staleAndOwnRejected': True,
              'cancelSentNoWrite': True, 'preservedCurrentPlayback': True, 'staleRouteRejected': True,
              'realMusicRequests': 0, 'realAccountWrites': 0, 'physicalAudioVerified': False}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
