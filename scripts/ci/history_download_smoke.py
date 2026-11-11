"""Exercise the real recently played episode download flow over synthetic services."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import sys

from motion_smoke import launch


STAGES = ['single_choice', 'cancel_no_task', 'restricted', 'batch_choice', 'published', 'duplicates']
RIGHTS = '/api/dj/voices/download/check'
MUSICS = '/api/dj/program/song/musics'
URL = '/api/song/enhance/download/url/v1'
ROOT = Path(__file__).resolve().parents[2]


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def audit(root, output, screenshots):
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stages'] == STAGES
    names = [f'ux_{index:02d}_{stage}' for index, stage in enumerate(STAGES, 1)]
    assert [file.stem for file in sorted(output.glob('ux_*.json'))] == names
    states = {name: read(output / f'{stem}.json') for name, stem in zip(STAGES, names, strict=True)}
    photos = sorted(output.glob('ux_*.png'))
    assert [photo.stem for photo in photos] == (names if screenshots else [])
    hashes = {}
    for photo in photos:
        content = photo.read_bytes()
        assert content.startswith(b'\x89PNG\r\n\x1a\n') and len(content) > 1000
        hashes[photo.name] = hashlib.sha256(content).hexdigest()

    first = [{'path': RIGHTS, 'args': {'voiceIdsStr': ['1']}}, {'path': MUSICS, 'args': {'ids': ['1']}}]
    before = [*first, {'path': RIGHTS, 'args': {'voiceIdsStr': ['3']}}]
    assert states['single_choice']['choices'] == ['standard'] and states['single_choice']['canConfirm']
    assert states['cancel_no_task']['focusRestored'] and states['cancel_no_task']['title'] is None
    assert all(states[name]['historyDownloadCalls'] == first for name in STAGES[:2])
    assert states['restricted']['choices'] == [] and not states['restricted']['canConfirm']
    assert states['batch_choice']['choices'] == ['standard', 'higher', 'exhigh']
    assert states['batch_choice']['canConfirm'] and states['batch_choice']['selected'] == 2
    for name in ('single_choice', 'restricted', 'batch_choice', 'duplicates'):
        state = states[name]
        assert state['title'] == '选择下载音质' and state['compact']
        assert not state['showsDirectory'] and not state['showsMetadata']
    assert all(states[name]['historyDownloadCalls'] == before for name in STAGES[2:4])
    assert all(not states[name]['active'] and not states[name]['tasks'] for name in STAGES[:4])

    finished = states['published']['historyDownloadCalls']
    assert finished[:len(before)] == before
    tail = finished[len(before):]
    assert sorted(call['args']['voiceIdsStr'] for call in tail if call['path'] == RIGHTS) == [['1'], ['2']]
    assert sorted(call['args']['id'] for call in tail if call['path'] == URL) == ['1001', '1002']
    assert len(tail) == 6 and sum(call['path'] == 'transfer' for call in tail) == 2
    duplicates = states['duplicates']
    assert not duplicates['canConfirm'] and duplicates['choices'] == []
    assert duplicates['historyDownloadCalls'] == complete['historyDownloadCalls'] == finished
    tasks = states['published']['tasks']
    assert tasks == duplicates['tasks'] and len(tasks) == 2
    assert not states['published']['active'] and not duplicates['active']
    audio = (ROOT / 'core/tests/fixtures/tag_audio/seed.mp3').read_bytes()
    paths = set()
    destination = (root / 'run/media').resolve(strict=True)
    for index, task in enumerate(tasks, 1):
        path = Path(task['path'])
        assert not path.is_symlink()
        path = path.resolve(strict=True)
        assert path.is_relative_to(destination)
        assert (task['voice'], task['media'], task['quality'], task['source']) == (
            str(index), str(1000 + index), 'standard', 'voice',
        )
        assert not task['encrypted'] and task['bytes'] == len(audio) and path.read_bytes() == audio
        paths.add(path)
    assert len(paths) == 2
    assert all(state['syntheticHistoryWrites'] == 0 for state in [*states.values(), complete])
    result = dict(ok=True, actualWebView=sys.platform, stages=len(names), screenshots=hashes,
                  publishedEpisodes=2, realMusicRequests=0, realAccountWrites=0)
    if sys.platform.startswith('linux'):
        locale = read(output / 'locale.json')
        assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
        result.update(locale=locale, network='loopback-only')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    linux = sys.platform.startswith('linux')
    if linux:
        if (not os.environ.get('DISPLAY', '').startswith(':')
                or {name for _, name in socket.if_nameindex()} != {'lo'}
                or len(Path('/proc/net/route').read_text(encoding='ascii').splitlines()) != 1):
            parser.error('use X11 in a sealed Linux network namespace containing only loopback')
    elif sys.platform != 'win32' or not args.runtime or args.screenshots:
        parser.error('use Windows with a verified runtime, or Linux X11 for screenshots')
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    env.update(LC_ALL='en_US.UTF-8', CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_HISTORY_PROOF='1',
               CUMULUS_HISTORY_DOWNLOAD_PROOF='1',
               CUMULUS_UX_SCRIPT=str(ROOT / 'desktop/examples/history_download_ui_probe.js'))
    if linux:
        env['GDK_BACKEND'] = 'x11'
    if args.runtime:
        env['MPV_DLL_DIR_CUMULUS'] = str(args.runtime.resolve(strict=True))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / key
        path.mkdir(mode=0o700)
        env[key] = str(path)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, args.screenshots, acknowledge_receipts=True)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert status == 0, f'native probe exit {status}'
    report = audit(root, output, args.screenshots)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
