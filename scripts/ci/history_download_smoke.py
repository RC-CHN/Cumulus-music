"""Exercise the real recently played episode download flow over synthetic services."""

import argparse
from contextlib import nullcontext
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

from local_folder_reveal_smoke import fake_file_manager
from motion_smoke import launch
from playback_power_smoke import windows_runtime
from voice_song_download_smoke import published_mp3


STAGES = ['single_choice', 'cancel_no_task', 'restricted', 'batch_choice', 'published', 'duplicates']
ROW_STAGES = [
    'row_download_offered', 'row_single_choice', 'row_cancelled', 'row_group_choice',
    'row_published', 'row_already_downloaded', 'row_reveal_offered', 'row_reveal_returned', 'row_restricted',
    'row_restricted_closed',
]
RIGHTS = '/api/dj/voices/download/check'
MUSICS = '/api/dj/program/song/musics'
URL = '/api/song/enhance/download/url/v1'
ROOT = Path(__file__).resolve().parents[2]
PRIVATE_ROW_BUS = 'CUMULUS_HISTORY_VOICE_ROW_PRIVATE_BUS'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def captures(output, screenshots, stages):
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stages'] == stages
    names = [f'ux_{index:02d}_{stage}' for index, stage in enumerate(stages, 1)]
    assert [file.stem for file in sorted(output.glob('ux_*.json'))] == names
    states = {name: read(output / f'{stem}.json') for name, stem in zip(stages, names, strict=True)}
    photos = sorted(output.glob('ux_*.png'))
    assert [photo.stem for photo in photos] == (names if screenshots else [])
    hashes = {}
    for photo in photos:
        content = photo.read_bytes()
        assert content.startswith(b'\x89PNG\r\n\x1a\n') and len(content) > 1000
        hashes[photo.name] = hashlib.sha256(content).hexdigest()
    return complete, states, hashes


def publications(root, tasks):
    assert len(tasks) == 2
    audio = (ROOT / 'core/tests/fixtures/tag_audio/seed.mp3').read_bytes()
    paths = {}
    destination = root / 'run/media/电台节目'
    for index, task in enumerate(tasks, 1):
        path = published_mp3(task['path'], destination)
        assert (task['voice'], task['media'], task['quality'], task['source']) == (
            str(index), str(1000 + index), 'standard', 'voice',
        )
        assert not task['encrypted'] and task['bytes'] == len(audio) and path.read_bytes() == audio
        paths[task['voice']] = path
    assert len(set(paths.values())) == 2
    return paths


def install_fallback_guard(root, env):
    directory = root / 'fake-bin'
    directory.mkdir(mode=0o700)
    opener = directory / 'xdg-open'
    opener.write_text(
        '#!/usr/bin/python3\nfrom pathlib import Path\n'
        f'Path({str(root / "unexpected-file-manager-fallback")!r}).touch()\n',
        encoding='utf-8',
    )
    opener.chmod(0o700)
    env['PATH'] = str(directory) + os.pathsep + env.get('PATH', '')


def verify_show_items(root, tasks, *, wait=False):
    logfile = root / 'show-items.jsonl'
    if wait:
        deadline = time.monotonic() + 4
        while not logfile.exists() or not logfile.stat().st_size:
            assert time.monotonic() < deadline, 'no private FileManager1 ShowItems receipt'
            time.sleep(.04)
    paths = publications(root, tasks)
    calls = [json.loads(line) for line in logfile.read_text(encoding='utf-8').splitlines()]
    assert calls == [[[paths['1'].resolve(strict=True).as_uri()], '']], calls
    assert not (root / 'unexpected-file-manager-fallback').exists()


def audit_row(root, output, screenshots, runtime):
    complete, states, hashes = captures(output, screenshots, ROW_STAGES)
    assert complete['row']
    assert all(state['syntheticHistoryWrites'] == 0 for state in [*states.values(), complete])
    offered, choice, cancelled, group, published, existing, reveal, returned, denied, closed = states.values()
    original_menu = ['play', 'next', 'comments', 'download', 'copy', 'remove']
    finished_menu = ['play', 'next', 'comments', 'copy', 'reveal', 'remove']
    for states_in_case, clicked, selected, keys in (
        ((offered, choice, cancelled), '1', ['1'], original_menu),
        ((group, published), '1', ['2', '1'], original_menu),
        ((existing,), '3', ['2', '1'], original_menu[:-1]),
        ((reveal, returned), '1', ['2', '1'], finished_menu),
        ((denied, closed), '3', ['1', '3'], original_menu),
    ):
        for state in states_in_case:
            assert (state['clicked'], state['selectedIds'], state['menuKeys']) == (
                clicked, selected, keys,
            )
    for state in (choice, group, denied):
        assert state['title'] == '选择下载音质' and state['compact']
        assert not state['showsDirectory'] and not state['showsMetadata']
    assert choice['choices'] == ['standard'] and choice['canConfirm']
    assert group['choices'] == ['standard', 'higher', 'exhigh'] and group['canConfirm']
    assert denied['choices'] == [] and not denied['canConfirm']
    assert '当前节目不支持普通下载' in denied['status']
    for state in (cancelled, existing, returned, closed):
        assert state['focusRestored'] and state['title'] is None
    assert published['stayedInHistory'] and returned['stayedInHistory']
    assert published['notice'] == '已添加 2 个单集到下载列表'
    assert existing['gesture'] == 'ctrl-context' and existing['notice'] == '资源已下载'
    assert existing['mountedQuality'] is False
    assert all(not state['active'] for state in states.values())
    assert all(not state['tasks'] for state in (offered, choice, cancelled, group))
    assert all(state['tasks'] == published['tasks'] for state in (existing, reveal, returned, denied, closed))

    first = [{'path': RIGHTS, 'args': {'voiceIdsStr': ['1']}}, {'path': MUSICS, 'args': {'ids': ['1']}}]
    assert offered['historyDownloadCalls'] == []
    assert all(state['historyDownloadCalls'] == first for state in (choice, cancelled, group))
    finished = published['historyDownloadCalls']
    assert finished[:len(first)] == first
    tail = finished[len(first):]
    assert sorted(call['args']['voiceIdsStr'] for call in tail if call['path'] == RIGHTS) == [['1'], ['2']]
    assert sorted(call['args']['id'] for call in tail if call['path'] == URL) == ['1001', '1002']
    assert len(tail) == 6 and sum(call['path'] == 'transfer' for call in tail) == 2
    assert existing['historyDownloadCalls'] == reveal['historyDownloadCalls'] == returned['historyDownloadCalls'] == finished
    restricted = [*finished, {'path': RIGHTS, 'args': {'voiceIdsStr': ['3']}}]
    assert denied['historyDownloadCalls'] == closed['historyDownloadCalls'] == complete['historyDownloadCalls'] == restricted
    publications(root, published['tasks'])
    native_shell = runtime is None
    assert returned['fileManagerDispatched'] is native_shell
    if native_shell:
        verify_show_items(root, published['tasks'])
    else:
        assert not (root / 'show-items.jsonl').exists()
    report = dict(ok=True, actualWebView='WebKitGTK' if native_shell else 'WebView2',
                  stages=len(states), screenshots=hashes, publishedEpisodes=2,
                  cancelledWithNoTask=True, noChooserForCompletedGroup=True,
                  stayedInHistory=True, exactRevealOffer='1',
                  nativeFileManagerDispatched=native_shell,
                  realFileManagerOpened=False, realMusicRequests=0, realAccountWrites=0)
    if native_shell:
        locale = read(output / 'locale.json')
        assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
        report.update(locale=locale, network='loopback-only', fakeFileManager1Calls=1)
    else:
        report.update(runtime=runtime, network='synthetic-transports-only')
    return report


def audit(root, output, screenshots):
    complete, states, hashes = captures(output, screenshots, STAGES)

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
    publications(root, tasks)
    assert all(state['syntheticHistoryWrites'] == 0 for state in [*states.values(), complete])
    result = dict(ok=True, actualWebView=sys.platform, stages=len(states), screenshots=hashes,
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
    parser.add_argument('--row', action='store_true', help='verify browse voice row Download and Locate')
    parser.add_argument('--private-row-child', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    linux = sys.platform.startswith('linux')
    if linux:
        if (not os.environ.get('DISPLAY', '').startswith(':')
                or {name for _, name in socket.if_nameindex()} != {'lo'}
                or len(Path('/proc/net/route').read_text(encoding='ascii').splitlines()) != 1):
            parser.error('use X11 in a sealed Linux network namespace containing only loopback')
        if args.row and not args.screenshots:
            parser.error('Linux row proof requires screenshots for the sealed FileManager1 acknowledgement')
    elif sys.platform != 'win32' or not args.runtime or args.screenshots:
        parser.error('use Windows with a verified runtime, or Linux X11 for screenshots')
    if args.private_row_child and (not linux or not args.row or os.environ.get(PRIVATE_ROW_BUS) != '1'
                                   or not os.environ.get('DBUS_SESSION_BUS_ADDRESS', '').startswith('unix:')):
        parser.error('the private D-Bus child is Linux row proof only')
    if args.row and linux and not args.private_row_child:
        env = {key: value for key, value in os.environ.items()
               if key not in ('DBUS_SESSION_BUS_ADDRESS', 'AT_SPI_BUS_ADDRESS', 'AT_SPI_DISPLAY')}
        env[PRIVATE_ROW_BUS] = '1'
        subprocess.run(['dbus-run-session', '--', sys.executable, __file__, *sys.argv[1:],
                        '--private-row-child'], env=env, check=True, timeout=195)
        return
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    script = ROOT / 'desktop/examples/history_download_ui_probe.js'
    env.update(LC_ALL='en_US.UTF-8', CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_HISTORY_PROOF='1',
               CUMULUS_HISTORY_DOWNLOAD_PROOF='1',
               CUMULUS_UX_SCRIPT=str(script))
    if linux:
        env['GDK_BACKEND'] = 'x11'
    runtime = None
    if args.row:
        env = {key: value for key, value in env.items()
               if not key.upper().startswith('WEBVIEW2_') and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
        mode_script = root / 'row-ui.js'
        mode_script.write_text('window.__historyVoiceRowDownloadProof = '
                               + json.dumps(dict(fileManager=linux)) + ';\n'
                               + script.read_text(encoding='utf-8'), encoding='utf-8')
        env.update(CUMULUS_UX_SCRIPT=str(mode_script), CUMULUS_HISTORY_VOICE_ROW_DOWNLOAD_PROOF='1')
        if linux:
            install_fallback_guard(root, env)
        else:
            stage, runtime = windows_runtime(binary, args.runtime)
            env['MPV_DLL_DIR_CUMULUS'] = str(stage)
    elif args.runtime:
        env['MPV_DLL_DIR_CUMULUS'] = str(args.runtime.resolve(strict=True))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    profile = (('APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP') if args.row and not linux else
               ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'))
    for key in profile:
        path = root / key
        path.mkdir(mode=0o700)
        env[key] = str(path)
    output = root / 'run/first'
    shell = fake_file_manager(root, env, exact_successes=1) if args.row and linux else nullcontext()
    def before_ack(path, value, _pid):
        if path.stem.endswith('_row_reveal_returned'):
            verify_show_items(root, value['tasks'], wait=True)
    with shell, (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, args.screenshots, acknowledge_receipts=True,
                        before_ack=before_ack if args.row and linux else None)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert status == 0, f'native probe exit {status}'
    report = (audit_row(root, output, args.screenshots, runtime) if args.row else
              audit(root, output, args.screenshots))
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
