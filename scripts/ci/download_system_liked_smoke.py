"""Real download-row system liked collection with one-sided and dual readback."""

import argparse
import json
import os
from pathlib import Path

from window_manager import launch


STAGES = ('verified_choice', 'canceled', 'one_sided', 'still_pending', 'reconciled', 'liked_heart')


def verify(output, screenshots):
    def read(name):
        return json.loads((output / name).read_text(encoding='utf-8'))

    result = read('complete.json')
    assert result['ok'] and result['stageCount'] == len(STAGES)
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    assert len(list(output.glob('ux_*.json'))) == len(STAGES)
    if screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(STAGES)
    rows = [read(f'ux_{i:02d}_{stage}.json') for i, stage in enumerate(STAGES, 1)]
    report = [row['syntheticDownloadLiked'] for row in [*rows, result]]
    payload = {'pid': '83', 'trackIds': '["1"]', 'op': 'add'}
    assert [value['writes'] for value in report] == [[], []] + [[payload]] * 5
    assert all(value['cachedLikes'] == ['3', '7', '10'] for value in report[:4])
    assert all(value['cachedLikes'] == ['3', '7', '10', '1'] for value in report[4:])
    assert len({value['genericLikes'] for value in report}) == 1
    list_read = {'path': '/api/user/playlist', 'args': {'uid': '2', 'offset': 0, 'limit': 1000}}
    detail = {'path': '/api/v6/playlist/detail', 'args': {'id': '83', 'n': 100000, 's': 0}}
    likes = {'path': '/api/song/like/get', 'args': {'uid': '2'}}
    assert report[0]['reads'] == report[1]['reads'] == [list_read, detail]
    assert report[2]['reads'] == [list_read, detail] * 2 + [list_read, detail, likes] * 2
    assert report[3]['reads'] == report[2]['reads'] + [list_read, detail, likes]
    assert report[4]['reads'] == report[3]['reads'] + [list_read, detail, likes]
    assert all(value['reads'] == report[4]['reads'] for value in report[5:])
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
                              'desktop/examples/download_system_liked_ui_probe.js'),
        CUMULUS_MOTION_PROOF='1',
        CUMULUS_DOWNLOAD_PAGE_UI_PROOF='1',
        CUMULUS_DOWNLOAD_COLLECTION_PROOF='1',
        CUMULUS_DOWNLOAD_COLLECTION_LIKED_PROOF='1',
        CUMULUS_PLAYLIST_COLLECT_PROOF='1',
    )
    for key, name in (
        ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
        ('XDG_RUNTIME_DIR', 'runtime'),
    ):
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    output = root / 'first'
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exit {status}'
    result = verify(output, args.screenshots)
    result.update(actualWebView=True, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({key: result[key] for key in ('ok', 'stageCount', 'syntheticDownloadLiked')}))


if __name__ == '__main__':
    main()
