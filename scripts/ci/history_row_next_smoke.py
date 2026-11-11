"""Verify Recently Played row Next through the sealed desktop WebView."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import sys

from history_header_smoke import projected, read
from motion_smoke import launch
from playback_power_smoke import windows_runtime


ROOT = Path(__file__).resolve().parents[2]
STAGES = (
    'song_context', 'song_inserted', 'voice_inserted', 'voice_random_step',
    'empty_song_clicked', 'empty_voice_clicked',
)
SEED = ['voice:5', 'voice:6', 'voice:7']
SONGS = ['voice:5', 'voice:6', 'song:3', 'song:1', 'voice:7']
VOICES = ['voice:5', 'voice:6', 'voice:4', 'voice:2', 'song:3', 'song:1', 'voice:7']


def audit(output, screenshots, runtime=None):
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stages'] == list(STAGES)
    names = [f'ux_{index:02d}_{stage}' for index, stage in enumerate(STAGES, 1)]
    assert [path.stem for path in sorted(output.glob('ux_*.json'))] == names
    rows = [read(output / f'{name}.json') for name in names]
    assert all(row['syntheticHistoryWrites'] == 0 for row in [*rows, complete])
    photos = sorted(output.glob('ux_*.png'))
    assert [photo.stem for photo in photos] == (names if screenshots else [])
    hashes = {}
    for photo in photos:
        image = photo.read_bytes()
        assert image.startswith(b'\x89PNG\r\n\x1a\n') and len(image) > 1000
        hashes[photo.name] = hashlib.sha256(image).hexdigest()

    context, song, voice, random_step, empty_song, empty_voice = rows
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

    for row, ids in zip(rows, (SEED, SONGS, VOICES, VOICES, ['song:4', 'song:2'],
                               ['voice:3', 'voice:1']), strict=True):
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
    report = dict(ok=True, actualWebView='WebView2' if runtime else 'WebKitGTK',
                  stages=len(STAGES), screenshots=hashes, nativeNullPlayback=True,
                  preservedPausedClock=True, randomTraversalEpisode='4',
                  emptySongStarted='2', emptyVoiceStarted='1', syntheticHistoryWrites=0,
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
