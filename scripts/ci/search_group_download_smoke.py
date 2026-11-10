"""Real Linux search-group download, native receipt, ordered task and lost-ack readback."""
import argparse
from contextlib import ExitStack, closing
import json
import os
from pathlib import Path
import sqlite3

from search_group_next_smoke import TrustedClicks
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

LABELS = ('first', 'blocked', 'last', 'anchor', 'download', 'check', 'accepted')


def load(path):
    return json.loads(path.read_text(encoding='utf-8'))


def flac_frames(path):
    data = path.read_bytes()
    assert data.startswith(b'fLaC')
    offset = 4
    while True:
        last = bool(data[offset] & 0x80)
        length = int.from_bytes(data[offset + 1:offset + 4], 'big')
        offset += 4 + length
        assert offset < len(data)
        if last:
            return data[offset:]


def run(args):
    repo = Path(__file__).resolve().parents[2]
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_SEARCH_UI_PROOF='1',
               CUMULUS_SEARCH_GROUP_NEXT_PROOF='1', CUMULUS_SEARCH_GROUP_DOWNLOAD_PROOF='1',
               CUMULUS_UX_SCRIPT=str(repo / 'desktop/examples/search_group_download_ui_probe.js'),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    output = root / 'first'
    with ExitStack() as stack:
        driver = TrustedClicks()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        code = launch(args.binary.resolve(strict=True), env, log, output, True,
                      before_ack=driver.at_capture)
    assert not (output / 'failed.json').exists(), (
        load(output / 'failed.json') if (output / 'failed.json').exists() else ''
    )
    assert code == 0
    report = load(output / 'complete.json')
    assert report['ok'] and report['stageCount'] == len(LABELS)
    assert report['realAccountWrites'] == report['realMusicRequests'] == 0
    rows = []
    for index, label in enumerate(LABELS, 1):
        path = output / f'ux_{index:02}_{label}.json'
        rows.append(load(path))
        assert path.with_suffix('.png').is_file()
    first, last = rows[0], rows[-1]
    assert first['searchGroupDownload'] == []
    assert all(row['queue'] == first['queue'] for row in rows)
    assert all(row['player']['position'] == first['player']['position'] for row in rows)
    prepare, start, check = last['actions']
    assert [step['command']['kind'] for step in (prepare, start, check)] == ['prepare', 'start', 'status']
    assert prepare['command']['anchor']['trackId'] == '102'
    assert [row['trackId'] for row in prepare['command']['rows']] == ['103', '102', '101']
    assert prepare['reply']['decision'] == 'ready'
    assert prepare['reply']['eligibleCount'] == 2 and prepare['reply']['omittedCount'] == 1
    assert prepare['reply']['receipt'] == start['command']['receipt'] == check['command']['receipt']
    ids = start['reply']['result']['accepted']
    assert len(ids) == 2 and check['reply']['result']['accepted'] == ids
    assert sorted(row['args']['id'] for row in last['searchGroupDownload']
                  if row['kind'] == 'authorize') == ['101', '103']
    assert sorted(row['id'] for row in last['searchGroupDownload']
                  if row['kind'] == 'media') == ['101', '103']
    assert len(last['searchGroupDownload']) == 4
    assert load(output / 'locale.json') == {
        'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8',
    }
    db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
    with closing(sqlite3.connect(db.as_uri() + '?mode=ro', uri=True)) as store:
        tasks = [json.loads(value) for value, in store.execute(
            "SELECT value FROM state WHERE kind='download_tasks_v1' AND owner='2'"
        )]
        tasks.sort(key=lambda row: (row['created_ms'], row['id']))
        assert [row['id'] for row in tasks] == ids
        assert [row['song_id'] for row in tasks] == ['103', '101']
        assert all(row['status'] == 'complete' and row['options']['quality'] == 'exhigh' for row in tasks)
    published = list((root / 'group-output').rglob('*.flac'))
    assert len(published) == 2
    reference = flac_frames(repo / 'core/tests/fixtures/stereo-pcm-source.flac')
    assert all(flac_frames(path) == reference for path in published)
    summary = {'ok': True, 'screenshots': len(LABELS), 'actualWebView': True,
               'orderedTasks': ['103', '101'], 'anchor': '102', 'readOnlyLostReplyRecovery': True,
               'exactFlacFrames': True, 'realAccountWrites': 0, 'realMusicRequests': 0}
    (root / 'report.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(summary))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    require_xvfb()
    run(args)
