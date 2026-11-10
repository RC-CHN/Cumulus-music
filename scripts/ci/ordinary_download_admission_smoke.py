"""Real Linux ordinary download duplicate feedback over the sealed search fixture."""
import argparse
from contextlib import ExitStack, closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3

from search_group_download_smoke import flac_frames
from search_group_next_smoke import TrustedClicks
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

LABELS = (
    'paused_row', 'paused_menu', 'paused_start', 'paused_notice', 'active_manager',
    'fresh_row', 'fresh_menu', 'fresh_start', 'fresh_completed',
    'complete_row', 'complete_menu', 'complete_start', 'completed_notice', 'completed_manager',
)


def load(path):
    return json.loads(path.read_text(encoding='utf-8'))


def digest(path):
    with path.open('rb') as reader:
        return hashlib.file_digest(reader, 'sha256').hexdigest()


def wire(song, quality):
    return [
        {'kind': 'authorize', 'args': {'id': song, 'level': quality, 'immerseType': 'c51'},
         'cached': False},
        {'kind': 'media', 'id': song, 'offset': 0},
    ]


def verify(root, repo):
    output = root / 'first'
    report = load(output / 'complete.json')
    assert report['ok'] and report['stageCount'] == len(LABELS)
    assert report['realAccountWrites'] == report['realMusicRequests'] == 0
    assert load(output / 'locale.json') == {
        'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8',
    }
    records = {}
    for index, label in enumerate(LABELS, 1):
        path = output / f'ux_{index:02}_{label}.json'
        row = records[label] = load(path)
        assert row['trustedInputs'] == index - 1 and path.with_suffix('.png').is_file()
        if label.endswith('_notice'):
            paint = load(path.with_name(f'paint-{path.name}'))
            assert len(paint['samples']) == 2 and all(sample['ok'] for sample in paint['samples'])
        if label.endswith('_start'):
            assert row['heading'] == '下载 1 首歌曲'
        if label != 'completed_manager':
            assert row['pointer']['button'] == (3 if label.endswith('_row') else 1)
    final = records['completed_manager']
    old, fresh = final['old'], final['fresh']
    paused = {'id': old['id'], 'song': '104', 'status': 'paused', 'bytes': 2048, 'quality': 'lossless'}
    assert fresh != old['id'] and old['bytes'] == paused['bytes']
    for label, row in records.items():
        has_completed = LABELS.index(label) >= LABELS.index('fresh_completed')
        inventory = row['records']
        assert row['old'] == old and inventory['active'] == [paused]
        assert inventory['counts'] == [1, int(has_completed)]
        assert row['searchGroupDownload'] == wire('104', 'lossless') + (
            wire('101', 'exhigh') if has_completed else []
        )
        if has_completed:
            assert len(inventory['complete']) == 1
            song = inventory['complete'][0]
            assert (song['id'], song['song'], song['status'], song['quality']) == (
                fresh, '101', 'complete', 'exhigh',
            )
        else:
            assert inventory['complete'] == []

    assert records['paused_notice']['notice'] == '现有任务的设置不同；可按原设置继续，或取消后按新设置下载'
    assert records['active_manager']['selectedTab'] == records['fresh_completed']['selectedTab'] == 'download-tab-active'
    assert records['completed_notice']['notice'] == '资源已下载'
    assert final['selectedTab'] == 'download-tab-song' and final['heading'] is None
    actions = final['actions']
    assert len(actions) == 3
    for action, song in zip(actions, ('104', '101', '101'), strict=True):
        assert action['command']['kind'] == 'start'
        assert action['command']['song_ids'] == [song]
        assert action['command']['options']['quality'] == 'exhigh'
        assert not action['reply']['failed']
    assert not actions[0]['reply']['accepted'] and actions[0]['reply']['skipped'] == [
        {'id': '104', 'reason': 'options_changed'},
    ]
    assert actions[1]['reply']['accepted'] == [fresh] and not actions[1]['reply'].get('skipped')
    assert not actions[2]['reply']['accepted'] and actions[2]['reply']['skipped'] == [
        {'id': '101', 'reason': 'downloaded'},
    ]

    db = (root / 'state/core.sqlite3').resolve(strict=True)
    with closing(sqlite3.connect(db.as_uri() + '?mode=ro', uri=True)) as store:
        saved = [json.loads(value) for value, in store.execute(
            "SELECT value FROM state WHERE kind='download_tasks_v1' AND owner='2'"
        )]
        tasks = {row['song_id']: row for row in saved}
        assert len(saved) == 2 and set(tasks) == {'101', '104'}
        existing, published = tasks['104'], tasks['101']
        assert existing['id'] == old['id'] and existing['created_ms'] == old['createdMs']
        assert existing['status'] == 'paused' and existing['bytes'] == 2048
        assert existing['authorization_count'] == 1 and existing['options']['quality'] == 'lossless'
        assert Path(existing['target']) == root / 'group-previous/original-104.flac'
        raw, = store.execute(
            "SELECT value FROM state WHERE kind='media_transfers_v1' AND owner='2' AND key=?",
            (old['id'],),
        ).fetchone()
        partial = json.loads(raw)
        assert partial['status'] == 'paused' and partial['bytes'] == 2048
        assert published['id'] == fresh and published['status'] == 'complete'
        assert published['authorization_count'] == 1 and published['options']['quality'] == 'exhigh'
        assert not store.execute("SELECT 1 FROM state WHERE kind='playlist-mutations'").fetchall()
    reference = repo / 'core/tests/fixtures/stereo-pcm-source.flac'
    assert Path(partial['part']).read_bytes() == reference.read_bytes()[:2048]
    assert flac_frames(Path(published['target'])) == flac_frames(reference)
    return {
        'ok': True, 'actualWebView': True, 'screenshots': len(LABELS), 'trustedInputs': len(LABELS)-1,
        'statuses': ['options_changed', 'accepted', 'downloaded'], 'exactFlacFrames': True,
        'retainedPausedBytes': 2048, 'nativeAuthorizations': 2, 'tasks': 2,
        'realAccountWrites': 0, 'realMusicRequests': 0,
    }


def run(args):
    repo = Path(__file__).resolve().parents[2]
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    binary, dist = args.binary.resolve(strict=True), args.frontend_dist.resolve(strict=True)
    script = repo / 'desktop/examples/ordinary_download_admission_ui_probe.js'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(
        CUMULUS_PROOF_DIR=str(root), CUMULUS_PROOF_FRONTEND_DIST=str(dist),
        CUMULUS_UX_SCRIPT=str(script), CUMULUS_MOTION_PROOF='1', CUMULUS_SEARCH_UI_PROOF='1',
        CUMULUS_SEARCH_GROUP_NEXT_PROOF='1', CUMULUS_SEARCH_GROUP_DOWNLOAD_PROOF='1',
        CUMULUS_SEARCH_GROUP_DOWNLOAD_DESTINATION_PROOF='1',
        CUMULUS_SEARCH_GROUP_DOWNLOAD_MIXED_PROOF='1',
    )
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
        code = launch(binary, env, log, output, True, before_ack=driver.at_capture)
    assert not (output / 'failed.json').exists(), (
        load(output / 'failed.json') if (output / 'failed.json').exists() else ''
    )
    assert code == 0, code
    summary = verify(root, repo)
    summary.update(binarySha256=digest(binary), frontendIndexSha256=digest(dist / 'index.html'),
                   scriptSha256=digest(script))
    (root / 'report.json').write_text(json.dumps(summary, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(summary, ensure_ascii=False))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    require_xvfb()
    run(args)
