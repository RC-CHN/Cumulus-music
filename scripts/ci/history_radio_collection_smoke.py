"""Verify both Recently Played radio collectors in a sealed Linux WebView."""

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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
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
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / key
        path.mkdir(mode=0o700)
        env[key] = str(path)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(binary, env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert result == 0, f'native probe exit {result}'
    report = audit(output, root / 'run/state/core.sqlite3', args.screenshots)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
