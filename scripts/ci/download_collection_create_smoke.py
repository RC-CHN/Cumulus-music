"""Run the sealed Downloads create-and-collect flow in Linux WebKitGTK."""
import argparse
import json
import os
import sqlite3
from pathlib import Path

from window_manager import launch


STAGES = ('form_prepared', 'canceled', 'confirmed', 'create_pending',
          'create_readback_only', 'add_pending', 'add_reconciled')


def verify(root, screenshots):
    output = root / 'first'
    def read(path):
        return json.loads(path.read_text(encoding='utf-8'))

    result = read(output / 'complete.json')
    assert result['ok'] and result['stageCount'] == len(STAGES)
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    paths = sorted(output.glob('ux_*.json'))
    assert [path.stem.split('_', 2)[2] for path in paths] == list(STAGES)
    if screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(STAGES)
    stages = dict(zip(STAGES, map(read, paths), strict=True))
    assert all(value['actual'] for value in stages.values())
    assert stages['form_prepared']['focused'] == 'name'
    assert stages['create_pending']['focused'] == stages['add_pending']['focused'] == 'reconcile'
    assert not any(stages[name]['createButton'] for name in
                   ('create_pending', 'create_readback_only', 'add_pending'))
    creates = [
        {'uid': '2', 'name': name, 'privacy': privacy}
        for name, privacy in [('合成下载公开歌单', 0), ('合成下载丢失创建', 10), ('合成下载丢失收藏', 10)]
    ]
    adds = [
        {'pid': '1001', 'trackIds': '["1"]', 'op': 'add'},
        {'pid': '1003', 'trackIds': '["9010"]', 'op': 'add'},
    ]
    for name, (create_count, add_count) in zip(
            STAGES, [(0, 0), (0, 0), (1, 1), (2, 1), (2, 1), (3, 2), (3, 2)], strict=True):
        report = stages[name]['syntheticCollection']
        assert report['guardedCreatePayloads'] == creates[:create_count]
        assert report['guardedAddPayloads'] == report['inlineAddPayloads'] == adds[:add_count]
        assert (report['creates'], report['writes']) == (create_count, add_count)
    assert result['syntheticCollection'] == stages['add_reconciled']['syntheticCollection']
    first, lost_create, recovered_create, lost_add, recovered_add = result['commands']
    assert len({first['receipt'], lost_create['receipt'], lost_add['receipt']}) == 3
    for sent, recovered in [(lost_create, recovered_create), (lost_add, recovered_add)]:
        assert sent['receipt'] == recovered['receipt']
        assert recovered['command'] == {'kind': 'status', 'reconcile': True}
        assert not recovered['outcome']['canAdd']
    assert recovered_create['outcome'] == {
        'create': 'confirmed', 'add': None, 'canAdd': False, 'playlistId': '1002'}
    assert recovered_add['outcome'] == {
        'create': 'confirmed', 'add': 'confirmed', 'canAdd': False, 'playlistId': '1003'}
    records = {}
    for path in root.rglob('core.sqlite3'):
        with sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True) as db:
            for owner, key, value in db.execute(
                "SELECT owner,key,value FROM state WHERE kind = 'playlist-mutations'",
            ):
                assert owner == '2' and key not in records
                records[key] = json.loads(value)
    assert len(records) == 5 and all(row['phase'] == 'confirmed' for row in records.values())
    result.update(actualWebView=True, windowsVisualVerified=False, journalEntries=len(records))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if os.name != 'posix':
        parser.error('this proof runner currently targets Linux GTK/WebKit')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(
        CUMULUS_PROOF_DIR=str(root),
        CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)),
        CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] /
                              'desktop/examples/download_collection_create_ui_probe.js'),
        CUMULUS_MOTION_PROOF='1',
        CUMULUS_DOWNLOAD_PAGE_UI_PROOF='1',
        CUMULUS_DOWNLOAD_COLLECTION_PROOF='1',
        CUMULUS_DOWNLOAD_COLLECTION_CREATE_PROOF='1',
        CUMULUS_PLAYLIST_COLLECT_PROOF='1',
    )
    for key, name in (('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')):
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exit {status}'
    report = verify(root, args.screenshots)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({key: report[key] for key in ('ok', 'stageCount', 'journalEntries', 'syntheticCollection')}))


if __name__ == '__main__':
    main()
