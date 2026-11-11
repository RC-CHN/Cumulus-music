"""Exercise desktop history song downloads and publish only synthetic MP3 bytes."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime
from voice_song_download_smoke import published_mp3


ROOT = Path(__file__).resolve().parents[2]
STAGES = (
    'original_anchor_picker', 'cancel_no_write', 'reopened_read_only',
    'published_ordered', 'direct_liked_singleton',
)
ROW_STAGES = (
    'row_blocked_group', 'row_picker', 'row_cancelled', 'row_reopened',
    'row_published', 'row_liked_singleton', 'row_completed_menu',
)


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def captures(output, screenshots, stages):
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stages'] == list(stages)
    names = [f'ux_{index:02d}_{stage}' for index, stage in enumerate(stages, 1)]
    assert [file.stem for file in sorted(output.glob('ux_*.json'))] == names
    rows = [read(output / f'{name}.json') for name in names]
    photos = sorted(output.glob('ux_*.png'))
    assert [photo.stem for photo in photos] == (names if screenshots else [])
    hashes = {}
    for photo in photos:
        content = photo.read_bytes()
        assert content.startswith(b'\x89PNG\r\n\x1a\n') and len(content) > 1000
        hashes[photo.name] = hashlib.sha256(content).hexdigest()
    return complete, rows, hashes


def verify_publications(root, group, single):
    assert [row['id'] for row in group['completed']] == ['2', '3']
    assert [row['id'] for row in single['completed']] == ['2', '3', '4']
    assert group['completed'] == single['completed'][:2]
    audio = (ROOT / 'core/tests/fixtures/tag_audio/seed.mp3').read_bytes()
    destination = root / 'run/media'
    paths = set()
    for task in single['completed']:
        path = published_mp3(task['path'], destination)
        assert task['source'] == 'song' and task['quality'] == 'standard' and not task['encrypted']
        assert task['bytes'] == len(audio) and path.read_bytes() == audio
        paths.add(path)
    assert len(paths) == 3


def verify_native_downloads(group, single, members, *, row_mode=False):
    expected_add = {'kind': 'guarded_add', 'path': '/api/v1/playlist/manipulate/tracks',
                    'args': {'pid': '81', 'op': 'add',
                             'trackIds': json.dumps(members, separators=(',', ':'))}}
    allowed = {'liked', 'destinations', 'detail', 'guarded_add', 'authorize', 'transfer'}
    if row_mode:
        allowed.add('privileges')
    for report, ids in ((group, ['2', '3']), (single, ['2', '3', '4'])):
        calls = report['calls']
        assert all(call['kind'] in allowed for call in calls)
        assert report['members'] == members and report['taskCount'] == len(ids)
        assert [call for call in calls if call['kind'] == 'guarded_add'] == [expected_add]
        offset = calls.index(expected_add)
        prior = [call for call in calls[:offset] if call['kind'] == 'detail']
        assert prior and all(call['pid'] == '81' and call['trackIds'] == [] for call in prior)
        after = [call for call in calls[offset + 1:] if call['kind'] == 'detail']
        assert after and all(call['pid'] == '81' and [item['id'] for item in call['trackIds']] == members
                             for call in after)
        authorized = [call for call in calls if call['kind'] == 'authorize']
        assert all(calls.index(call) > calls.index(after[0]) for call in authorized)
        assert sorted(call['args']['id'] for call in authorized) == ids
        assert sorted(call['id'] for call in calls if call['kind'] == 'transfer') == ids
    for report, ids in ((group, ['2', '3']), (single, ['4'])):
        order = report['batchOrder']
        assert [task['song'] for task in order] == ids
        assert all(task['status'] == 'complete' for task in order)
        assert [task['dispatch']['index'] for task in order] == list(range(len(ids)))
        assert len({task['dispatch']['sequence'] for task in order}) == 1
    assert single['batchOrder'][0]['dispatch']['sequence'] > group['batchOrder'][0]['dispatch']['sequence']


def finish_report(output, result):
    if sys.platform.startswith('linux'):
        locale = read(output / 'locale.json')
        assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
        result.update(network='loopback-only', locale=locale)
    return result


def audit_row(root, output, screenshots):
    complete, rows, hashes = captures(output, screenshots, ROW_STAGES)
    assert complete['row']
    blocked, first, cancelled, reopened, group, single, finished = rows
    group_keys = ['play', 'next', 'collect', 'download']
    for cases, clicked, selection, gesture, keys in (
        ((blocked,), '6', ['1', '5'], 'ctrl-context', group_keys),
        ((first, cancelled, reopened, group), '5', ['3', '1', '2'], 'ctrl-context', group_keys),
        ((single,), '4', ['4'], 'button',
         ['play', 'next', 'comments', 'collect', 'download', 'copy', 'remove']),
        ((finished,), '2', ['2'], 'keyboard', ['play', 'next', 'comments', 'collect', 'copy', 'remove']),
    ):
        for row in cases:
            assert (row['clicked'], row['selectedIds'], row['gesture'], row['menuKeys']) == (
                clicked, selection, gesture, keys,
            )
            assert row['stayedInHistory'] and not row['active'] and not row['quality']
    assert blocked['downloadDisabled'] and blocked['copyrightHint'] == '暂无版权'
    assert blocked['title'] is None
    for row in (first, reopened):
        assert row['title'] == '下载到歌单' and row['destinations'] == ['81']
    for row in (cancelled, group, single):
        assert row['title'] is None and row['returnFocus']
    assert cancelled['feedback'] is None
    assert all(not row['completed'] for row in (blocked, first, cancelled, reopened))
    assert group['feedback'] == '已添加 2 首到下载列表，跳过 1 首'
    assert single['feedback'] == '已添加 1 首到下载列表' and single['chooserMounted'] is False
    assert finished['downloadVisible'] is False and finished['completed'] == single['completed']

    reports = [row['historySongDownload'] for row in [*rows, complete]]
    assert all(row['syntheticHistoryWrites'] == 0 for row in [*rows, complete])
    first_privileges = {'kind': 'privileges', 'ids': ['1', '2', '3', '4', '5', '6']}
    for report in reports:
        rights = [call for call in report['calls'] if call['kind'] == 'privileges']
        assert rights and rights[0] == first_privileges
    for report in reports[:4]:
        assert not report['members'] and not report['batchOrder'] and not report['taskCount']
        assert all(call['kind'] in ('privileges', 'liked', 'destinations') for call in report['calls'])
    assert all(call['kind'] != 'destinations' for call in reports[0]['calls'])
    assert {'kind': 'liked', 'ids': ['2', '3', '4']} in reports[1]['calls']
    count_reads = lambda report: sum(call['kind'] == 'destinations' for call in report['calls'])
    assert count_reads(reports[1]) > 0 and reports[2] == reports[1]
    assert count_reads(reports[3]) > count_reads(reports[2])
    verify_native_downloads(reports[4], reports[5], ['3', '2'], row_mode=True)
    assert count_reads(reports[4]) == count_reads(reports[5])
    assert reports[6] == reports[7] == reports[5]
    verify_publications(root, group, single)
    return finish_report(output, dict(
        ok=True, actualWebView='WebView2' if sys.platform == 'win32' else 'WebKitGTK',
        network='synthetic-transports-only', stages=len(rows), screenshots=hashes,
        groupBlockedDownload=True, clickedUnlikedAnchor='5', cancelledWithoutWrites=True,
        playlistAdd=['3', '2'], orderedDownload=['2', '3'], likedDirectDownload=['4'],
        completedDownloadHidden='2', syntheticPlaylistWrites=1, realAccountWrites=0,
        realMediaRequests=0,
    ))


def audit(root, output, screenshots):
    complete, rows, hashes = captures(output, screenshots, STAGES)
    first, cancelled, reopened, group, single = rows

    for row in rows[:4]:
        assert row['selected'] == ['01', '02', '03']
    for row in (first, reopened):
        assert row['title'] == '下载到歌单' and row['destinations'] == ['81']
    for row in (cancelled, group, single):
        assert row['title'] is None and row['returnFocus']
    assert single['selected'] == ['04']
    assert not any(row['quality'] or row['active'] for row in rows)
    assert cancelled['feedback'] is None
    assert group['feedback'] == '已添加 2 首到下载列表，跳过 1 首'
    assert single['feedback'] == '已添加 1 首到下载列表'
    assert all(not row['completed'] for row in rows[:3])

    reports = [row['historySongDownload'] for row in [*rows, complete]]
    assert all(row['syntheticHistoryWrites'] == 0 for row in [*rows, complete])
    before = reports[:3]
    assert all(not report['members'] and not report['batchOrder'] and not report['taskCount']
               for report in before)
    assert all(call['kind'] in ('liked', 'destinations') for report in before for call in report['calls'])
    assert any(call == {'kind': 'liked', 'ids': ['2', '3', '4']} for call in before[0]['calls'])
    count_reads = lambda report: sum(call['kind'] == 'destinations' for call in report['calls'])
    assert count_reads(before[0]) > 0 and count_reads(before[0]) == count_reads(before[1])
    assert count_reads(before[2]) > count_reads(before[1])

    group_report, single_report = reports[3:5]
    verify_native_downloads(group_report, single_report, ['1', '2', '3'])
    assert reports[5] == single_report
    assert count_reads(group_report) == count_reads(single_report)
    verify_publications(root, group, single)
    result = dict(ok=True, actualWebView='WebView2' if sys.platform == 'win32' else 'WebKitGTK',
                  network='synthetic-transports-only', stages=len(rows), screenshots=hashes,
                  playlistAdd=['1', '2', '3'], orderedDownload=['2', '3'], likedDirectDownload=['4'],
                  syntheticPlaylistWrites=1, realAccountWrites=0, realMediaRequests=0)
    return finish_report(output, result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path, help='same-run staged Windows native media directory')
    parser.add_argument('--screenshots', action='store_true', help='capture GTK screenshots on Linux')
    parser.add_argument('--row', action='store_true', help='verify SONG browse row Download')
    args = parser.parse_args()
    windows = sys.platform == 'win32'
    if windows:
        if not args.runtime or args.screenshots:
            parser.error('Windows requires --runtime; GTK screenshots are Linux only')
    elif sys.platform.startswith('linux'):
        if args.runtime:
            parser.error('--runtime identifies the staged Windows native media directory')
        if (not os.environ.get('DISPLAY', '').startswith(':')
                or {name for _, name in socket.if_nameindex()} != {'lo'}
                or len(Path('/proc/net/route').read_text(encoding='ascii').splitlines()) != 1):
            parser.error('use X11 in a sealed Linux network namespace containing only loopback')
    else:
        parser.error('this WebView proof supports Linux and Windows')
    binary = args.binary.resolve(strict=True)
    runtime = None
    if windows:
        stage, runtime = windows_runtime(binary, args.runtime)
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_')) and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    script = ROOT / 'desktop/examples/history_song_download_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'),
               CUMULUS_HISTORY_PROOF='1', CUMULUS_HISTORY_SONG_DOWNLOAD_PROOF='1',
               CUMULUS_UX_SCRIPT=str(script))
    if args.row:
        row_script = root / 'row-ui.js'
        row_script.write_text('window.__historySongRowDownloadProof = true;\n' +
                              script.read_text(encoding='utf-8'), encoding='utf-8')
        env.update(CUMULUS_UX_SCRIPT=str(row_script), CUMULUS_HISTORY_SONG_ROW_DOWNLOAD_PROOF='1')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if windows:
        env['MPV_DLL_DIR_CUMULUS'] = str(stage)
        profile_dirs = ('APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP')
    else:
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
        profile_dirs = ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR')
    for key in profile_dirs:
        path = root / key
        path.mkdir(mode=0o700)
        env[key] = str(path)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, args.screenshots, acknowledge_receipts=True)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert status == 0, f'native probe exit {status}'
    report = audit_row(root, output, args.screenshots) if args.row else audit(root, output, args.screenshots)
    if runtime:
        report['runtime'] = runtime
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
