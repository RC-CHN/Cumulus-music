"""Playlist resource menus and confirmation/download/share UX in a real WebView."""
import argparse
import json
import os
from pathlib import Path
import socket
import sys
from motion_smoke import launch
from playback_power_smoke import windows_runtime


SONG_COMMENT_STAGES = (
    'initial', 'exact_count', 'exact_thread', 'zero_plain', 'wrong_thread_plain',
    'transport_plain', 'private_type_hidden', 'left_while_held', 'reopened_count',
    'loaded_suffix', 'away_from_suffix', 'retained_suffix_count',
    'sorted_count', 'restored_order_count',
)
COUNT_PATH = '/api/resource/commentInfo/list'
THREAD_PATH = '/api/v1/resource/comments/R_SO_4_3'
PLAYLIST_DETAIL_PATH = '/api/v6/playlist/detail'
SONG_DETAILS_PATH = '/api/v3/song/detail'


def audit_song_comments(root, result, screenshots, runtime):
    first = root / 'first'
    names = [f'ux_{i:02d}_{stage}' for i, stage in enumerate(SONG_COMMENT_STAGES, 1)]
    assert result['ok'] and result['stages'] == list(SONG_COMMENT_STAGES) and result['focusRestored']
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    assert sorted(path.stem for path in first.glob('ux_*.json')) == names
    assert sorted(path.stem for path in first.glob('ux_*.png')) == (names if screenshots else [])
    rows = [json.loads((first / f'{name}.json').read_text(encoding='utf-8')) for name in names]
    (initial, exact, thread, zero, wrong, transport, hidden, left, reopened,
     suffix, away, retained, sorted_row, original) = rows
    assert initial['playlistId'] == '2' and initial['countCalls'] == 0
    assert initial['syntheticLibrary']['detailReads'] >= 1
    assert (exact['clicked'], exact['via'], exact['comment']) == ('3', 'keyboard', '查看评论(36)')
    assert (thread['clicked'], thread['resourceKind'], thread['resourceId']) == ('3', 'track', '3')
    for row, id, gesture in ((zero, '4', 'button'), (wrong, '5', 'context'), (transport, '6', 'keyboard')):
        assert (row['clicked'], row['via'], row['comment']) == (id, gesture, '查看评论')
        assert row['menuKeys'][:3] == ['play', 'next', 'comments']
    assert (hidden['clicked'], hidden['comment']) == ('7', None)
    assert hidden['menuKeys'][:2] == ['play', 'next'] and 'comments' not in hidden['menuKeys']
    assert left['clicked'] == '8' and left['oldMenuSuppressed'] and left['heldReturned']
    assert (reopened['clicked'], reopened['comment'], reopened['newAnchor']) == ('8', '查看评论(51)', True)
    assert (suffix['clicked'], suffix['comment'], suffix['ordinal']) == ('101', '查看评论(70)', '102')
    assert suffix['tableScroll']['after'] > suffix['tableScroll']['before']
    assert away['playlistId'] == '3'
    assert (retained['clicked'], retained['comment'], retained['ordinal'], retained['newAnchor']) == (
        '101', '查看评论(73)', '102', True)
    before_catalog = reopened['playlistSongCommentFixture']['catalogReads']
    for row, playlist_id, ids in (
        (suffix, '2', range(101, 106)), (away, '3', range(1, 41)),
        (retained, '2', range(1, 101)),
    ):
        current_catalog = row['playlistSongCommentFixture']['catalogReads']
        assert current_catalog == before_catalog + [
            dict(path=PLAYLIST_DETAIL_PATH, id=playlist_id, cached=False),
            dict(path=SONG_DETAILS_PATH, ids=list(map(str, ids)), cached=False),
        ], current_catalog[len(before_catalog):]
        before_catalog = current_catalog
    for row, sort, ordinal in ((sorted_row, 'ascending', '2'), (original, 'none', '10')):
        assert (row['clicked'], row['comment'], row['sort'], row['rowIndex']) == (
            '9', '查看评论(64)', sort, ordinal)
    calls = [dict(path=COUNT_PATH, args={'resourceType': 4, 'resourceIds': json.dumps([id])},
                  cached=False, thread=wire_thread, count=count, outcome=outcome)
             for id, wire_thread, count, outcome in (
                 ('3', 'R_SO_4_3', 36, 'count'), ('4', 'R_SO_4_4', 0, 'zero'),
                 ('5', 'R_SO_4_9', 99, 'wrong_thread'), ('6', None, None, 'transport'),
                 ('8', 'R_SO_4_8', 88, 'held'), ('8', 'R_SO_4_8', 51, 'reopened'),
                 ('101', 'R_SO_4_101', 70, 'suffix'), ('101', 'R_SO_4_101', 73, 'retained_back'),
                 ('9', 'R_SO_4_9', 64, 'sorted'), ('9', 'R_SO_4_9', 64, 'restored_order'),
             )]
    fixture = result['playlistSongCommentFixture']
    assert fixture['heldStarted'] and fixture['heldReleased'] and fixture['heldReturned']
    assert fixture['counts'] == calls
    for row, length in zip(rows, [0, 1, 1, 2, 3, 4, 4, 5, 6, 7, 7, 8, 9, 10], strict=True):
        assert row['playlistSongCommentFixture']['counts'] == calls[:length]
    thread_reads = fixture['threads']
    assert not exact['playlistSongCommentFixture']['threads']
    assert thread['playlistSongCommentFixture']['threads'] == thread_reads
    assert any(call['path'] == THREAD_PATH for call in thread_reads)
    assert all(call['path'] in (THREAD_PATH, THREAD_PATH.replace('/comments/', '/hotcomments/'))
               and not call['cached'] and call['args']['offset'] == 0 for call in thread_reads)
    for row in (*rows, result):
        library = row['syntheticLibrary']
        assert library['writes'] == 0 and not library['trackWrites']
        assert row['nativePlayer']['state'] == 'idle', row['nativePlayer']['state']
    audit = dict(ok=True, actualWebView='WebView2' if runtime else 'WebKitGTK', stages=len(rows),
                 screenshots=len(rows) if screenshots else 0, countCalls=calls,
                 exactThread=THREAD_PATH, staleCountSuppressed=True, hiddenSongCountReads=0,
                 sortedAndOriginalCounts=True, retainedBackSong='101', backReadHeadOnly=True,
                 initialSongCountReads=0, syntheticPlaylistWrites=0, realMusicRequests=0,
                 realAccountWrites=0)
    if runtime:
        audit['runtime'] = runtime
    else:
        locale = json.loads((first / 'locale.json').read_text(encoding='utf-8'))
        assert locale['numeric'] == 'C' and all(
            locale[key].casefold().replace('-', '') == 'en_us.utf8' for key in ('messages', 'characters'))
        audit['locale'] = locale
    (root / 'audit.json').write_text(json.dumps(audit, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(audit, ensure_ascii=False))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--song-comment-count', action='store_true')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    windows = sys.platform == 'win32'
    binary = args.binary.resolve(strict=True)
    runtime = None
    if args.song_comment_count:
        if windows:
            if not args.runtime or args.screenshots:
                parser.error('Windows requires --runtime; GTK capture is Linux only')
            runtime_path, runtime = windows_runtime(binary, args.runtime)
        elif sys.platform != 'linux' or args.runtime or not os.environ.get('DISPLAY', '').startswith(':') or (
                {name for _, name in socket.if_nameindex()} != {'lo'} or
                len(Path('/proc/net/route').read_text(encoding='ascii').splitlines()) != 1):
            parser.error('use X11 in a sealed Linux network namespace containing only loopback')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_LIBRARY_UI_PROOF='1')
    env['CUMULUS_PLAYLIST_SONG_COMMENT_PROOF' if args.song_comment_count else
        'CUMULUS_PLAYLIST_MENU_UI_PROOF'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if runtime:
        env['MPV_DLL_DIR_CUMULUS'] = str(runtime_path)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    source = Path(__file__).resolve().parents[2] / 'desktop/examples' / (
        'playlist_song_comment_ui_probe.js' if args.song_comment_count else 'playlist_menu_ui_probe.js')
    script = root / 'playlist-menu.js'
    script.write_text(source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        code = launch(binary, env, log, root / 'first', args.screenshots,
                      acknowledge_receipts=args.song_comment_count)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    result = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    if args.song_comment_count:
        audit_song_comments(root, result, args.screenshots, runtime)
        return
    assert result['ok'] and result['playlistMenuUi'] and result['stageCount'] == 52
    assert result['scrollStabilityVerified'] and result['focusedDuringLeave']
    assert result['leavingChecks'] and all(row['inert'] for row in result['leavingChecks'])
    assert result['clipboardVerified'] and result['mutationRecoveryVerified'] and result['fullDownloadVerified']
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    assert result['syntheticLibrary']['writes'] == 4
    assert result['syntheticLibrary']['removed'] == [4, 8, 61]
    assert len(list((root / 'first').glob('ux_*.json'))) == 52
    if args.screenshots:
        assert len(list((root / 'first').glob('ux_*.png'))) == 52
    playing = json.loads((root / 'first/ux_51_playing.json').read_text(encoding='utf-8'))['nativePlayer']
    paused = json.loads((root / 'first/ux_52_paused.json').read_text(encoding='utf-8'))['nativePlayer']
    assert playing['state'] == 'playing' and paused['state'] == 'paused'
    assert playing['queueLen'] == paused['queueLen'] == 40
    assert playing['playId'] == paused['playId'] and paused['position'] > 0
    result.update(actualWebView=True, physicalDisplayVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: result[k] for k in ['ok', 'stageCount', 'actualWebView', 'realAccountWrites']}))


if __name__ == '__main__':
    main()
