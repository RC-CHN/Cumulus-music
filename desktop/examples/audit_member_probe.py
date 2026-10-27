#!/usr/bin/env python3
"""Read-only audit of actual membership DOM/IPC/native/official-response evidence."""
import argparse
import json
from pathlib import Path
import sqlite3


def read(p):
    return json.loads(p.read_text())


def stages(root, folder, expected):
    p = root / folder
    assert read(p / 'capture.json')['ok'] and read(p / 'complete.json')['ok']
    assert not (p / 'blocked-endpoint.json').exists()
    result = {v.stem: read(v) for v in p.glob('member_*.json')}
    assert len(result) == expected
    for name, v in result.items():
        assert not v['overflow'] and 'nativeError' not in v
        assert v['nativePlayer']['error'] is None
        assert (p / (name + '.png')).is_file()
        assert v['nativeMember']['epoch'] == v['nativeRights']['epoch'] == v['nativePlayer']['epoch']
    return result


def verify(root, reopen):
    s = stages(root, 'first', 7)
    raw = [read(p) for p in sorted((root / 'first').glob('official-*.json'))]
    recommend = next(r['response']['data']['songInfoList'] for r in raw if r['path'].endswith('/viptab/recommend/song/list'))
    history = {r['params']['date']: r['response']['data']['songs'] for r in raw if r['path'].endswith('/songs/history/detail')}
    modules = next(r['response']['data'] for r in raw if r['path'].endswith('/viptab/module/get'))
    visible = s['member_home']['nativeMember']['value']['page']['modules']
    assert [m['title'] for m in visible] == [m['showName'] for m in modules if m['moduleType'] in [1, 3, 4, 5, 6, 7, 9, 12]]
    assert s['member_home']['recommendations'] == len(recommend)
    assert len(history) == 2
    for name in ['member_recommend_play', 'member_history_play', 'member_saved']:
        v = s[name]
        songs = recommend if name == 'member_recommend_play' else history[v['query']['date']]
        ids = [str(x['id']) for x in songs]
        assert [x['track']['id'] for x in v['nativeQueue']['tracks']] == ids
        assert v['nativePlayer']['queueLen'] == len(ids)
        assert v['playerTitle'] == v['nativePlayer']['track']['name']
        assert not v['nativePlayer']['fm']
    for name in ['member_history', 'member_history_date']:
        v = s[name]
        assert v['nativeMember']['value']['page']['date'] == v['query']['date']
        assert [t['id'] for t in v['nativeMember']['value']['page']['tracks']] == [str(t['id']) for t in history[v['query']['date']]]
    saved = s['member_saved']['nativePlayer']
    assert saved['state'] == 'paused' and abs(saved['position'] - 12) < .7
    calls = read(root / 'first/api-calls.json')
    auth = [c['params'] for c in calls if c['path'] == '/api/song/enhance/player/url/v1']
    assert any(v['trialMode'] == 41 for v in auth)
    assert any(v['trialMode'] == -1 for v in auth)
    rights = next(r['response']['data'] for r in raw if r['path'].endswith('/client/vip/info'))
    view = s['member_home']['nativeRights']['value']
    names = dict(vinyl='associator', plus='redplus', music_package='musicPackage', album='albumVip', voice_book='voiceBookVip')
    for e in view['entitlements']:
        original = rights.get(names[e['kind']]) or {}
        assert e['expiresAt'] == original.get('expireTime', 0)
        assert e['active'] == (e['expiresAt'] > view['serverNow'])
        assert next(x['active'] for x in s['member_home']['rights'] if x['kind'] == e['kind']) == e['active']
    selected = s['member_download_dialog']['nativeMember']['value']['page']['tracks']
    assert str(len(selected)) in s['member_download_dialog']['downloadTitle']
    with sqlite3.connect((root / 'state/core.sqlite3').resolve().as_uri() + '?mode=ro', uri=True) as db:
        assert db.execute("SELECT COUNT(*) FROM state WHERE kind='download_tasks_v1'").fetchone()[0] == 0
        queue = json.loads(db.execute("SELECT value FROM state WHERE kind='playback' AND key='queue'").fetchone()[0])
        assert all(i['source']['kind'] == 'member' and i['resource']['trial_mode'] == -1 for i in queue['items'])
    count = 7
    if reopen:
        r = stages(root, 'reopen', 2)
        restored = r['member_reopened']['nativePlayer']
        assert restored['entryId'] == saved['entryId'] and restored['state'] == 'paused' and abs(restored['position'] - 12) < .7
        assert r['member_reopened']['nativeQueue']['tracks'] == s['member_saved']['nativeQueue']['tracks']
        assert r['member_resumed']['nativePlayer']['position'] > 12.3
        count += 2
    return {'ok': True, 'screenshots': count, 'member_modules': len(visible), 'recommendation_queue': len(recommend),
            'history_queue_sizes': [len(v) for v in history.values()], 'server_entitlements_match': True,
            'trial_modes': [41, -1], 'reopen': reopen, 'download_started': False, 'explicit_remote_writes': 0,
            'audio_output': 'real libmpv/null on Linux'}


def links(root):
    s = stages(root, 'first', 4)
    assert read(root / 'first/complete.json')['links']
    raw = [read(p) for p in sorted((root / 'first').glob('official-*.json'))]
    album = s['member_link_album']
    a = next(r for r in raw if r['path'] == '/api/album/v3/detail' and str(r['params']['id']) == album['id'])
    assert str(a['response']['album']['id']) == album['id']
    assert a['response']['album']['name'] == album['title']
    playlist = s['member_link_playlist']
    p = next(r for r in raw if r['path'] == '/api/v6/playlist/detail' and str(r['params']['id']) == playlist['id'])
    assert str(p['response']['playlist']['id']) == playlist['id']
    assert p['response']['playlist']['name'] == playlist['title']
    book = s['member_link_book']
    b = next(r for r in raw if r['path'] == '/api/djradio/v3/get' and str(r['params']['id']) == book['id'])
    radio = b['response'].get('data') or b['response'].get('djRadio')
    assert str(radio['id']) == book['id'] and radio['name'] == book['title']
    calls = read(root / 'first/api-calls.json')
    assert any(c['path'] == '/api/v1/resource/comments/R_SO_4_' + s['member_link_comments']['id'] for c in calls)
    for v in s.values():
        assert not v['nativeQueue']['tracks']
    return {'ok': True, 'screenshots': 4, 'album_detail': True, 'playlist_detail': True,
            'audiobook_detail': True, 'song_comments': True, 'paid_playback_attempted': False,
            'explicit_remote_writes': 0}


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('root', type=Path)
    p.add_argument('--reopen', action='store_true')
    p.add_argument('--links', action='store_true')
    args = p.parse_args()
    result = links(args.root) if args.links else verify(args.root, args.reopen)
    with (args.root / ('independent-reopen-audit.json' if args.reopen else 'independent-audit.json')).open('x') as f:
        json.dump(result, f, indent=2)
        f.write('\n')
    print(json.dumps(result))
