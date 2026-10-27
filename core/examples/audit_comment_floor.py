#!/usr/bin/env python3
"""Independently compare floor wire pages, Rust projection and SQLite cache.

Read-only evidence inspection; no network or account credentials are loaded.
"""
import argparse
import hashlib
import json
import sqlite3
from pathlib import Path


def audit(root):
    files = sorted(root.glob('api-*.json'))
    calls = [json.loads(p.read_text()) for p in files]
    assert len(calls) == 4
    assert [c['path'] for c in calls] == [
        '/api/w/nuser/account/get', '/api/resource/comment/floor/get',
        '/api/resource/comment/floor/get', '/api/w/nuser/account/get']
    assert all(c['response']['code'] == 200 for c in calls)
    owner = str(calls[0]['response']['profile']['userId'])
    assert str(calls[-1]['response']['profile']['userId']) == owner
    pages = json.loads((root / 'pages.json').read_text())
    parent = pages['query']['parentId']
    thread = 'R_SO_4_' + pages['resource']['id']
    native = [pages['first'], pages['second']]
    ids = []
    for index, (call, page) in enumerate(zip(calls[1:3], native)):
        query = pages['query'] if index == 0 else native[0]['next']
        assert call['payload'] == dict(threadId=thread, parentCommentId=parent,
                                       time=query['time'], limit=query['limit'])
        raw = call['response']['data']
        if raw.get('ownerComment') is None:
            assert page['owner'] is None
        else:
            assert str(raw['ownerComment']['commentId']) == page['owner']['id'] == parent
            assert raw['ownerComment']['content'] == page['owner']['content']
        assert len(raw['comments']) == len(page['comments']) <= query['limit']
        assert raw['totalCount'] == page['total']
        assert raw['hasMore'] == page['more']
        for original, projected in zip(raw['comments'], page['comments']):
            assert str(original['commentId']) == projected['id']
            assert str(original['parentCommentId']) == projected['parent_id'] == parent
            assert original['content'] == projected['content']
            assert original['time'] == projected['time']
            assert str(original['user']['userId']) == projected['author']['id']
            ids.append(projected['id'])
        if page['more']:
            assert page['next'] == dict(parentId=parent, time=raw['time'], limit=query['limit'])
        else:
            assert page['next'] is None
    assert len(ids) == len(set(ids)) == native[0]['total'] == 27
    assert [len(p['comments']) for p in native] == [20, 7]
    assert native[1]['more'] is False
    db_path = root / 'state/core.sqlite3'
    with sqlite3.connect(db_path.resolve().as_uri() + '?mode=ro', uri=True) as db:
        rows = db.execute("SELECT owner,value FROM state WHERE kind='comment-floors-v1'").fetchall()
        assert len(rows) == 2
        cached = []
        for account, value in rows:
            value = json.loads(value)
            assert account == owner and value['resource'] == pages['resource']
            assert value['revision'] is None
            cached.append(value)
        for query, page in zip([pages['query'], native[0]['next']], native):
            assert [v['page'] for v in cached if v['query'] == query] == [page]
        assert db.execute("SELECT count(*) FROM state WHERE kind IN ('comment-writes-v1','comment-likes-v1')").fetchone()[0] == 0
    inputs = files + [root / 'pages.json', db_path]
    report = dict(ok=True, rawPages=2, counts=[20, 7], uniqueReplies=27,
                  exactServerCursor=True, sqlitePagesEqual=True, accountNamespaces=1,
                  mutationRequests=0, hashes={str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in inputs})
    (root / 'independent-audit.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k != 'hashes'}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    audit(parser.parse_args().root)
