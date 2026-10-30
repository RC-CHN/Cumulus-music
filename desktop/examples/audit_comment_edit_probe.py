#!/usr/bin/env python3
"""Independently audit synthetic comment UI requests, native journals and reopen.

This does not claim public comment writes were performed on the real service.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import sqlite3

WRITE_PATHS = {'/api/resource/comments/add', '/api/v1/resource/comments/reply',
               '/api/resource/comments/delete'}


def load(path):
    return json.loads(path.read_text())


def writes(stage):
    return [c for c in stage['fixture']['calls'] if c['path'] in WRITE_PATHS]


def audit(root):
    first = load(root / 'first/complete.json')
    reopened = load(root / 'reopen/complete.json')
    assert first['ok'] and reopened['ok']
    assert len(writes(first)) == 6 and len(writes(reopened)) == 8
    for run, count in [('first', 11), ('reopen', 6)]:
        assert load(root / run / 'capture.json')['ok']
        stages = sorted((root / run).glob('edit_*.json'))
        assert len(stages) == count
        for path in stages:
            value = load(path)
            assert value['overflow'] is False and value['realAccountWrites'] == 0
            assert path.with_suffix('.png').read_bytes()[:8] == b'\x89PNG\r\n\x1a\n'
    records = first['operations']['value']
    assert Counter(r['phase'] for r in records) == {'confirmed': 3, 'rejected': 2, 'uncertain': 1}
    unknown = next(r for r in records if r['phase'] == 'uncertain')
    assert not unknown['acknowledged'] and unknown['receiptId'] is None
    assert unknown['command']['content'] == '丢失回包后保留责任'
    for name in ['edit_reopen_pending', 'edit_reopen_unresolved', 'edit_reopen_abandon_confirm']:
        stage = load(root / 'reopen' / (name + '.json'))
        assert len(writes(stage)) == 6
        row = next(r for r in stage['operations']['value'] if r['id'] == unknown['id'])
        assert row['phase'] == 'uncertain' and not row['acknowledged'] and row['receiptId'] is None
    abandoned = load(root / 'reopen/edit_reopen_abandoned.json')
    assert len(writes(abandoned)) == 6
    assert next(r for r in abandoned['operations']['value'] if r['id'] == unknown['id'])['phase'] == 'abandoned'
    final = reopened['operations']['value']
    assert Counter(r['phase'] for r in final) == {'confirmed': 5, 'rejected': 2, 'abandoned': 1}
    assert {r['serviceCode'] for r in final if r['phase'] == 'rejected'} == {480, 404}
    assert load(root / 'first/edit_unicode_limit.json')['utf16Length'] == 1001
    assert len(writes(load(root / 'first/edit_unicode_limit.json'))) == 0
    assert load(root / 'first/edit_rejected_phone.json')['draft'] == '需要绑定手机号'
    assert load(root / 'first/edit_rejected_reply.json')['draft'] == '目标已消失'
    assert len(writes(load(root / 'first/edit_delete_confirm.json'))) == 2
    assert len(writes(load(root / 'first/edit_deleted.json'))) == 3
    floor1 = load(root / 'first/edit_floor_first.json')['floorIds']
    floor2 = load(root / 'first/edit_floor_second.json')['floorIds']
    assert floor1 == ['102'] + [str(i) for i in range(201, 221)]
    assert floor2 == ['102', '221']

    calls = reopened['fixture']['calls']
    for call in calls:
        if call['lostResponse']:
            assert call['path'] == '/api/resource/comments/add'
            assert call['payload']['content'] == '丢失回包后保留责任'
        if call['path'] not in WRITE_PATHS:
            assert call['path'] in {'/api/w/nuser/account/get', '/api/user/playlist',
                                   '/api/song/like/get', '/api/v3/discovery/recommend/songs',
                                   '/api/v1/resource/comments/R_SO_4_123', '/api/resource/comment/floor/get',
                                   '/api/search/default/keyword/get', '/api/rtrs/abt/front/expinfo/list',
                                   '/api/relay/setting/show'}
    mutations = writes(reopened)
    assert Counter(c['path'] for c in mutations) == {
        '/api/resource/comments/add': 4, '/api/v1/resource/comments/reply': 2,
        '/api/resource/comments/delete': 2}
    assert sum(c['lostResponse'] for c in mutations) == 1
    for call in mutations:
        p = call['payload']
        assert p['threadId'] == 'R_SO_4_123'
        if call['path'].endswith('/add'):
            assert all(int(p[k]) == v for k, v in dict(resourceType=0,resourceId=0,expressionPicId=-1,bubbleId=-1).items())
        elif call['path'].endswith('/reply'):
            assert p['commentId'] == '102' and int(p['resourceId']) == int(p['resourceType']) == 0
        else:
            assert p['commentId'] in {'9001', '9003'}
    assert mutations[0]['payload']['content'] == ' 第一条评论 🎵\n保留原始空白 '
    assert mutations[1]['response']['comment']['beReplied'][0]['beRepliedCommentId'] == '102'
    for i, call in enumerate(calls):
        if call['path'] not in WRITE_PATHS or call['lostResponse'] or call['response']['code'] != 200:
            continue
        delete = call['path'].endswith('/delete')
        target = call['payload']['commentId'] if delete else call['response']['comment']['commentId']
        later = []
        for row in calls[i+1:]:
            if row['path'] in WRITE_PATHS:
                break
            if row['path'] == '/api/v1/resource/comments/R_SO_4_123' and row['payload']['commentId'] == target:
                later.append(row)
        assert len(later) >= 2
        for read in later[:2]:
            observed = read['response']['currentComment']
            assert observed is None if delete else observed['commentId'] == target

    remote = load(root / 'fixture.json')
    assert remote == reopened['fixture']
    assert {'101', '102', '9000', '9002'} <= remote['rows'].keys()
    assert '9001' not in remote['rows'] and '9003' not in remote['rows']
    assert remote['rows']['101']['content'] == '自己的原评论，保留不动。'
    assert remote['rows']['102']['content'] == '听到这里，想起了夏天的海风。'
    # Compare actual durable native state, rather than trusting DOM status text.
    with sqlite3.connect((root / 'state/core.sqlite3').resolve().as_uri() + '?mode=ro', uri=True) as db:
        rows = db.execute("SELECT owner,key,value FROM state WHERE kind='comment-writes-v1'").fetchall()
        assert len(rows) == 8
        for owner, key, encoded in rows:
            row = json.loads(encoded)
            projected = next(r for r in final if r['id'] == key)
            assert owner == '1' and row['id'] == key
            assert row['phase'] == projected['phase']
            assert row['command'] == projected['command']
            assert row['acknowledged'] == projected['acknowledged']
    inputs = [root/'fixture.json', root/'state/core.sqlite3'] + sorted(root.glob('*/*.json')) + sorted(root.glob('*/*.png'))
    report = dict(ok=True, syntheticWrites=8, realAccountWrites=0, screenshots=17,
                  unresolvedAcrossProcess=True, noAutomaticResend=True, exactPayloads=True,
                  originalCommentsPreserved=True, durableOperations=8,
                  hashes={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs})
    (root/'independent-audit.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k!='hashes'}))


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root',type=Path)
    audit(parser.parse_args().root)
