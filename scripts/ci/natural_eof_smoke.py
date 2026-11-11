"""Verify natural EOF in the actual PlayerBar, Tauri, SQLite, and null libmpv."""
import argparse
from contextlib import closing, contextmanager, nullcontext
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import sys

from minibar_skip_smoke import check_skip_notices, read
from motion_smoke import launch as launch_webview
from playback_power_smoke import windows_runtime
from window_manager import launch


ROOT = Path(__file__).resolve().parents[2]
PUBLISHED_FLAC = ROOT / 'core/tests/fixtures/held_audio/search-playback-8s.flac'
PUBLISHED_FLAC_DIGEST = '8bd8b67fc43432ae445548a57d59842ec82843a28343fc380c7c11197d949535'
DOWNLOAD_CASES = ('t1-download', 't1-download-stale')
CASES = {
    't1-skip': ('t1', '3', 'playing', 'sequential', ['1', '2', '3']),
    'control': ('c', '1', 'ended', 'sequential', ['1', '2', '3']),
    't1-tail': ('t1', '1', 'idle', 'sequential', ['1', '2', '3']),
    'order-loop-skip': ('t1', '3', 'playing', 'order_loop', ['2', '3', '1']),
    'random-skip': ('t1', '3', 'playing', 'random', ['1', '3', '2']),
    'random-control': ('c', '1', 'ended', 'random', ['1', '2', '3']),
    't1-download': ('t1', '3', 'playing', 'sequential', ['1', '2', '3']),
    't1-download-stale': ('t1', '1', 'ended', 'sequential', ['1', '2', '3']),
}
WIRE = '/api/song/enhance/player/url/v1'
# The notice is centered inside the actual main window. Keep the pointer at
# the primary display's outer corner; the DOM witness also rejects hover.
POINTER_PARK = (4, 4)


class WindowsCursor:
    def __init__(self):
        user32 = ctypes.WinDLL('user32', use_last_error=True)
        self.get = user32.GetCursorPos
        self.get.argtypes = [ctypes.POINTER(wintypes.POINT)]
        self.get.restype = wintypes.BOOL
        self.set = user32.SetCursorPos
        self.set.argtypes = [ctypes.c_int, ctypes.c_int]
        self.set.restype = wintypes.BOOL

    def position(self):
        point = wintypes.POINT()
        if not self.get(ctypes.byref(point)):
            raise ctypes.WinError(ctypes.get_last_error())
        return point.x, point.y

    def move(self, position):
        if not self.set(*position):
            raise ctypes.WinError(ctypes.get_last_error())


@contextmanager
def parked_windows_cursor(cursor):
    original = cursor.position()
    cursor.move(POINTER_PARK)
    try:
        if cursor.position() != POINTER_PARK:
            raise RuntimeError('could not park the Windows proof cursor')
        yield POINTER_PARK
    finally:
        # Don't overwrite movement that occurred after the proof parked it.
        if cursor.position() == POINTER_PARK:
            cursor.move(original)


def check_download(directory, connection, case, before, after, complete, saved):
    """Tie playback back to the exact same-owner frozen fixture and durable queue."""
    stale = case == 't1-download-stale'
    frozen = PUBLISHED_FLAC.read_bytes()
    assert len(frozen) == 38531 and hashlib.sha256(frozen).hexdigest() == PUBLISHED_FLAC_DIGEST
    expected = bytearray(frozen)
    if stale:
        expected[-16] ^= 1
    file = directory.parent / 'published-eof' / 'song-3.flac'
    assert not file.parent.is_symlink() and file.is_file() and not file.is_symlink()
    assert file.read_bytes() == expected

    publication = before['downloadPublication']
    task_id = publication['taskId']
    assert isinstance(task_id, str) and task_id
    assert publication == {
        'owner': '2', 'songId': '3', 'taskId': task_id, 'bytes': len(frozen),
        'sha256': PUBLISHED_FLAC_DIGEST, 'fileSha256': hashlib.sha256(expected).hexdigest(),
        'stale': stale, 'intact': not stale, 'authorizationCount': 1,
    }, publication
    assert after['downloadPublication'] == complete['downloadPublication'] == publication
    task_rows = connection.execute(
        "SELECT owner, key, value FROM state WHERE kind='download_tasks_v1'"
    ).fetchall()
    assert len(task_rows) == 1 and task_rows[0][:2] == ('2', task_id), task_rows
    task = json.loads(task_rows[0][2])
    integrity = {'bytes': len(frozen), 'sha256': PUBLISHED_FLAC_DIGEST}
    assert (task['id'], task['song_id'], task['status'], task['encrypted']) == (
        task_id, '3', 'complete', False,
    )
    assert task['authorization_count'] == 1 and task['output_integrity'] == integrity
    assert Path(task['target']).samefile(file), task['target']
    offline_rows = connection.execute(
        "SELECT owner, value FROM state WHERE kind='offline_music_v1'"
    ).fetchall()
    assert len(offline_rows) == 1 and offline_rows[0][0] == '2', offline_rows
    offline = json.loads(offline_rows[0][1])
    assert (offline['task_id'], offline['song_id'], offline['encrypted']) == (task_id, '3', False)
    assert offline['integrity'] == integrity and Path(offline['path']).samefile(file)

    initial, settled = before['durableQueue'], after['durableQueue']
    keys = ('mode', 'order', 'current', 'items')
    assert [entry['entry_id'] for entry in initial['items']] == [entry for _, entry in before['ids']]
    assert initial == {
        'mode': 'sequential', 'order': [entry for _, entry in before['ids']],
        'current': before['current'], 'items': initial['items'],
    }, initial
    assert settled == {**initial, 'current': after['current']} == complete['durableQueue']
    assert settled == {key: saved[key] for key in keys}
    return {'exactFrozenPublication': not stale, 'sameSizeMutationRejected': stale,
            'winnerAuthorizationRequests': 0, 'nativeHeldFlac': not stale,
            'queueRowsAndOrderPreserved': True}


def check(directory, case, capture, windows):
    group, target, state, mode, display = CASES[case]
    skip = state == 'playing'
    publication = case in DOWNLOAD_CASES
    target_duration = 8 if publication else 30
    before = read(directory / 'ux_01_before.json')
    after = read(directory / 'ux_02_after.json')
    stages = [before, after]
    names = ['ux_01_before', 'ux_02_after']
    if case in ('control', 'random-control'):
        names.append('ux_03_control_next')
    if case == 'random-control':
        names.extend(('ux_04_random_mode', 'ux_05_random_next'))
    stages.extend(read(directory / (name + '.json')) for name in names[2:])
    assert sorted(path.stem for path in directory.glob('ux_*.json')) == names
    complete = read(directory / 'complete.json')
    assert complete['ok'] and complete['case'] == case and complete['group'] == group
    assert complete['target'] == target and complete['stageCount'] == len(stages), complete
    if windows and skip:
        for checkpoint in (after, complete):
            assert checkpoint['skipPointer'] == {'hovered': False, 'events': []}, checkpoint['skipPointer']
    notices = check_skip_notices(stages, complete, skip)
    assert before['ids'] == after['ids'] and [row[0] for row in before['ids']] == display
    entries = dict(before['ids'])
    for row, track, status in ((before, '1', 'paused'), (after, target, state)):
        player, dom = row['player'], row['dom']
        assert row['case'] == case and row['group'] == group and row['actual'] and row['online']
        assert player['trackId'] == dom['trackId'] == track, row
        assert player['state'] == dom['state'] == status and player['error'] is None, row
        assert player['playId'] == dom['playId'] and player['mode'] == mode, row
        assert player['entryId'] == row['current'] == entries[track], row
        assert dom['unread'] and dom['title'] == f'合成自然结束歌曲 {track}', dom
        assert dom['primaryLabel'] == ('暂停' if status == 'playing' else '播放'), dom
    previous, current = before['player'], after['player']
    assert .2 < previous['position'] < 7.5 and previous['duration'] == 8
    assert previous['timelineMode'] == 'full' and previous['playId']
    assert previous['endedMinibarReceipt'] is None and before['dom']['endedStepKey'] is None
    if case != 'random-control':
        assert current['endedMinibarReceipt'] is None and after['dom']['endedStepKey'] is None
    assert [action['action']['paused'] for action in after['actions']] == [True, False]
    assert after['actions'] == [
        {'epoch': previous['epoch'], 'action': {
            'kind': 'pause_for', 'play_id': previous['playId'],
            'expected_queue_revision': previous['queueRevision'], 'paused': paused,
        }}
        for paused in (True, False)
    ], after['actions']
    assert [reply['state'] for reply in after['replies']] == ['paused', 'playing']
    assert all(reply['playId'] == previous['playId'] and reply['entryId'] == before['current']
               for reply in after['replies'])

    prefix, trace = before['trace'], after['trace']
    assert trace[:len(prefix)] == prefix and complete['trace'] == trace, trace
    assert [row for row in trace if row['kind'] == 'verified_assignment'] == [
        {'kind': 'verified_assignment', 'group': group},
    ], trace
    assert any(row['kind'] == 'count' and row['path'] == '/api/pl/count' for row in prefix), prefix
    assert all(row.get('path') != '/api/v3/song/detail' for row in trace), trace
    authorization = [row for row in trace if row['kind'] == 'authorization']
    expected_ids = ['1', '2', '3'] if skip and not publication else ['1', '2']
    expected_authorization = [
        {'kind': 'authorization', 'path': WIRE, 'id': item} for item in expected_ids
    ]
    if publication:
        for row, code in zip(expected_authorization, (200, 403), strict=True):
            row['serviceCode'] = code
    assert authorization == expected_authorization, trace
    assert authorization[:2] == [row for row in prefix if row['kind'] == 'authorization'], prefix
    loads = [row for row in trace if row['kind'] == 'native_load']
    assert [(row['id'], row['duration']) for row in loads] == (
        [('1', 8), ('3', target_duration)] if skip else [('1', 8)]
    ), trace
    old = loads[0]['entry']
    events = [row for row in trace if row['kind'] == 'native_eof']
    assert events == [{'kind': 'native_eof', 'entry': old, 'error': 0}], trace
    assert events[0] not in prefix and trace.index(loads[0]) < trace.index(events[0]), trace
    assert trace.index(authorization[1]) < trace.index(events[0]), trace
    assert [row for row in trace if row['kind'] == 'native_output'] == [
        {'kind': 'native_output', 'entry': row['entry'], 'output': 'null'} for row in loads
    ], trace

    def old_checkpoint(value):
        assert value['entry_id'] == before['current'] and value['state'] == 'ended'
        assert value['seconds'] == 8 and value['paused'] is False, value

    stops = [row for row in trace if row['kind'] == 'native_stop']
    if skip:
        assert .2 < current['position'] < target_duration and current['duration'] == target_duration, current
        assert current['playId'] and current['playId'] != previous['playId']
        assert current['queueRevision'] != previous['queueRevision'] and current['timelineMode'] == 'full'
        assert loads[1]['entry'] != old and trace.index(events[0]) < trace.index(loads[1]), trace
        if publication:
            assert (loads[1]['source'], loads[1]['codec'], loads[1]['verifiedFixtureBytes']) == (
                'held_flac', 'flac', True,
            ), trace
            assert after['sourceQuality'] == after['renderedSourceQuality'] == 'lossless'
            assert 0 < after['heldClockStart'] < current['position'] - .1, after
        else:
            assert trace.index(events[0]) < trace.index(authorization[2]) < trace.index(loads[1]), trace
        assert stops == [{'kind': 'native_stop', 'entry': old}], trace
        old_checkpoint(loads[1]['positionBeforeLoad'])
    else:
        assert current['position'] == after['dom']['position'] == 8, after
        assert after['dom']['elapsed'] == '00:08', after['dom']
        assert current['queueRevision'] == previous['queueRevision']
        old_checkpoint(after['durablePosition'])
        old_checkpoint(complete['durablePosition'])
        if case in ('control', 'random-control'):
            assert current['duration'] == 8 and not stops, trace
            action = {'direction': 'next', 'expected_queue_revision': current['queueRevision']}
            if case == 'control':
                assert current['playId'] == previous['playId']
                action.update(kind='step_mini_bar', play_id=current['playId'])
            else:
                assert current['playId'] is None
                receipt = current['endedMinibarReceipt']
                assert isinstance(receipt, str) and re.fullmatch('[a-f0-9]{32}', receipt)
                assert json.loads(after['dom']['endedStepKey']) == [
                    current['epoch'], receipt, current['entryId'], current['queueRevision'],
                ]
                action.update(kind='step_ended_mini_bar', receipt=receipt, entry_id=current['entryId'])
            retained = stages[2]
            assert retained['actual'] and retained['online'] and retained['group'] == group
            assert retained['case'] == case and retained['player'] == current
            assert retained['dom'] == after['dom'] and retained['dom']['nextEnabled']
            assert retained['dom']['nextLabel'] == '下一首'
            assert retained['current'] == after['current'] and retained['ids'] == after['ids']
            assert retained['actions'] == after['actions'] + [
                {'epoch': current['epoch'], 'action': action},
            ], retained['actions']
            assert retained['replies'] == after['replies'] + [current], retained['replies']
            assert retained['trace'] == trace, retained['trace']
            assert retained['durablePosition'] == after['durablePosition']
            old_checkpoint(retained['durablePosition'])
            if case == 'random-control':
                switched, clicked = stages[3:]
                new = switched['player']
                assert new['queueRevision'] != current['queueRevision']
                assert new == {**current, 'mode': 'sequential',
                               'queueRevision': new['queueRevision']}, new
                assert switched['dom'] == {
                    **retained['dom'], 'mode': 'sequential', 'modeLabel': '顺序播放',
                    'endedStepKey': switched['dom']['endedStepKey'],
                }, switched['dom']
                assert switched['dom']['nextEnabled'] and json.loads(switched['dom']['endedStepKey']) == [
                    new['epoch'], receipt, new['entryId'], new['queueRevision'],
                ]
                assert switched['actions'] == retained['actions'] + [{
                    'epoch': new['epoch'], 'action': {
                        'kind': 'mode_for', 'mode': 'sequential',
                        'expected_queue_revision': current['queueRevision'], 'fm': False,
                    },
                }], switched['actions']
                assert switched['replies'] == retained['replies'] + [new], switched['replies']
                assert clicked['player'] == new and clicked['dom'] == switched['dom']
                assert clicked['actions'] == switched['actions'] + [{
                    'epoch': new['epoch'], 'action': {
                        **action, 'expected_queue_revision': new['queueRevision'],
                    },
                }], clicked['actions']
                assert clicked['replies'] == switched['replies'] + [new], clicked['replies']
                for row in (switched, clicked):
                    assert row['actual'] and row['online'] and row['group'] == group and row['case'] == case
                    assert row['trace'] == trace and row['durablePosition'] == after['durablePosition']
                    assert row['ids'] == after['ids'] and row['current'] == after['current']
                    old_checkpoint(row['durablePosition'])
        elif publication:
            assert current['playId'] == previous['playId'] and current['duration'] == 8
            assert current['timelineMode'] == 'full' and not stops, trace
        else:
            assert current['playId'] is None and current['duration'] is None
            assert stops == [{'kind': 'native_stop', 'entry': old}]
            assert trace.index(stops[0]) > trace.index(events[0]), trace

    locale = None
    if not windows:
        locale = read(directory / 'locale.json')
        assert locale['numeric'] == 'C', locale
        assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
                   for key in ('messages', 'characters')), locale
    images = sorted(directory.glob('ux_*.png'))
    assert [image.stem for image in images] == (names if capture else []), images
    for name, stage in zip(names, stages, strict=True):
        sources = ['.sidebar-caption', '.playerbar']
        if skip and name == 'ux_02_after':
            sources.append('.ordinary-skip-layer .ordinary-skip-notice')
        assert [sample['source'] for sample in stage['paintSamples']] == sources, stage
        if capture:
            paint = read(directory / ('paint-' + name + '.json'))
            assert len(paint['samples']) == len(sources) and all(
                sample['ok'] for sample in paint['samples']
            ), paint
    transport = ['3', '1', '2'] if mode == 'random' else display
    final_mode, final_transport = mode, transport
    if case == 'random-control':
        final_mode, final_transport = 'sequential', display
        for row in stages[:3]:
            assert row['durableQueue'] == {
                'mode': mode, 'order': [entries[item] for item in transport],
                'current': entries[target],
            }, row['durableQueue']
        for row in (*stages[3:], complete):
            assert row['durableQueue'] == {
                'mode': final_mode, 'order': [entries[item] for item in final_transport],
                'current': entries[target],
            }, row['durableQueue']
    database = (directory.parent / 'state' / 'core.sqlite3').resolve(strict=True)
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as connection:
        persisted = connection.execute(
            "SELECT value FROM state WHERE owner='2' AND kind='playback' AND key='queue'"
        ).fetchone()
        saved = json.loads(persisted[0])
        download = check_download(directory, connection, case, before, after, complete, saved) if publication else None
    assert saved['order'] == [entries[item] for item in final_transport], saved['order']
    assert saved['current'] == entries[target] and saved['mode'] == final_mode
    result = {'case': case, 'group': group, 'target': target, 'state': state,
              'displayOrder': display, 'transportOrder': transport,
              'finalMode': final_mode, 'finalTransportOrder': final_transport,
              'stages': stages, 'notices': notices, 'locale': locale, 'screenshots': len(images)}
    if download is not None:
        result['downloadFallback'] = download
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path, help='exact staged Windows media runtime')
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--case', choices=CASES, action='append',
                        help='run only these named cases; default: all cases')
    args = parser.parse_args()
    windows = sys.platform == 'win32'
    if not windows and sys.platform != 'linux':
        parser.error('this proof runs on Linux and Windows')
    if windows and (not args.runtime or args.screenshots):
        parser.error('Windows requires --runtime and supports no GTK screenshot capture')
    if not windows and args.runtime:
        parser.error('--runtime identifies the Windows native media stage only')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    binary = args.binary.resolve(strict=True)
    base = {key: value for key, value in os.environ.items()
            if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))
            and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    windows_media = None
    if windows:
        stage, windows_media = windows_runtime(binary, args.runtime)
        base['MPV_DLL_DIR_CUMULUS'] = str(stage)
        profiles = ('APPDATA', 'LOCALAPPDATA', 'TEMP')
    else:
        base.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
        profiles = ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR')
    if args.frontend_dist:
        base['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    cases = {}
    for case in dict.fromkeys(args.case or CASES):
        work = root / case
        work.mkdir()
        run = work / 'run'
        output = run / 'first'
        env = {**base, 'CUMULUS_PROOF_DIR': str(run), 'CUMULUS_ORDINARY_EOF_CASE': case}
        for key in profiles:
            location = work / key
            location.mkdir(mode=0o700)
            env[key] = str(location)
        if windows:
            env['TMP'] = env['TEMP']
        host = work / 'host.log'
        park = windows and CASES[case][2] == 'playing'
        with (parked_windows_cursor(WindowsCursor()) if park else nullcontext()) as cursor:
            with host.open('x', encoding='utf-8') as log:
                launcher = launch_webview if windows else launch
                status = launcher(binary, env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), read(failed) if failed.exists() else None
        assert status == 0 and 'panicked at' not in host.read_text(encoding='utf-8'), host
        cases[case] = check(output, case, args.screenshots, windows)
        if cursor is not None:
            cases[case]['parkedWindowsCursor'] = list(cursor)
    report = {'actualWebView': True, 'decoder': 'libmpv', 'audioOutput': 'null',
              'webViewRuntime': 'WebView2' if windows else 'WebKitGTK',
              'windowsMediaRuntime': windows_media, 'physicalAudioOutputVerified': False,
              'transport': 'sealed_synthetic_verified_capability', 'cases': cases}
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n',
                                      encoding='utf-8')
    print(json.dumps({case: outcome['state'] for case, outcome in cases.items()}))


if __name__ == '__main__':
    main()
