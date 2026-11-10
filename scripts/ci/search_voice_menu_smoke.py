"""Exercise the voice More menus and native directory handoff in Linux."""
import argparse
from contextlib import closing, nullcontext
import ctypes
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from local_folder_reveal_smoke import fake_file_manager
from search_queue_drop_smoke import TrustedDrag
from window_manager import launch


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


QUEUE_STAGES = ('playing_menu', 'paused', 'fallback_menu', 'escaped', 'remove_menu', 'removed',
                'resume_menu', 'resumed', 'directory_menu', 'directory_closed')


def queue_input(driver, stage, payload):
    if stage == 'ux_02_paused':
        driver.move(payload['pointer'])
        for pressed in (1, 0):
            assert driver.xtest.XTestFakeButtonEvent(driver.display, 3, pressed, 0)
            driver.x11.XSync(driver.display, 0)
            time.sleep(.04)
    elif stage == 'ux_03_fallback_menu':
        driver.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        driver.x11.XKeysymToKeycode.restype = ctypes.c_ubyte
        driver.xtest.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        escape = driver.x11.XKeysymToKeycode(driver.display, 0xff1b)
        assert escape, 'X11 Escape keycode'
        for pressed in (1, 0):
            assert driver.xtest.XTestFakeKeyEvent(driver.display, escape, pressed, 0)
            driver.x11.XSync(driver.display, 0)
    elif stage in ('ux_01_playing_menu', 'ux_05_remove_menu', 'ux_07_resume_menu', 'ux_09_directory_menu'):
        driver.move(payload['pointer'])
        driver.button(True)
        time.sleep(.04)
        driver.button(False)


def queue_result(root, output, rows, complete, locale):
    before = rows['playing_menu']['playback']
    paused = rows['paused']['playback']
    assert complete['trusted'] == dict(pause=1, play=1, right=1, escape=1, remove=1, directory=1)
    assert complete['audioFactoryCount'] == 1
    assert dict(rows['playing_menu']['items'])['play'] == '暂停'
    assert dict(rows['playing_menu']['items'])['comments'] == '查看评论(54)'
    assert dict(rows['fallback_menu']['items'])['comments'] == '查看评论'
    assert dict(rows['resume_menu']['items'])['play'] == '播放'
    initial = rows['playing_menu']['queue']
    removed = rows['removed']['queue']
    assert initial['revision'] != removed['revision']
    for index, (label, record) in enumerate(rows.items()):
        view, queue = record['playback'], record['queue']
        assert view['playId'] == before['playId'] and view['entryId'] == before['entryId']
        assert view['state'] == ('playing' if index == 0 or index >= 7 else 'paused')
        if view['state'] == 'paused':
            assert view['position'] == paused['position'], label
        assert queue == (initial if index < 5 else removed), label
        assert [row['track']['voiceId'] for row in queue['tracks']] == (['401', '402'] if index < 5 else ['401'])
    assert rows['escaped']['focused'] == initial['tracks'][1]['entryId']
    assert rows['removed']['focused'] == rows['directory_closed']['focused'] == before['entryId']
    assert rows['resumed']['playback']['position'] > paused['position']
    for stage in ('01_playing_menu', '03_fallback_menu', '05_remove_menu', '07_resume_menu', '09_directory_menu'):
        samples = read(output / f'paint-ux_{stage}.json')['samples']
        assert len(samples) == 1 and samples[0]['ok'], stage
    leaf = root / 'episode files' / 'synthetic-voice.flac'
    assert leaf.is_file()
    calls = [json.loads(line) for line in (root / 'show-items.jsonl').read_text(encoding='utf-8').splitlines()]
    assert calls == [[[leaf.resolve().as_uri()], '']]
    assert not (root / 'folder-argv.txt').exists(), 'FileManager1 success must not fall back'
    assert complete['voiceMenuCalls'] == [
        {'resourceType': 1, 'resourceIds': f'["{id_}"]'} for id_ in (401, 401, 402, 402, 401, 401)]
    # Only the initial voice load and second-row queue insertion resolve sources.
    assert complete['voiceNextCalls'] == [
        ['/api/dj/program/detail', {'id': '401'}],
        ['/api/dj/program/detail', {'id': '401'}],
        ['/api/dj/play/url/check', {'voiceId': '401'}],
        ['/api/dj/program/detail', {'id': '402'}],
    ]
    return dict(ok=True, actualWebView=True, states=QUEUE_STAGES, screenshots=len(QUEUE_STAGES),
                locale=locale, sameDecoder=True, exactFileManager1Calls=1, trusted=complete['trusted'],
                realMusicRequests=0, realAccountWrites=0, wire=complete['voiceMenuCalls'],
                sourcePreparation=complete['voiceNextCalls'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('binary', 'root', 'frontend-dist'):
        parser.add_argument('--' + name, type=Path, required=True)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--play', action='store_true', help='check explicit Search Menu Play versus row Pause')
    mode.add_argument('--queue', action='store_true', help='check the ordinary queue voice More menu')
    parser.add_argument('--queue-private-bus', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if sys.platform != 'linux' or not os.environ.get('DISPLAY'):
        parser.error('requires Linux Xvfb')
    assert len(Path('/proc/net/route').read_text().splitlines()) == 1, 'requires a sealed network namespace'
    if args.queue and not args.queue_private_bus:
        # Do not claim the file-manager name on a user's desktop bus.
        env = {k: v for k, v in os.environ.items()
               if k not in ('DBUS_SESSION_BUS_ADDRESS', 'AT_SPI_BUS_ADDRESS', 'AT_SPI_DISPLAY')}
        subprocess.run(['dbus-run-session', '--', sys.executable, __file__,
                        *sys.argv[1:], '--queue-private-bus'],
                       env=env, check=True, timeout=195)
        return
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    script = ('queue_voice_menu_ui_probe.js' if args.queue else 'search_voice_menu_play_ui_probe.js'
              if args.play else 'search_voice_menu_ui_probe.js')
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_SEARCH_UI_PROOF='1',
               CUMULUS_SEARCH_VOICE_MENU_PROOF='1', LC_ALL='en_US.UTF-8', GDK_BACKEND='x11',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] /
                                     'desktop/examples' / script),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)))
    if args.queue:
        env['CUMULUS_QUEUE_VOICE_FILE_PROOF'] = '1'
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        folder = root / key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)
    launcher = root / 'bin' / 'xdg-open'
    launcher.parent.mkdir(mode=0o700)
    launcher.write_text('#!/bin/sh\nprintf "%s\\n" "$@" >> "$CUMULUS_PROOF_DIR/folder-argv.txt"\n',
                        encoding='utf-8')
    launcher.chmod(0o700)
    env['PATH'] = str(launcher.parent) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    shell = fake_file_manager(root, env, once=True) if args.queue else nullcontext()
    with shell, closing(TrustedDrag()) as pointer, (root / 'host.log').open('x', encoding='utf-8') as log:
        def input_at_capture(path, payload, _pid):
            if args.queue:
                return queue_input(pointer, path.stem, payload)
            targets = (('ux_01_playing_menu', 'ux_03_row_pause', 'ux_04_paused_menu') if args.play else
                       ('ux_01_completed_menu',))
            if path.stem in targets:
                pointer.move(payload['pointer'])
                pointer.button(True)
                time.sleep(.04)
                pointer.button(False)
        code = launch(args.binary.resolve(strict=True), env, log, output, True, before_ack=input_at_capture)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    complete = read(output / 'complete.json')
    stages = (QUEUE_STAGES if args.queue else
              ('playing_menu', 'playing_kept', 'row_pause', 'paused_menu', 'resumed') if args.play else
              ('completed_menu', 'folder_closed', 'classified_fallback', 'late_ignored'))
    rows = {path.stem.split('_', 2)[2]: read(path) for path in sorted(output.glob('ux_*.json'))}
    assert code == 0 and complete['ok'] and complete['stageCount'] == len(stages)
    assert tuple(rows) == stages and len(list(output.glob('ux_*.png'))) == len(stages)
    locale = read(output / 'locale.json')
    assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    if args.queue:
        write_report(root, queue_result(root, output, rows, complete, locale))
        return
    if args.play:
        assert rows['playing_menu']['label'] == rows['paused_menu']['label'] == '播放'
        assert rows['playing_kept']['trustedMenu'] == rows['resumed']['trustedIndex'] == 1
        assert rows['resumed']['trustedMenu'] == 2 and complete['audioFactoryCount'] == 1
        fields = ('playId', 'entryId', 'queueRevision')
        original = rows['playing_menu']['playback']
        paused = rows['resumed']['paused']
        assert paused['state'] == 'paused' and paused['position'] is not None
        assert all(paused[field] == original[field] for field in fields)
        for stage, row in rows.items():
            current = row['playback']
            assert current['state'] == ('paused' if stage == 'paused_menu' else 'playing')
            assert current['track']['voiceId'] == '401'
            assert current['queueLen'] == 1 and all(current[field] == original[field] for field in fields)
        assert rows['resumed']['playback']['position'] > paused['position']
        assert complete['voiceNextCalls'] == [
            ['/api/dj/program/detail', {'id': '401'}],
            ['/api/dj/play/url/check', {'voiceId': '401'}],
        ]
        # Three menu preparations and one current-player badge all read the same program count.
        assert complete['voiceMenuCalls'] == [{'resourceType': 1, 'resourceIds': '["401"]'}] * 4
        for stage in ('01_playing_menu', '04_paused_menu'):
            samples = read(output / f'paint-ux_{stage}.json')['samples']
            assert len(samples) == 1 and samples[0]['ok'], stage
        report = dict(ok=True, actualWebView=True, states=stages, screenshots=len(stages), locale=locale,
                      sameDecoder=True, trustedMenuPlay=2, trustedIndexPause=1, paintedMenus=2,
                      realMusicRequests=0, realAccountWrites=0, wire=complete['voiceNextCalls'])
        write_report(root, report)
        return
    for stage in ('01_completed_menu', '03_classified_fallback', '04_late_ignored'):
        samples = read(output / f'paint-ux_{stage}.json')['samples']
        assert len(samples) == (2 if stage == '01_completed_menu' else 1)
        assert all(sample['ok'] for sample in samples), stage
    assert rows['completed_menu']['count'] == 54 and complete['trustedFolderClicks'] == 1
    assert rows['folder_closed']['triggerFocused'] and rows['classified_fallback']['plain']
    assert rows['late_ignored']['focusKept'] and rows['late_ignored']['freshRow'] == '402'
    assert (root / 'folder-argv.txt').read_text(encoding='utf-8').splitlines() == [str(root / 'episode files')]
    assert complete['voiceMenuCalls'] == [
        {'resourceType': 1, 'resourceIds': f'["{id_}"]'} for id_ in (401, 402, 401, 402)]
    report = dict(ok=True, actualWebView=True, states=stages, screenshots=len(stages), locale=locale,
                  directoryArgvVerified=True, trustedFolderClick=True, pendingReplyRetired=True, paintedMenus=3,
                  realMusicRequests=0, realAccountWrites=0, wire=complete['voiceMenuCalls'])
    write_report(root, report)


def write_report(root, report):
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n',
                                     encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
