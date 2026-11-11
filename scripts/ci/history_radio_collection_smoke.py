"""Verify Recently Played radio collectors or row menus in a sealed Linux WebView."""

import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import sys

from motion_smoke import launch


STAGES = [
    ('podcast_initial', '81', '收藏', 'false', 0, 0),
    ('podcast_collected', '81', '取消收藏', 'true', 1, 3),
    ('confirmation', '81', '取消收藏', 'true', 1, 3),
    ('cancelled', '81', '取消收藏', 'true', 1, 3),
    ('lost_ack', '81', '核对收藏', None, 2, 4),
    ('reconciled', '81', '收藏', 'false', 2, 6),
    ('book_initial', '82', '收藏', 'false', 2, 6),
    ('book_collected', '82', '取消收藏', 'true', 3, 9),
]
DETAIL = '/api/djradio/v3/get'
MEMBERSHIP = '/api/djradio/get/subed'
SUBSCRIBE = '/api/djradio/sub'
UNSUBSCRIBE = '/api/djradio/unsub'
PROGRAMS = '/api/v6/dj/program/byradio'
SOURCE = '/api/song/enhance/player/url/v1'
RIGHTS = '/api/dj/play/url/check'
DELETE = '/api/play-record/delete'
MENU_STAGES = [
    'baseline', 'own_podcast_menu', 'podcast_menu', 'opened_81', 'collected_podcast_menu',
    'share_dialog', 'share_copied', 'podcast_next', 'book_menu', 'book_copied', 'opened_82',
    'book_play', 'removal_confirmation', 'removal_cancelled', 'removal_confirmed',
]


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def audit(output, database, screenshots):
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(STAGES)
    paths = sorted(output.glob('ux_*.json'))
    names = [f'ux_{index:02d}_{stage[0]}' for index, stage in enumerate(STAGES, 1)]
    assert [path.stem for path in paths] == names
    photos = sorted(output.glob('ux_*.png'))
    assert [path.stem for path in photos] == (names if screenshots else [])
    sha = {}
    for image in photos:
        raw = image.read_bytes()
        assert raw.startswith(b'\x89PNG\r\n\x1a\n') and len(raw) > 1000, image
        sha[image.name] = hashlib.sha256(raw).hexdigest()
    calls = complete['historyChapterCalls']
    assert all(not call['cached'] for call in calls)
    assert [call['args'] for call in calls if call['path'] == DETAIL] == (
        [{'id': '81'}] * 6 + [{'id': '82'}] * 3
    )
    members = [call['args'] for call in calls if call['path'] == MEMBERSHIP]
    assert members and all(body == {'targetUserId': '2', 'limit': 1000} for body in members)
    writes = [(call['path'], call['args']) for call in calls
              if call['path'] in (SUBSCRIBE, UNSUBSCRIBE)]
    assert writes == [(SUBSCRIBE, {'id': '81'}), (UNSUBSCRIBE, {'id': '81'}),
                      (SUBSCRIBE, {'id': '82'})]
    action_calls = [(call['path'], call['args']['id']) for call in calls
                    if call['path'] in (DETAIL, SUBSCRIBE, UNSUBSCRIBE)]
    assert action_calls == [
        (DETAIL, '81'), (SUBSCRIBE, '81'), (DETAIL, '81'), (DETAIL, '81'),
        (DETAIL, '81'), (UNSUBSCRIBE, '81'), (DETAIL, '81'), (DETAIL, '81'),
        (DETAIL, '82'), (SUBSCRIBE, '82'), (DETAIL, '82'), (DETAIL, '82'),
    ]
    assert [call['args'] for call in calls if call['path'] == '/api/v6/dj/program/byradio'] == [
        {'radioId': '84', 'limit': 250, 'offset': 0},
    ]
    assert [call['args'] for call in calls if call['path'] == '/api/dj/play/url/check'] == [
        {'voiceId': '842'},
    ]
    assert [json.loads(call['args']['ids']) for call in calls
            if call['path'] == '/api/song/enhance/player/url/v1'] == [['1842']]
    baseline = None
    passive = None
    for path, (name, id_, title, pressed, write_count, details) in zip(paths, STAGES, strict=True):
        value = read(path)
        assert value['id'] == id_ and value['title'] == title and value['pressed'] == pressed
        assert value['kind'] == ('podcast' if id_ == '81' else 'audiobook')
        assert value['chapter'] == ('播客第二集' if id_ == '81' else '有声书第二章')
        subset = value['historyChapterCalls']
        assert subset == calls[:len(subset)]
        assert sum(call['path'] in (SUBSCRIBE, UNSUBSCRIBE) for call in subset) == write_count
        assert sum(call['path'] == DETAIL for call in subset) == details
        if name == 'podcast_collected':
            passive = subset
        if name in ('confirmation', 'cancelled'):
            assert subset == passive, 'opening or cancelling made a request'
        assert not value['syntheticHistoryWrites']
        if name == 'confirmation':
            assert value['dialog'] == {
                'title': '确定不再收藏该播单？', 'focus': '取消', 'actions': ['取消', '不再收藏'],
            }
        else:
            assert value['dialog'] is None
        if name == 'cancelled':
            assert value['focus']
        if name == 'lost_ack':
            assert '收藏操作待核对' in value['notice']
        if name in ('podcast_collected', 'book_collected'):
            assert value['notice'] == '收藏成功'
        if name == 'reconciled':
            assert value['notice'] == '取消收藏成功'
        player, queue = value['nativePlayer'], value['nativeQueue']
        assert player['state'] == 'paused' and player['position'] > .25 and not player['error']
        assert player['track']['voiceId'] == '842' and player['track']['id'] == '1842'
        assert player['queueRevision'] == queue['revision'] and queue['current'] == player['entryId']
        assert [row['track']['voiceId'] for row in queue['tracks']] == ['841', '842']
        playback = value['playback']
        assert playback == {
            **{key: player[key] for key in ('state', 'playId', 'entryId', 'queueRevision', 'position')},
            'voiceId': '842', 'mediaId': '1842',
        }
        if baseline is None:
            baseline = playback
        assert playback == baseline
    assert not complete['syntheticHistoryWrites']
    locale = read(output / 'locale.json')
    assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as connection:
        rows = connection.execute(
            "SELECT owner, key, value FROM state WHERE kind='podcast-mutations-v1' ORDER BY key"
        ).fetchall()
    assert [(owner, key) for owner, key, _ in rows] == [
        ('2', 'podcast:subscribe:81'), ('2', 'podcast:subscribe:82'),
    ]
    for (_, _, encoded), id_, desired in zip(rows, ['81', '82'], [False, True], strict=True):
        record = json.loads(encoded)
        assert record['id'] == id_ and record['phase'] == 'confirmed'
        assert record['observed'] is desired and record['desired'] is desired
    return dict(ok=True, actualLinuxWebView=True, nativeNullPlayback=True, stages=len(names),
                screenshots=sha, locale=locale, syntheticCollectionWrites=writes,
                syntheticHistoryWrites=0, network='loopback-only')


def audit_menu(output, database, screenshots):
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(MENU_STAGES)
    names = [f'ux_{index:02d}_{name}' for index, name in enumerate(MENU_STAGES, 1)]
    assert [path.stem for path in sorted(output.glob('ux_*.json'))] == names
    photos = sorted(output.glob('ux_*.png'))
    assert [path.stem for path in photos] == (names if screenshots else [])
    hashes = {}
    for photo in photos:
        data = photo.read_bytes()
        assert data.startswith(b'\x89PNG\r\n\x1a\n') and len(data) > 1000
        hashes[photo.name] = hashlib.sha256(data).hexdigest()
    records = {name: read(output / f'{path}.json')
               for name, path in zip(MENU_STAGES, names, strict=True)}
    calls = complete['historyChapterCalls']
    assert all(not call['cached'] for call in calls)
    for value in records.values():
        subset = value['historyChapterCalls']
        assert subset == calls[:len(subset)]
        assert value['syntheticHistoryWrites'] == int(value['stage'].endswith('removal_confirmed'))
        player, queue = value['nativePlayer'], value['nativeQueue']
        assert player['queueRevision'] == queue['revision'] and player['entryId'] == queue['current']
        assert player['epoch'] == queue['epoch'] and player['position'] > .25 and not player['error']
        assert value['queue'] == [row['track']['voiceId'] for row in queue['tracks']]

    ordinary = ['open', 'play', 'next', 'share', 'copy', 'remove']
    eligible = ['open', 'play', 'next', 'collect', 'share', 'copy', 'remove']
    for stage, id_, items in [
        ('own_podcast_menu', '83', ordinary), ('podcast_menu', '81', eligible),
        ('collected_podcast_menu', '81', ordinary), ('book_menu', '82', eligible),
    ]:
        assert records[stage]['id'] == id_ and records[stage]['items'] == items
    assert records['own_podcast_menu']['trigger'] == 'contextmenu'
    assert records['book_menu']['trigger'] == 'shift-f10'
    for stage in ('podcast_menu', 'book_menu'):
        assert records[stage]['layout'] == {
            'width': '48px', 'height': '48px', 'corner': '8px',
            'title': '16px', 'gap': '10px', 'visible': True,
        }
    assert records['collected_podcast_menu']['collected'] == 'true'
    for id_, title, episode in [('81', '合成历史播客', '播客第一集'),
                                ('82', '合成历史有声书', '有声书第一章')]:
        opened = records[f'opened_{id_}']
        assert (opened['id'], opened['title'], opened['firstEpisode']) == (id_, title, episode)
        assert any(call['path'] == PROGRAMS and
                   call['args'] == {'radioId': id_, 'limit': 200, 'offset': 0, 'asc': True}
                   for call in opened['historyChapterCalls'])
    assert records['share_dialog']['title'] == '分享播客'
    assert records['share_dialog']['publicLink'] == 'https://music.163.com/djradio?id=81'
    assert records['share_copied']['id'] == '81' and records['share_copied']['clipboardMatches']
    assert records['book_copied']['id'] == '82' and records['book_copied']['clipboardMatches']

    baseline = records['baseline']['nativePlayer']
    assert baseline['state'] == 'paused' and baseline['mode'] == 'single_loop'
    assert (baseline['track']['voiceId'], baseline['track']['id']) == ('842', '1842')
    playback_keys = ('epoch', 'state', 'playId', 'entryId', 'position', 'mode', 'track')
    first_play = MENU_STAGES.index('book_play')
    for name in MENU_STAGES[:first_play]:
        value = records[name]
        assert all(value['nativePlayer'][key] == baseline[key] for key in playback_keys)
        appended = MENU_STAGES.index(name) >= MENU_STAGES.index('podcast_next')
        assert value['queue'] == (['841', '842', '811', '812'] if appended else ['841', '842'])
        assert (value['nativePlayer']['queueRevision'] != baseline['queueRevision']) == appended
        assert [json.loads(call['args']['ids']) for call in value['historyChapterCalls']
                if call['path'] == SOURCE] == [['1842']]
        assert [call['args'] for call in value['historyChapterCalls'] if call['path'] == RIGHTS] == [
            {'voiceId': '842'},
        ]
    assert any(call['path'] == PROGRAMS and call['args'] == {'radioId': '81', 'limit': 250, 'offset': 0}
               for call in records['podcast_next']['historyChapterCalls'])
    started = records['book_play']['nativePlayer']
    assert started['state'] == 'playing' and started['mode'] == 'sequential'
    assert started['playId'] != baseline['playId'] and started['entryId'] != baseline['entryId']
    assert started['origin'] == {'kind': 'podcast', 'id': '82'}
    assert (started['track']['voiceId'], started['track']['id']) == ('821', '1821')
    rights = [call['args']['voiceId'] for call in calls if call['path'] == RIGHTS]
    sources = [json.loads(call['args']['ids']) for call in calls if call['path'] == SOURCE]
    # Native prefetch may also start the next episode after Play. The
    # source can still be pending when the GUI capture ends.
    assert rights[:2] == ['842', '821'] and rights[2:] in ([], ['822'])
    assert sources[:2] == [['1842'], ['1821']] and sources[2:] in ([], [['1822']])
    assert len(sources) <= len(rights)
    for name in MENU_STAGES[first_play:]:
        assert records[name]['queue'] == ['821', '822']
        after = records[name]['nativePlayer']
        assert all(after[key] == started[key] for key in
                   ('epoch', 'playId', 'entryId', 'queueRevision', 'origin', 'track'))
        if name != 'book_play':
            assert after['state'] == 'paused'
            assert after['position'] == records['removal_confirmation']['nativePlayer']['position']

    confirmation = records['removal_confirmation']
    assert (confirmation['title'], confirmation['actions'], confirmation['focus']) == (
        '删除 1 条播放记录？', ['取消', '确认'], '取消',
    )
    assert '同时请求删除当前账号的多端记录' in confirmation['description']
    assert '音乐文件、收藏和歌单内容会保留' in confirmation['description']
    assert records['removal_cancelled']['rowRetained']
    assert records['removal_confirmed']['remaining'] == ['83']
    assert complete['syntheticHistoryWrites'] == 1
    deletes = [call['args'] for call in calls if call['path'] == DELETE]
    assert deletes == [{'type': 'DJ_RADIO', 'resourceId': '81',
                        'lastResourceId': '83', 'lastPlayTime': 1793179999917}]
    assert [call['args'] for call in calls if call['path'] == SUBSCRIBE] == [{'id': '81'}]
    assert not any(call['path'] == UNSUBSCRIBE for call in calls)
    assert not any(call['path'] == DELETE for call in records['removal_cancelled']['historyChapterCalls'])
    assert records['share_dialog']['historyChapterCalls'] == records['share_copied']['historyChapterCalls']
    assert records['book_menu']['historyChapterCalls'] == records['book_copied']['historyChapterCalls']

    locale = read(output / 'locale.json')
    assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as connection:
        saved = connection.execute(
            "SELECT owner, key, value FROM state WHERE kind='podcast-mutations-v1'"
        ).fetchall()
        journals = connection.execute(
            "SELECT owner, value FROM state WHERE kind='play-history-mutations-v1'"
        ).fetchall()
    assert [(owner, key) for owner, key, _ in saved] == [('2', 'podcast:subscribe:81')]
    collection = json.loads(saved[0][2])
    assert collection['phase'] == 'confirmed' and collection['observed'] is True
    assert len(journals) == 1 and journals[0][0] == '2'
    deletion = json.loads(journals[0][1])
    assert deletion['kind'] == 'podcast' and deletion['phase'] == 'confirmed'
    assert deletion['remote_planned'] and deletion['remote_ids'] == ['81']
    return dict(ok=True, actualLinuxWebView=True, nativeNullPlayback=True, stages=len(names),
                screenshots=hashes, locale=locale, nativeClipboardChecks=2,
                syntheticCollectionWrites=1, syntheticHistoryWrites=deletes,
                realMusicRequests=0, realAccountWrites=0, network='loopback-only')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--menu', action='store_true', help='verify the radio row More menu')
    args = parser.parse_args()
    if not sys.platform.startswith('linux') or not os.environ.get('DISPLAY', '').startswith(':'):
        parser.error('a Linux X11 WebView session is required')
    if ({name for _, name in socket.if_nameindex()} != {'lo'}
            or len(Path('/proc/net/route').read_text(encoding='ascii').splitlines()) != 1):
        parser.error('use a sealed network namespace containing only loopback')
    # The capture driver is this process; its GDK backend must also be X11.
    os.environ['GDK_BACKEND'] = 'x11'
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11', CUMULUS_PROOF_DIR=str(root / 'run'),
               CUMULUS_HISTORY_PROOF='1', CUMULUS_HISTORY_RADIO_CHAPTER_PROOF='1',
               CUMULUS_HISTORY_RADIO_COLLECTION_PROOF='1')
    if args.menu:
        script = Path(__file__).resolve().parents[2] / 'desktop/examples/history_radio_menu_ui_probe.js'
        env.update(CUMULUS_HISTORY_RADIO_MENU_PROOF='1', CUMULUS_UX_SCRIPT=str(script.resolve(strict=True)))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / key
        path.mkdir(mode=0o700)
        env[key] = str(path)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(binary, env, log, output, args.screenshots, acknowledge_receipts=args.menu)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert result == 0, f'native probe exit {result}'
    check = audit_menu if args.menu else audit
    report = check(output, root / 'run/state/core.sqlite3', args.screenshots)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
