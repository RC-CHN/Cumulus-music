"""Playback cache controls through actual WebView/IPC, with isolated local state."""

import argparse
import json
import os
from pathlib import Path
import sqlite3

from window_manager import launch


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def write_script(path, source, *, reopen=False, blocked=False, old_revision=None):
    path.write_text(
        'window.__cacheReopen = ' + json.dumps(reopen) + ';\n'
        + 'window.__cacheBlocked = ' + json.dumps(blocked) + ';\n'
        + 'window.__oldCacheRevision = ' + json.dumps(old_revision) + ';\n' + source,
        encoding='utf-8',
    )


def run(args, root, source, stages, *, reopen=False, blocked=False, old_revision=None):
    label = 'reopen' if reopen else 'first'
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_PLAYBACK_CACHE_PROOF='1', GDK_BACKEND='x11', LC_ALL='en_US.UTF-8')
    if reopen:
        env['CUMULUS_PROOF_REOPEN'] = '1'
    if blocked:
        env['CUMULUS_PLAYBACK_CACHE_BLOCKED'] = '1'
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    for name in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                 'APPDATA', 'LOCALAPPDATA'):
        folder = root / 'os-profile' / name.lower()
        folder.mkdir(parents=True, mode=0o700, exist_ok=True)
        env[name] = str(folder)
    script = root / f'{label}.js'
    write_script(script, source, reopen=reopen, blocked=blocked, old_revision=old_revision)
    env['CUMULUS_UX_SCRIPT'] = str(script)

    out = root / label
    with (root / f'{label}.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, out, args.screenshots)
    failed = out / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    complete = read(out / 'complete.json')
    assert code == 0 and complete['ok'] and complete['stageCount'] == len(stages), complete
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['stockPlayed'] == blocked
    assert len(list(out.glob('ux_*.png'))) == (len(stages) if args.screenshots else 0)
    return [read(out / f'ux_{index:02}_{name}.json') for index, name in enumerate(stages, 1)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if os.name == 'nt' and args.screenshots:
        parser.error('screenshots require the Linux X11 probe')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    source = (Path(__file__).resolve().parents[2] / 'desktop/examples/playback_cache_ui_probe.js')
    source = source.read_text(encoding='utf-8')
    profile = root / 'normal'
    directory = profile / 'state/streaming-audio-v1'
    directory.mkdir(parents=True, mode=0o700)
    # The process has never owned either file. Clear must not sweep by path,
    # even for an unfamiliar file placed inside the app's cache directory.
    contents = b'not owned by the playback byte cache\n'
    sentinels = (directory / 'unknown-file', profile / 'state/neighbor-media')
    for sentinel in sentinels:
        sentinel.write_bytes(contents)
    initial, capacity, cleared = run(args, profile, source, ('initial', 'capacity', 'cleared'))
    old_revision = cleared['view']['revision']
    reopened, = run(args, profile, source, ('reopened',), reopen=True, old_revision=old_revision)
    assert all(Path(item['view']['directory']).samefile(directory)
               for item in (initial, capacity, cleared, reopened))
    assert initial['view']['capacityBytes'] == 1024 ** 3
    assert capacity['view']['capacityBytes'] == cleared['view']['capacityBytes'] == (
        reopened['view']['capacityBytes']) == 1_610_612_736
    assert len({item['view']['revision'] for item in (initial, capacity, cleared, reopened)}) == 4
    assert all(sentinel.read_bytes() == contents for sentinel in sentinels)
    with sqlite3.connect((profile / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        record = db.execute('SELECT value FROM state WHERE owner=? AND kind=? AND key=?',
                            ('local-device', 'streaming-media-cache-preferences-v1', 'current'))
        assert json.loads(record.fetchone()[0]) == {'revision': 1, 'capacityBytes': 1_610_612_736}

    blocked = root / 'blocked'
    blocked_path = blocked / 'state/streaming-audio-v1'
    blocked_path.parent.mkdir(parents=True)
    blocked_path.write_bytes(contents)
    unavailable, = run(args, blocked, source, ('unavailable',), blocked=True)
    assert unavailable['stockPlayed'] and blocked_path.read_bytes() == contents
    report = dict(ok=True, actualWebView=True, actualNativeStockFallback=True,
                  capacitySurvivedReopen=True, oldRevisionRejected=True,
                  foreignFilesUnchanged=True, syntheticCachedMediaBytes=0,
                  screenshots=5 if args.screenshots else 0, realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
