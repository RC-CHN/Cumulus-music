"""Inspect existing native/WebView evidence, without making network requests."""
import argparse
import json
from pathlib import Path
import sqlite3


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def inspect(root, count):
    first = root / 'first'
    complete = read(first / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == count
    assert not (first / 'failed.json').exists()
    stages = list(first.glob('ux_*.json'))
    assert len(stages) == count
    for stage in stages:
        assert stage.with_suffix('.png').is_file()
        value = read(stage)
        assert value['pageBottom'] <= value['mainBottom'] + 1
    assert {read(p)['theme'] for p in stages} == {'light', 'dark'}
    assert any(read(p)['width'] == 760 and read(p)['height'] == 540 for p in stages)
    db = sqlite3.connect(f'file:{root / "state" / "core.sqlite3"}?mode=ro', uri=True)
    snapshots = db.execute("SELECT value FROM state WHERE kind='account-settings-v1' AND key='snapshot'").fetchall()
    assert len(snapshots) == 1
    operations = [json.loads(row[0]) for row in db.execute("SELECT value FROM state WHERE kind='account-settings-intents-v1'")]
    return complete, json.loads(snapshots[0][0]), operations


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--synthetic', type=Path, required=True)
    parser.add_argument('--live', type=Path, required=True)
    args = parser.parse_args()
    synthetic, snapshot, operations = inspect(args.synthetic.resolve(), 21)
    assert synthetic['syntheticSettingsWrites'] == 6 and synthetic['readbackBeforeSelection']
    assert sorted(op['phase'] for op in operations) == ['accepted_current_state', 'confirmed', 'confirmed', 'confirmed', 'confirmed', 'rejected']
    assert snapshot['values']['shareSetting'] == 1
    assert snapshot['values']['playRecordSetting'] == 2
    assert snapshot['values']['personalServiceSetting'] == 2
    assert snapshot['values']['allowLikedNotify'] is True  # Unapplied, explicitly accepted.
    assert snapshot['values']['allowNewFollowerNotify'] is False  # Applied despite lost reply.
    assert snapshot['values']['allowSubscriptionNotify'] is True  # Rejected write.
    live, snapshot, operations = inspect(args.live.resolve(), 5)
    assert live['readOnly'] and live['syntheticSettingsWrites'] == 0
    assert not operations and not live['settings']['operations']
    assert live['settings']['revision'] == snapshot['revision']
    assert len(live['settings']['fields']) == 7
    for field in live['settings']['fields']:
        assert field['state'] == 'available' and field['value'] == snapshot['values'][field['key']]
    calls = read(args.live / 'first' / 'api-calls.json')
    assert len(calls) == 5
    assert {c['path'] for c in calls} == {
        '/api/w/nuser/account/get', '/api/user/playlist', '/api/song/like/get',
        '/api/v3/discovery/recommend/songs', '/api/user/setting',
    }
    assert all(c['code'] == 200 for c in calls)
    print(json.dumps({'ok': True, 'syntheticStates': 21, 'syntheticWrites': 6,
                      'liveStates': 5, 'liveAccountSettingsReads': 1, 'liveWrites': 0,
                      'checkedFields': 7, 'physicalRefreshRateVerified': False}))


if __name__ == '__main__':
    main()
