"""Verify Recently Played row menus through the sealed desktop WebView."""

import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import sys

from history_header_smoke import projected, read
from motion_smoke import launch
from playback_power_smoke import windows_runtime


ROOT = Path(__file__).resolve().parents[2]
SHARE_STAGES = (
    'share_song_chooser', 'share_song_cancelled', 'share_song_second_chooser',
    'share_song_late_denied', 'share_song_copy_chooser', 'share_song_copied',
    'share_voice_chooser', 'share_voice_qr', 'share_voice_closed',
)
STAGES = (
    'song_context', 'song_inserted', 'voice_inserted', 'voice_random_step',
    'empty_song_clicked', 'empty_voice_clicked',
    'play_blocked', 'play_rejected', 'play_song', 'play_resume_ready', 'play_resumed',
    'play_voice',
    'actions_song_menu', 'actions_song_comment', 'actions_song_copied',
    'actions_song_count_fallback', 'actions_voice_menu', 'actions_voice_comment',
    'actions_voice_copied', *SHARE_STAGES, 'actions_delete_confirm', 'actions_delete_cancelled',
    'actions_delete_confirmed',
)
SEED = ['voice:5', 'voice:6', 'voice:7']
SONGS = ['voice:5', 'voice:6', 'song:3', 'song:1', 'voice:7']
VOICES = ['voice:5', 'voice:6', 'voice:4', 'voice:2', 'song:3', 'song:1', 'voice:7']
PLAY_SEED = ['voice:11', 'voice:12', 'voice:13']
PLAY_SONGS = ['voice:11', 'voice:12', 'song:3', 'song:1', 'voice:13']
RESUMED_SONGS = ['voice:11', 'voice:12', 'song:3', 'song:9', 'song:1', 'voice:13']
PLAY_VOICES = ['voice:11', 'voice:12', 'song:3', 'song:9', 'song:1', 'voice:4', 'voice:2', 'voice:13']
SINGLE_ACTIONS = ['play', 'next', 'comments', 'collect', 'download', 'share', 'copy', 'remove']
VOICE_ACTIONS = ['play', 'next', 'comments', 'download', 'share', 'copy', 'remove']
DELETE_CALL = dict(path='/api/play-record/batch/delete', cached=False, args={
    'type': 'SONG', 'resourceIds': '["10","7"]', 'lastResourceId': '300',
    'lastPlayTime': 1793179700000,
})


def kept_queue_entries(before, after):
    tracks = after['nativeQueue']['tracks']
    entries = dict(zip(projected(tracks), (row['entryId'] for row in tracks), strict=True))
    previous = before['nativeQueue']['tracks']
    assert all(entries[identity] == row['entryId']
               for identity, row in zip(projected(previous), previous, strict=True))


def native_play(row, prior, *, song, voice=None, resume=False):
    player, before = row['nativePlayer'], prior['nativePlayer']
    assert player['epoch'] == before['epoch'] and player['mode'] == before['mode'] == 'sequential'
    assert (player['state'], player['track']['id'], player['track'].get('voiceId')) == (
        'playing', song, voice,
    )
    assert row['focusRestored']
    clock = row['clock']
    assert clock['last'] - clock['first'] > .2 and player['position'] >= clock['last']
    if resume:
        assert (player['playId'], player['entryId']) == (before['playId'], before['entryId'])
        assert clock['first'] >= before['position'] - .05
    else:
        assert player['playId'] != before['playId'] and player['entryId'] != before['entryId']
    assert row['nativeQueue']['revision'] != prior['nativeQueue']['revision']
    kept_queue_entries(prior, row)


def audit_row_actions(actions, incumbent, complete):
    (song_menu, song_comment, song_copy, fallback, voice_menu, voice_comment, voice_copy,
     confirmation, cancelled, removed) = actions
    baseline = song_menu['nativePlayer']
    assert baseline['state'] == 'paused'
    assert all(baseline[key] == incumbent['nativePlayer'][key] for key in (
        'epoch', 'playId', 'entryId', 'track',
    ))
    for row in actions:
        assert row['nativeQueue'] == incumbent['nativeQueue']
        assert all(row['nativePlayer'][key] == baseline[key] for key in (
            'epoch', 'playId', 'entryId', 'state', 'position', 'track',
        ))

    for row, resource_type, id in (
        (song_menu, 4, '10'), (fallback, 4, '11'), (voice_menu, 1, '10'), (confirmation, 4, '7'),
    ):
        expected = dict(path='/api/resource/commentInfo/list', cached=False, args={
            'resourceType': resource_type, 'resourceIds': json.dumps([id]),
        })
        assert expected in row['historyRowMenuCalls']
    for row, opened, keys, kind, label, body, thread in (
        (song_comment, song_menu, SINGLE_ACTIONS, 'track', '查看评论(36)', '合成历史歌曲评论', 'R_SO_4_10'),
        (voice_comment, voice_menu, VOICE_ACTIONS, 'voice', '查看评论(47)', '合成历史声音评论', 'A_DJ_1_10'),
    ):
        assert opened['keys'] == keys
        assert (opened['clicked'], opened['label']) == ('10', label)
        assert (row['commentKind'], row['commentId'], row['label'], row['body']) == (
            kind, '10', label, body,
        )
        assert any(call['path'] == '/api/v1/resource/comments/' + thread and not call['cached']
                   and call['args']['offset'] == 0 for call in row['historyRowMenuCalls'])
    assert fallback['clicked'] == '11' and fallback['label'] == '查看评论'
    assert voice_menu['selected'] == ['9', '10']
    for row, category, selected in (
        (song_copy, 'song', ['10']), (voice_copy, 'dj', ['9', '10']),
    ):
        assert row['clicked'] == '10' and row['selected'] == selected
        assert row['focusRestored'] and row['nativeClipboard'] and row['notice'] == '复制成功'
        assert row['clipboard'] == f'https://music.163.com/{category}?id=10'

    assert all(row['clicked'] == '7' and row['selected'] == ['10', '7']
               for row in (confirmation, cancelled, removed))
    assert confirmation['keys'] == ['play', 'next', 'collect', 'download', 'remove']
    assert (confirmation['title'], confirmation['actions'], confirmation['focus']) == (
        '删除 2 条播放记录？', ['取消', '确认'], '取消',
    )
    assert '音乐文件、收藏和歌单内容会保留' in confirmation['description']
    assert cancelled['focusRestored'] and cancelled['revision'] == confirmation['beforeRevision']
    assert removed['fallbackFocus'] == '歌曲' and removed['remaining'] == 298
    assert removed['revision'] != cancelled['revision']
    for row, expected in ((confirmation, []), (cancelled, []), (removed, [DELETE_CALL]),
                          (complete, [DELETE_CALL])):
        assert [call for call in row['historyRowMenuCalls'] if '/delete' in call['path']] == expected


def share_qr_payload(matrix):
    """Read an undamaged Version 2/3 byte-mode QR from the rendered SVG modules."""
    size = len(matrix)
    assert size in (25, 29) and all(len(row) == size and set(row) <= {'0', '1'} for row in matrix)
    module = lambda x, y: int(matrix[y][x])
    # Both format copies must carry the same valid BCH-encoded level and mask.
    corner = ([(8, y) for y in range(6)] + [(8, 7), (8, 8), (7, 8)]
              + [(x, 8) for x in reversed(range(6))])
    opposite = [(size - 1 - i, 8) for i in range(8)] + [(8, size - 15 + i) for i in range(8, 15)]
    bits_at = lambda positions: sum(module(x, y) << i for i, (x, y) in enumerate(positions))
    fmt = bits_at(corner)
    assert fmt == bits_at(opposite)
    data = (fmt ^ 0x5412) >> 10
    rem = data
    for _ in range(10):
        rem = (rem << 1) ^ ((rem >> 9) * 0x537)
    assert fmt == (((data << 10) | rem) ^ 0x5412)
    mask = data & 7
    level = data >> 3
    # ISO QR codeword layouts for the levels that can contain this exact URL.
    layout = {25: {1: (44, 1, 34)}, 29: {0: (70, 1, 44), 1: (70, 1, 55), 3: (70, 2, 17)}}
    assert level in layout[size]
    total, blocks, width = layout[size][level]

    def function(x, y):
        return (x == 6 or y == 6 or (x < 9 and (y < 9 or y >= size - 8))
                or (x >= size - 8 and y < 9)
                or (size - 9 <= x <= size - 5 and size - 9 <= y <= size - 5))

    def masked(x, y):
        product = x * y
        return ((x + y) % 2, y % 2, x % 3, (x + y) % 3, (x // 3 + y // 2) % 2,
                product % 2 + product % 3, (product % 2 + product % 3) % 2,
                ((x + y) % 2 + product % 3) % 2)[mask] == 0

    bits = []
    right, upward = size - 1, True
    while right > 0:
        if right == 6:
            right = 5
        for y in (reversed(range(size)) if upward else range(size)):
            for x in (right, right - 1):
                if not function(x, y):
                    bits.append(str(module(x, y) ^ masked(x, y)))
        right -= 2
        upward = not upward
    assert len(bits) == total * 8 + 7 and bits[-7:] == ['0'] * 7
    codewords = [int(''.join(bits[i:i + 8]), 2) for i in range(0, total * 8, 8)]
    payload = ''.join(f'{value:08b}' for block in range(blocks)
                      for value in codewords[block:width * blocks:blocks])
    assert payload[:4] == '0100'
    length = int(payload[4:12], 2)
    end = 12 + length * 8
    assert 0 < length and end + 4 <= len(payload) and payload[end:end + 4] == '0000'
    return bytes(int(payload[i:i + 8], 2) for i in range(12, end, 8)).decode('ascii')


def audit_row_share(share, by_name, complete):
    chooser, cancelled, second, denied, copy, copied, voice, qr, closed = share
    for row, ids in ((chooser, ['10']), (second, ['10']), (copy, ['10']), (voice, ['9', '10'])):
        assert (row['clicked'], row['selected'], row['title'], row['choices'], row['focus']) == (
            '10', ids, '分享到', ['微信好友', '复制链接'], '关闭分享',
        )
    for row, ids in ((cancelled, ['10']), (denied, ['10']), (copied, ['10']), (closed, ['9', '10'])):
        assert (row['clicked'], row['selected'], row['focusRestored']) == ('10', ids, True)
    assert cancelled['closeGesture'] == 'dialog_cancel' and closed['closeGesture'] == 'close_button'
    assert denied['error'] == '当前资源暂不支持分享。' and denied['nativeClipboardUnchanged']
    assert copied['clipboardMarker'] == 'Cumulus synthetic history share clipboard marker'
    assert copied['nativeClipboardMarker'] is True and copied['nativeClipboard'] is True
    assert (copied['clipboard'], copied['notice']) == ('https://music.163.com/song?id=10', '复制成功')
    assert (qr['clicked'], qr['selected'], qr['title'], qr['hint'], qr['focus'], qr['qrWidth']) == (
        '10', ['9', '10'], '分享到微信', '打开微信, 扫一扫', '关闭微信分享二维码', 180,
    )
    assert share_qr_payload(qr['qrModules']) == 'https://music.163.com/dj?id=10'

    exact = [dict(path='/api/song/enhance/privilege', args={'ids': '["10"]'}, cached=False,
                  sharePrivilege=sp) for sp in (1, 1, 0, 1, 1)]
    reads = dict(zip(SHARE_STAGES, (1, 1, 2, 3, 4, 5, 5, 5, 5), strict=True))
    for name, row in by_name.items():
        count = reads.get(name, len(exact) if name.startswith('actions_delete_') else 0)
        assert row['historyRowShareCalls'] == exact[:count], (name, row['historyRowShareCalls'])
    assert complete['historyRowShareCalls'] == exact


def audit(output, screenshots, runtime=None):
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stages'] == list(STAGES)
    names = [f'ux_{index:02d}_{stage}' for index, stage in enumerate(STAGES, 1)]
    assert [path.stem for path in sorted(output.glob('ux_*.json'))] == names
    rows = [read(output / f'{name}.json') for name in names]
    by_name = dict(zip(STAGES, rows, strict=True))
    assert all(row['syntheticHistoryWrites'] == 0 for row in rows[:-1])
    assert rows[-1]['syntheticHistoryWrites'] == complete['syntheticHistoryWrites'] == 1
    previous_calls = []
    for row in (*rows, complete):
        current = row['historyRowMenuCalls']
        assert current[:len(previous_calls)] == previous_calls
        previous_calls = current
    photos = sorted(output.glob('ux_*.png'))
    assert [photo.stem for photo in photos] == (names if screenshots else [])
    hashes = {}
    for photo in photos:
        image = photo.read_bytes()
        assert image.startswith(b'\x89PNG\r\n\x1a\n') and len(image) > 1000
        hashes[photo.name] = hashlib.sha256(image).hexdigest()

    (context, song, voice, random_step, empty_song, empty_voice,
     blocked, rejected, song_play, paused, resumed, voice_play, *after_play) = rows
    actions = [row for name, row in by_name.items() if name.startswith('actions_')]
    assert (context['nativeHistorySize'], context['menuKey']) == (300, 'next')
    assert 0 < context['visibleRows'] < context['nativeHistorySize']
    for row, selection, clicked, gesture in (
        (context, ['3', '1'], '1', 'contextmenu'),
        (song, ['3', '1'], '1', 'contextmenu'),
        (voice, ['4', '2'], '2', 'keyboard'),
        (empty_song, ['4', '2'], '2', 'button'),
        (empty_voice, ['3', '1'], '1', 'button'),
    ):
        assert (row['selected'], row['clicked'], row['gesture']) == (selection, clicked, gesture)

    expected_ids = (SEED, SONGS, VOICES, VOICES, ['song:4', 'song:2'],
                    ['voice:3', 'voice:1'], PLAY_SEED, PLAY_SEED, PLAY_SONGS, PLAY_SONGS,
                    RESUMED_SONGS, PLAY_VOICES) + (PLAY_VOICES,) * len(after_play)
    for row, ids in zip(rows, expected_ids, strict=True):
        player, queue = row['nativePlayer'], row['nativeQueue']
        assert player['epoch'] == queue['epoch'] and player['entryId'] == queue['current']
        assert player['queueRevision'] == queue['revision'] and not player['error']
        assert player['playId'] and player['position'] > .25
        assert row['queueIds'] == projected(queue['tracks']) == ids

    initial = context['nativePlayer']
    assert (initial['state'], initial['mode'], initial['track']['id'], initial['track']['voiceId']) == (
        'paused', 'sequential', '1006', '6',
    )
    for row in (song, voice):
        assert row['notice'] == '已设为下一首播放'
        current = row['nativePlayer']
        assert all(current[key] == initial[key] for key in (
            'epoch', 'playId', 'entryId', 'state', 'position', 'track',
        ))
    assert song['nativeQueue']['revision'] != context['nativeQueue']['revision']
    assert voice['nativeQueue']['revision'] != song['nativeQueue']['revision']
    assert voice['nativePlayer']['mode'] == 'random'
    advanced = random_step['nativePlayer']
    assert random_step['previousPlayId'] == initial['playId'] != advanced['playId']
    assert advanced['epoch'] == initial['epoch'] and advanced['entryId'] != initial['entryId']
    assert (advanced['state'], advanced['mode'], advanced['track']['id'], advanced['track']['voiceId']) == (
        'playing', 'random', '1004', '4',
    )
    for row, prior, mode, song_id, voice_id in (
        (empty_song, advanced, 'random', '2', None),
        (empty_voice, empty_song['nativePlayer'], 'sequential', '1001', '1'),
    ):
        start = row['nativePlayer']
        assert row['empty']['ids'] == [] and row['empty']['mode'] == mode
        assert row['empty']['priorPlayId'] == prior['playId'] != start['playId']
        assert start['epoch'] == initial['epoch']
        assert row['empty']['revision'] != row['nativeQueue']['revision']
        assert (start['state'], start['mode'], start['track']['id'], start['track'].get('voiceId')) == (
            'playing', mode, song_id, voice_id,
        )
    assert (blocked['clicked'], blocked['selected'], blocked['playDisabled'], blocked['nextDisabled']) == (
        '8', ['8'], True, True,
    )
    assert blocked['copyrightHint'] == ['暂无版权', '暂无版权']
    assert blocked['keys'] == ['play', 'next', 'comments', 'collect', 'download', 'remove']
    assert (rejected['clicked'], rejected['selected'], rejected['focusRestored']) == (
        '8', ['3', '8'], True,
    )
    assert rejected['error'] and rejected['error'] != '播放记录或播放状态已更新，请重新打开菜单。'
    assert blocked['nativeQueue'] == rejected['nativeQueue']
    assert all(blocked['nativePlayer'][key] == rejected['nativePlayer'][key] for key in (
        'epoch', 'playId', 'entryId', 'state', 'position', 'track',
    ))
    before_play = blocked['nativePlayer']
    assert (before_play['state'], before_play['track']['id'], before_play['track']['voiceId']) == (
        'paused', '1012', '12',
    )
    assert (song_play['clicked'], song_play['selected']) == ('1', ['3', '8', '1'])
    native_play(song_play, rejected, song='1')
    assert (paused['clicked'], paused['selected']) == ('1', ['9', '1'])
    held = paused['nativePlayer']
    assert (held['state'], held['track']['id']) == ('paused', '1') and held['position'] > 3.5
    assert all(held[key] == song_play['nativePlayer'][key] for key in ('epoch', 'playId', 'entryId'))
    assert (resumed['clicked'], resumed['selected']) == ('1', ['9', '1'])
    native_play(resumed, paused, song='1', resume=True)
    assert (voice_play['clicked'], voice_play['selected']) == ('2', ['4', '2'])
    native_play(voice_play, resumed, song='1002', voice='2')
    audit_row_actions(actions, voice_play, complete)
    share = [by_name[name] for name in SHARE_STAGES]
    baseline = actions[0]['nativePlayer']
    for row in share:
        assert row['nativeQueue'] == voice_play['nativeQueue']
        assert all(row['nativePlayer'][key] == baseline[key] for key in (
            'epoch', 'playId', 'entryId', 'state', 'position', 'track',
        ))
    audit_row_share(share, by_name, complete)
    database = output.parent / 'state/core.sqlite3'
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as connection:
        journal = connection.execute(
            "SELECT owner, value FROM state WHERE kind='play-history-mutations-v1'"
        ).fetchall()
    assert len(journal) == 1 and journal[0][0] == '2'
    mutation = json.loads(journal[0][1])
    assert mutation['kind'] == 'song' and mutation['phase'] == 'confirmed'
    assert mutation['action'] == {'kind': 'delete', 'ids': ['10', '7']} and mutation['remote_planned']
    report = dict(ok=True, actualWebView='WebView2' if runtime else 'WebKitGTK',
                  stages=len(STAGES), screenshots=hashes, nativeNullPlayback=True,
                  preservedPausedClock=True, randomTraversalEpisode='4',
                  emptySongStarted='2', emptyVoiceStarted='1',
                  playSongStarted='1', playVoiceStarted='2', playResumedSameDecoder=True,
                  playRetainedUnrelatedEntries=True, copyrightBlockedSelection=True,
                  commentCounts={'song': 36, 'voice': 47}, commentCountFallback=True,
                  nativeClipboardChecks=5, directNativeClipboardChecks=2, cancelledDeletionWrites=0,
                  shareSongPrivilegeReads=5, shareSongCancelDidNotRecheck=True,
                  shareSongLateDenialPreservedClipboard=True, shareSongNativeClipboardCopy=True,
                  shareVoicePrivilegeReads=0,
                  shareVoiceQrDecoded='https://music.163.com/dj?id=10',
                  deletedSongIds=['10', '7'], syntheticHistoryWrites=1, confirmedDeletionJournal=True,
                  realMusicRequests=0, realAccountWrites=0)
    if runtime:
        report.update(runtime=runtime, network='synthetic-transports-only')
    else:
        locale = read(output / 'locale.json')
        assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
        report.update(locale=locale, network='loopback-only')
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path, help='same-run staged Windows native media directory')
    parser.add_argument('--screenshots', action='store_true', help='capture GTK screenshots on Linux')
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
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_HISTORY_PROOF='1',
               CUMULUS_HISTORY_ROW_NEXT_PROOF='1',
               CUMULUS_UX_SCRIPT=str(ROOT / 'desktop/examples/history_row_next_ui_probe.js'))
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
    report = audit(output, args.screenshots, runtime)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
