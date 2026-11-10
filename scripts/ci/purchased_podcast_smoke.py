"""Real My Purchases WebView over a sealed podcast read/media fixture."""
import argparse
from contextlib import closing
from hashlib import sha256
import json
import os
from pathlib import Path
import sqlite3
import stat
import sys

from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

STAGES = ('mixed', 'selected', 'compact', 'normal', 'radio', 'parent', 'denied', 'more', 'ready',
          'cancelled', 'confirm', 'published')
PLAYBACK_STAGES = ('baseline', 'radio_rejected', 'denied', 'inserted', 'paused', 'resumed')
BOUGHT = '/api/voicelist/bought/list'
RADIO = '/api/djradio/v3/get'
PROGRAMS = '/api/v6/dj/program/byradio'
RESUME = '/api/dj/user/setting/resumeplay/get'
RIGHTS = '/api/dj/voices/download/check'
OPTIONS = '/api/dj/program/song/musics'
DETAIL = '/api/dj/program/detail'
URL = '/api/song/enhance/download/url/v1'
PLAY_RIGHTS = '/api/dj/play/url/check'
PLAY_URL = '/api/song/enhance/player/url/v1'
ORDER = ['radio:81', 'episode:81', 'episode:82']
CHOICES = [['standard', '普通（1.0M）', True], ['higher', '较高（2.0M）', False]]
SOURCE = Path(__file__).resolve().parents[2] / 'core/tests/fixtures/tag_audio/seed.mp3'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def verify(root, capture):
    output = root / 'run/first'
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(STAGES), complete
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    paths = sorted(output.glob('ux_*.json'))
    assert [p.stem for p in paths] == [f'ux_{i:02}_{name}' for i, name in enumerate(STAGES, 1)]
    stages = dict(zip(STAGES, map(read, paths), strict=True))
    assert all(stage['actual'] for stage in stages.values()), stages
    assert stages['mixed']['visibleRows'] == stages['selected']['visibleRows'] == ORDER
    for name, height, cover, gap in [('mixed', 64, 48, 8), ('compact', 56, 36, 0), ('normal', 64, 48, 8)]:
        assert stages[name]['visibleRows'] == ORDER
        assert stages[name]['geometry'] == dict(heights=[height] * 3, covers=[cover] * 3, header=40, gap=gap)
    assert stages['radio']['heading'] == '合成已购播单 81'
    assert stages['parent']['heading'] == '合成所属播单 71'
    assert stages['denied']['submitDisabled']
    assert stages['more']['visibleRows'] == [*ORDER, 'radio:90']
    assert stages['ready']['choices'] == CHOICES
    assert stages['confirm']['submitEnabled']
    assert stages['confirm']['choices'] == [
        ['standard', CHOICES[0][1], False], ['higher', CHOICES[1][1], True],
    ]
    for name in STAGES[:-1]:
        assert stages[name]['purchasedTrace']['mediaGets'] == 0
        assert all(path not in (DETAIL, URL) for path, _ in stages[name]['purchasedTrace']['calls'])
    calls = lambda name: stages[name]['purchasedTrace']['calls']
    assert all(calls(name) == [[BOUGHT, {'limit': 20, 'offset': 0}]]
               for name in ('mixed', 'selected', 'compact', 'normal'))
    assert [path for path, _ in calls('denied')] == [
        BOUGHT, RADIO, PROGRAMS, RESUME, RADIO, PROGRAMS, RESUME, RIGHTS,
    ]
    assert calls('denied')[-1] == [RIGHTS, {'voiceIdsStr': ['81']}]
    assert calls('more')[-1] == [BOUGHT, {'limit': 20, 'offset': 20}]
    assert calls('ready') == calls('cancelled')
    assert calls('ready')[len(calls('more')):] == [
        [RIGHTS, {'voiceIdsStr': ['82']}], [OPTIONS, {'ids': ['82']}],
    ]
    assert calls('confirm')[len(calls('ready')):] == [
        [RIGHTS, {'voiceIdsStr': ['82']}], [OPTIONS, {'ids': ['82']}],
    ]
    trace = complete['purchasedTrace']
    assert trace['mediaGets'] == 1 and trace['calls'] == calls('published')
    assert trace['calls'][len(calls('confirm')):] == [
        [DETAIL, {'id': '82'}], [DETAIL, {'id': '82'}], [RIGHTS, {'voiceIdsStr': ['82']}],
        [URL, {'id': '182', 'level': 'higher', 'immerseType': 'c51'}],
    ]
    assert [body for path, body in trace['calls'] if path == RADIO] == [{'id': '81'}, {'id': '71'}]

    records = []
    for db in (root / 'run/state').rglob('core.sqlite3'):
        with closing(sqlite3.connect(db.resolve().as_uri() + '?mode=ro', uri=True)) as connection:
            records.extend(json.loads(value) for value, in connection.execute(
                "SELECT value FROM state WHERE kind='download_tasks_v1'"))
    task, = records
    visible = stages['published']['task']
    assert visible == {'id': task['id'], 'voiceId': '82', 'mediaId': '182', 'quality': 'higher'}
    assert task['origin'] == {'kind': 'voice', 'voice_id': '82'} and task['song_id'] == '182'
    assert task['status'] == 'complete' and task['authorization_count'] == 1 and not task['encrypted']
    assert task['options'] == {
        'quality': 'higher', 'tags': False, 'cover': False, 'lyrics': False, 'container': 'audio',
    }
    published = Path(task['target']).resolve(strict=True)
    directory = (root / 'run/download-output').resolve(strict=True)
    assert any(parent.samefile(directory) for parent in published.parents)
    assert stat.S_ISREG(published.stat().st_mode) and published.suffix.lower() == '.mp3'
    assert sha256(published.read_bytes()).digest() == sha256(SOURCE.read_bytes()).digest()
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters'))
    pngs = sorted(output.glob('ux_*.png'))
    assert [p.stem for p in pngs] == ([p.stem for p in paths] if capture else [])
    return dict(ok=True, actualWebView=True, stages=list(STAGES), screenshots=len(pngs),
                syntheticVoice='82', syntheticMedia='182', actualQuality='higher',
                realMusicRequests=0, realAccountWrites=0, mediaGets=trace['mediaGets'])


def verify_playback(root, capture):
    output = root / 'run/first'
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(PLAYBACK_STAGES), complete
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    paths = sorted(output.glob('ux_*.json'))
    assert [p.stem for p in paths] == [
        f'ux_{i:02}_{name}' for i, name in enumerate(PLAYBACK_STAGES, 1)
    ]
    stages = dict(zip(PLAYBACK_STAGES, map(read, paths), strict=True))
    assert all(s['actual'] and s['visibleRows'] == ORDER and s['directCount'] == 0
               and s['purchasedTrace']['mediaGets'] == 0 for s in stages.values()), stages
    assert complete['purchasedTrace']['mediaGets'] == 0
    revision = stages['baseline']['pageRevision']
    assert len(revision) == 64 and all(s['pageRevision'] == revision for s in stages.values())
    assert stages['baseline']['replaceQueue'] and stages['baseline']['singleClickSelected']
    assert stages['radio_rejected']['radioError'] == 'invalid_input'

    native = {name: stage['native'] for name, stage in stages.items()}
    old, current = native['baseline'], native['inserted']
    assert old['kind'] == 'catalog' and old['song'] == '3' and old['voice'] is None
    assert old['playId'] and old['entryId'] == old['current'] == old['rows'][2]['entryId']
    assert old['position'] > .2 and old['mode'] == 'sequential'
    assert [r['song'] for r in old['rows']] == ['1', '2', '3']
    assert all(r['voice'] is None for r in old['rows'])
    stable = ('song', 'voice', 'kind', 'entryId', 'playId', 'queueRevision', 'current', 'rows', 'mode')
    for name in ('baseline', 'radio_rejected', 'denied'):
        assert native[name]['state'] == 'paused'
        assert all(native[name][key] == old[key] for key in stable), native
    assert current['kind'] == 'voice' and current['song'] == '182' and current['voice'] == '82'
    assert current['playId'] and current['playId'] != old['playId']
    assert current['queueRevision'] != old['queueRevision']
    assert current['entryId'] == current['current'] and current['mode'] == old['mode']
    assert current['position'] > .2
    assert current['rows'] == [*old['rows'], {
        'entryId': current['entryId'], 'song': '182', 'voice': '82',
    }]
    assert len({r['entryId'] for r in current['rows']}) == 4
    for name, state in [('inserted', 'playing'), ('paused', 'paused'), ('resumed', 'playing')]:
        assert native[name]['state'] == state
        assert all(native[name][key] == current[key] for key in stable), native
    assert native['paused']['position'] >= current['position']
    assert native['resumed']['position'] > native['paused']['position'] + .1

    final = stages['resumed']
    for name, count in zip(PLAYBACK_STAGES, (0, 0, 1, 2, 3, 4), strict=True):
        assert stages[name]['preparations'] == final['preparations'][:count]
        assert stages[name]['selections'] == final['selections'][:count]
    assert len(final['preparations']) == len(final['selections']) == 4
    for prepared, selection, source, voice in zip(
        final['preparations'], final['selections'], (old, old, current, current),
        ('81', '82', '82', '82'), strict=True,
    ):
        assert prepared == {
            'owner': selection['owner'], 'playId': source['playId'],
            'queueRevision': source['queueRevision'],
        }
        assert selection['kind'] == 'podcast_purchased_episode'
        assert selection['target'] == {'kind': 'episode', 'id': voice, 'revision': revision}
        if voice == '81':
            assert selection['error'] == 'unavailable' and 'reply' not in selection
        else:
            assert selection['reply']['playId'] == current['playId']
            assert selection['reply']['entryId'] == current['entryId']
            assert selection['reply']['queueRevision'] == current['queueRevision']

    def playback_url(id_):
        return [PLAY_URL, {'ids': json.dumps([id_]), 'level': 'standard', 'encodeType': 'aac',
                           'immerseType': 'c51', 'trialMode': -1}]

    wire_old = [[BOUGHT, {'limit': 20, 'offset': 0}], playback_url('3')]
    wire_denied = [*wire_old, [DETAIL, {'id': '81'}], [PLAY_RIGHTS, {'voiceId': '81'}]]
    wire_allowed = [
        *wire_denied, [DETAIL, {'id': '82'}], [PLAY_RIGHTS, {'voiceId': '82'}], playback_url('182'),
    ]
    for name, wire in zip(PLAYBACK_STAGES, (wire_old, wire_old, wire_denied,
                                           wire_allowed, wire_allowed, wire_allowed), strict=True):
        assert stages[name]['purchasedTrace']['calls'] == wire, stages[name]['purchasedTrace']
    assert complete['purchasedTrace']['calls'] == wire_allowed
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters'))
    pngs = sorted(output.glob('ux_*.png'))
    assert [p.stem for p in pngs] == ([p.stem for p in paths] if capture else [])
    return dict(ok=True, actualWebView=True, stages=list(PLAYBACK_STAGES), screenshots=len(pngs),
                syntheticVoice='82', syntheticMedia='182', preservedQueue=['1', '2', '3', '182'],
                playUrlRequests=2, programEntitlementRequests=2,
                realMusicRequests=0, realAccountWrites=0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--frontend-dist', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--runtime', type=Path, help='packaged Windows native runtime directory')
    parser.add_argument('--script', type=Path, help='replace only the embedded synthetic driver')
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--playback', action='store_true', help='prove purchased episode playback, not downloads')
    args = parser.parse_args()
    if sys.platform not in ('linux', 'win32') or (args.screenshots and sys.platform != 'linux'):
        parser.error('use Linux X11 or Windows; screenshots require X11')
    if sys.platform == 'linux':
        require_xvfb()
        if len(Path('/proc/net/route').read_text().splitlines()) != 1:
            parser.error('Linux requires an isolated no-default-route network namespace')
    runtime = args.runtime.resolve(strict=True) if args.runtime else None
    if sys.platform == 'win32' and (not runtime or not (runtime / 'libmpv-2.dll').is_file()):
        parser.error('Windows requires --runtime with the packaged libmpv-2.dll')
    binary = args.binary.resolve(strict=True)
    dist = args.frontend_dist.resolve(strict=True)
    if not (dist / 'index.html').is_file():
        parser.error('--frontend-dist must contain index.html')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_PROOF_FRONTEND_DIST=str(dist))
    env['CUMULUS_PODCAST_PURCHASED_PLAYBACK_PROOF' if args.playback else
        'CUMULUS_PODCAST_PURCHASED_PROOF'] = '1'
    if sys.platform == 'linux':
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if runtime:
        env['PATH'] = str(runtime) + os.pathsep + env.get('PATH', '')
        if sys.platform == 'win32':
            env['MPV_DLL_DIR_CUMULUS'] = str(runtime)
    if args.script:
        env['CUMULUS_UX_SCRIPT'] = str(args.script.resolve(strict=True))
    isolated_profile(env, root)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, (root / 'host.log').read_text(encoding='utf-8')
    report = (verify_playback if args.playback else verify)(root, args.screenshots)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
