"""Exercise desktop history collection with synthetic account and playlist writes."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime


ROOT = Path(__file__).resolve().parents[2]
STAGES = (
    'filtered_header_picker', 'header_cancel', 'header_confirmed',
    'batch_picker', 'batch_confirmed', 'batch_empty_eligibility',
)
ROW_STAGES = (
    'row_single_picker', 'row_single_cancel', 'row_single_denied',
    'row_modifier_group_picker', 'row_modifier_group_confirmed',
    'row_fallback_picker', 'row_fallback_confirmed',
)
ADD = '/api/v1/playlist/manipulate/tracks'


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


def guarded_writes(reports, expected, counts):
    guarded = [
        {'kind': 'guarded_add', 'path': ADD, 'args': {
            'op': 'add', 'pid': pid, 'trackIds': json.dumps(ids, separators=(',', ':')),
        }} for pid, ids in expected.items()
    ]
    for report, count in zip(reports, counts, strict=True):
        calls = report['calls']
        assert all(call['kind'] in ('destinations', 'detail', 'guarded_add') for call in calls)
        assert [call for call in calls if call['kind'] == 'guarded_add'] == guarded[:count]
        assert report['members'] == dict(list(expected.items())[:count])
        for write in guarded[:count]:
            offset = calls.index(write)
            pid = write['args']['pid']
            before = [call['trackIds'] for call in calls[:offset]
                      if call['kind'] == 'detail' and call['pid'] == pid]
            after = [call['trackIds'] for call in calls[offset + 1:]
                     if call['kind'] == 'detail' and call['pid'] == pid]
            assert before and all(ids == [] for ids in before)
            assert after and all([row['id'] for row in ids] == expected[pid] for ids in after)
    return guarded


def audit_row(output, screenshots, runtime):
    complete, rows, hashes = captures(output, screenshots, ROW_STAGES)
    assert complete['row']
    singleton, cancelled, denied, group, first, fallback, second = rows
    single_menu = ['play', 'next', 'comments', 'collect', 'download', 'copy', 'remove']
    for case, clicked, selection, gesture, keys in (
        ((singleton, cancelled), '2', ['2'], 'keyboard', single_menu),
        ((denied,), '3', ['3'], 'button', single_menu),
        ((group, first), '6', ['4', '2', '1'], 'ctrl-context', ['play', 'next', 'collect', 'download']),
        ((fallback, second), '2', ['5', '2', '3'], 'button',
         ['play', 'next', 'collect', 'download', 'remove']),
    ):
        for row in case:
            assert (row['clicked'], row['selectedIds'], row['gesture'], row['menuKeys']) == (
                clicked, selection, gesture, keys,
            )
    for row in (singleton, cancelled):
        assert row['query'] == '合成筛选目标' and row['visibleRows'] == ['合成筛选目标 · 02']
    for row, summary in (
        (singleton, '所选播放记录共 1 项，将收藏 1 首歌曲。'),
        (group, '所选播放记录共 3 项，将收藏 2 首歌曲。1 项无法收藏，已略过。'),
        (fallback, '所选播放记录共 3 项，将收藏 1 首歌曲。2 项无法收藏，已略过。'),
    ):
        assert row['title'] == '收藏到歌单' and row['destinations'] == ['81', '82']
        assert row['summary'] == summary
    for row in (cancelled, denied, first, second):
        assert row['title'] is None and row['returnFocus']
    assert cancelled['feedback'] is None
    assert denied['mountedPicker'] is False and denied['feedbackRole'] == 'status'
    assert denied['feedback'] == '所选播放记录没有可收藏的歌曲。'
    assert first['feedback'] == second['feedback'] == '已收藏到歌单。'
    assert (first['playlist'], second['playlist']) == ('81', '82')

    reports = [row['historySongCollectCalls'] for row in [*rows, complete]]
    assert all(row['syntheticHistoryWrites'] == 0 for row in [*rows, complete])
    expected = {'81': ['4', '1'], '82': ['5']}
    guarded = guarded_writes(reports, expected, (0, 0, 0, 0, 1, 1, 2, 2))
    # Native HistoryApi joins the initial remote snapshot with ordinary song
    # privileges. Menu preparation reuses those exact verified rows; it does
    # not issue a new privilege read in UI order or include the unselected anchor.
    assert all(report['rights'] == [['1', '2', '3', '4', '5', '6']] for report in reports)
    assert reports[0]['calls'] == reports[1]['calls'] == reports[2]['calls']
    assert reports[0]['calls'] and all(call['kind'] == 'destinations' for call in reports[0]['calls'])
    assert reports[6] == reports[7]
    result = dict(ok=True, actualWebView='WebView2' if runtime else 'WebKitGTK',
                  stages=len(rows), screenshots=hashes, orderedPlaylistAdds=expected,
                  singletonCancelWrites=0, deniedPickerMounted=False, nativePlaylistReadbacks=2,
                  syntheticPlaylistWrites=len(guarded), realAccountWrites=0)
    if runtime:
        result.update(runtime=runtime, network='synthetic-transports-only')
    else:
        locale = read(output / 'locale.json')
        assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
        result.update(locale=locale, network='loopback-only')
    return result


def audit(output, screenshots):
    complete, rows, hashes = captures(output, screenshots, STAGES)
    header, cancelled, header_done, batch, batch_done, empty = rows

    for row in (header, cancelled, header_done):
        assert row['query'] == '合成筛选目标' and row['visibleRows'] == ['合成筛选目标 · 02']
    assert header['summary'] == '所选播放记录共 6 项，将收藏 4 首歌曲。2 项无法收藏，已略过。'
    assert batch['summary'] == '所选播放记录共 4 项，将收藏 2 首歌曲。2 项无法收藏，已略过。'
    for row in (header, batch):
        assert row['title'] == '收藏到歌单' and row['destinations'] == ['81', '82']
    for row in (batch, batch_done):
        assert row['selected'] == ['01', '02', '03', '04']
    for row in (cancelled, header_done, batch_done, empty):
        assert row['title'] is None and row['returnFocus']
    assert cancelled['feedback'] is None
    assert header_done['feedback'] == batch_done['feedback'] == '已收藏到歌单。'
    assert empty['selected'] == ['03'] and empty['feedbackRole'] == 'status'
    assert empty['feedback'] == '所选播放记录没有可收藏的歌曲。'

    reports = [row['historySongCollectCalls'] for row in [*rows, complete]]
    assert all(row['syntheticHistoryWrites'] == 0 for row in [*rows, complete])
    expected = {
        '81': ['5', '4', '2', '1'],  # Full header source, independent of the UI filter.
        '82': ['1', '4'],            # Batch source in native order, not click order.
    }
    guarded = guarded_writes(reports, expected, (0, 0, 1, 1, 2, 2, 2))
    assert reports[0] == reports[1] and reports[0]['calls']
    assert all(call['kind'] == 'destinations' for call in reports[0]['calls'])
    batch_open_reads = reports[3]['calls'][len(reports[2]['calls']):]
    assert batch_open_reads and all(call['kind'] == 'destinations' for call in batch_open_reads)
    assert reports[4] == reports[5] == reports[6]

    result = dict(ok=True, actualWebView=sys.platform, stages=len(rows), screenshots=hashes,
                  orderedPlaylistAdds=expected, syntheticPlaylistWrites=len(guarded), realAccountWrites=0)
    if sys.platform.startswith('linux'):
        locale = read(output / 'locale.json')
        assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
        result.update(locale=locale, network='loopback-only')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--row', action='store_true', help='verify browse song row collection')
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
    script = ROOT / 'desktop/examples/history_song_collect_ui_probe.js'
    env.update(LC_ALL='en_US.UTF-8', CUMULUS_PROOF_DIR=str(root / 'run'),
               CUMULUS_HISTORY_PROOF='1', CUMULUS_HISTORY_SONG_COLLECT_PROOF='1',
               CUMULUS_UX_SCRIPT=str(script))
    if linux:
        env['GDK_BACKEND'] = 'x11'
    runtime = None
    if args.row:
        env = {key: value for key, value in env.items()
               if not key.upper().startswith('WEBVIEW2_') and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
        row_script = root / 'row-ui.js'
        row_script.write_text('window.__historySongRowCollectProof = true;\n' +
                              script.read_text(encoding='utf-8'), encoding='utf-8')
        env.update(CUMULUS_UX_SCRIPT=str(row_script), CUMULUS_HISTORY_SONG_ROW_COLLECT_PROOF='1')
        if not linux:
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
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(binary, env, log, output, args.screenshots, acknowledge_receipts=True)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert status == 0, f'native probe exit {status}'
    report = audit_row(output, args.screenshots, runtime) if args.row else audit(output, args.screenshots)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
