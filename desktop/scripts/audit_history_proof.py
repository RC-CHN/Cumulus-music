"""Read-only audit of real WebView history evidence; no account/network actions."""
import argparse
import json
from pathlib import Path
import sqlite3


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def audit(ui, local, live=None):
    stages = [read(p) for p in (ui / 'first').glob('ux_*.json')]
    complete = read(ui / 'first/complete.json')
    assert complete['ok'] and len(stages) == complete['stageCount'] == 24
    assert complete['realMusicRequests'] == 0 and complete['syntheticHistoryWrites'] == 2
    assert {v['theme'] for v in stages} == {'light', 'dark'}
    assert {v['scope'] for v in stages} == {'歌曲', '声音', '播客', '有声书', '专辑', '歌单'}
    assert all(v['rows'] <= 35 and v['footerBottom'] <= v['mainBottom'] + 1 for v in stages)
    assert len(list((ui / 'first').glob('ux_*.png'))) == 24
    assert sum(v['width'] == 760 and v['height'] == 540 for v in stages) >= 6
    guest = read(local / 'local_history_page.json')
    replay = read(local / 'local_history_replay.json')
    assert guest['guest'] and guest['canSync'] is False and guest['historyRows'] == 1
    assert replay['guest'] and replay['historyReplayVerified'] and replay['position'] > .2
    assert replay['mediaStart'] == 30 and replay['duration'] == 90 and replay['queue'] == 1
    assert replay['audioOutput'] == 'null'
    from audit_local_history_proof import audit as audit_local
    result = {'ok': True, 'uiStates': 24, 'uiScreenshots': 24,
              'guestNativeHistoryReplay': True, 'local': audit_local(local),
              'physicalFpsVerified': False, 'actualSoundCardVerified': False}
    if live:
        directory = live / 'first'
        done = read(directory / 'complete.json')
        assert done['ok'] and done['explicitHistoryWrites'] == 0
        assert done['playbackOutput'] == 'null'
        playing = read(directory / 'ux_07_playing.json')
        player = playing['nativePlayer']
        queue = playing['nativeQueue']
        assert player['state'] == 'playing' and player['error'] is None and player['position'] > .2
        assert player['queueLen'] == len(queue['tracks']) > 0
        allowed = {'/api/w/nuser/account/get', '/api/user/playlist', '/api/song/like/get',
                   '/api/v3/discovery/recommend/songs', '/api/play-record/song/list',
                   '/api/play-record/album/list', '/api/play-record/playlist/list',
                   '/api/play-record/voice/list', '/api/play-record/djradio/list',
                   '/api/play-record/voice/book/timeline/list', '/api/song/enhance/privilege',
                   '/api/v3/song/detail', '/api/song/enhance/player/url/v1', '/api/song/lyric/v1'}
        allowed |= {'/api/rtrs/abt/front/expinfo/list', '/api/relay/config/get',
                    '/api/relay/setting/show', '/api/link/position/show/resource'}
        calls = read(directory / 'api-calls.json')
        assert all(c['path'] in allowed and c['code'] == 200 for c in calls)
        database = (live / 'state/core.sqlite3').resolve()
        with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as connection:
            rows = connection.execute("SELECT key,value FROM state WHERE kind='play-history-v1' AND key LIKE 'remote:%'").fetchall()
        assert len(rows) == 6
        for key, value in rows:
            assert len(json.loads(value)['records']) == done['counts'][key.split(':')[1]]
        result['live'] = {'states': 7, 'counts': done['counts'], 'readRequests': len(calls),
                          'historyWrites': 0, 'queueLength': player['queueLen'],
                          'playingPosition': player['position'], 'output': 'null',
                          'sqliteMatchesCounts': True}
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--ui', type=Path, required=True)
    parser.add_argument('--local', type=Path, required=True)
    parser.add_argument('--live', type=Path)
    args = parser.parse_args()
    print(json.dumps(audit(args.ui, args.local, args.live), ensure_ascii=False, indent=2))
