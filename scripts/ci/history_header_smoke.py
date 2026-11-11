"""Verify Recently Played header actions in a sealed Linux WebView."""

import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import sys

from motion_smoke import launch


STAGES = [
    'song_filtered_menu', 'song_add_all', 'song_batch', 'song_batch_back', 'song_clear_confirmation',
    'song_clear_cancelled', 'song_cleared', 'other_categories', 'voice_empty_filter_menu',
    'voice_add_all_started',
]
MENU = ['add-all', 'batch', 'clear']


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def projected(tracks):
    return [f"voice:{row['track']['voiceId']}" if row['track'].get('voiceId')
            else f"song:{row['track']['id']}" for row in tracks]


def audit(output, database, screenshots):
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stages'] == STAGES and complete['syntheticHistoryWrites'] == 1
    names = [f'ux_{index:02d}_{name}' for index, name in enumerate(STAGES, 1)]
    assert [file.stem for file in sorted(output.glob('ux_*.json'))] == names
    photos = sorted(output.glob('ux_*.png'))
    assert [photo.stem for photo in photos] == (names if screenshots else [])
    hashes = {}
    for photo in photos:
        content = photo.read_bytes()
        assert content.startswith(b'\x89PNG\r\n\x1a\n') and len(content) > 1000
        hashes[photo.name] = hashlib.sha256(content).hexdigest()
    states = {name: read(output / f'{path}.json') for name, path in zip(STAGES, names, strict=True)}
    for index, name in enumerate(STAGES):
        state = states[name]
        assert state['syntheticHistoryWrites'] == int(index >= STAGES.index('song_cleared'))
        if name == 'voice_empty_filter_menu':
            assert 'nativePlayer' not in state and state['queueIds'] == []
            continue
        player, queue = state['nativePlayer'], state['nativeQueue']
        assert player['epoch'] == queue['epoch'] and player['entryId'] == queue['current']
        assert player['queueRevision'] == queue['revision'] and player['position'] > .25
        assert not player['error'] and state['queueIds'] == projected(queue['tracks'])

    base = states['song_filtered_menu']
    assert base['category'] == '歌曲' and base['menu'] == MENU and base['nativeHistorySize'] == 300
    assert base['visibleRows'] == 1 and base['checkedRows'] == 0 and base['filter'] == '台湾 a'
    assert base['queueIds'] == ['voice:5', 'voice:6', 'voice:7']
    held = base['nativePlayer']
    assert (held['state'], held['mode'], held['track']['voiceId'], held['track']['id']) == (
        'paused', 'sequential', '6', '1006',
    )
    expected = ['voice:5', 'voice:6', *(f'song:{i}' for i in range(1, 301)), 'voice:7']
    first_added = states['song_add_all']
    assert first_added['visibleRows'] == 1 and first_added['checkedRows'] == 0
    assert first_added['filter'] == base['filter'] and first_added['notice'] == '已添加到播放列表'
    assert first_added['nativeQueue']['revision'] != base['nativeQueue']['revision']
    assert first_added['syntheticHistoryReads'] == base['syntheticHistoryReads']
    for name in STAGES[1:STAGES.index('voice_empty_filter_menu')]:
        state = states[name]
        assert state['queueIds'] == expected
        assert state['nativeQueue']['revision'] == first_added['nativeQueue']['revision']
        player = state['nativePlayer']
        assert all(player[key] == held[key] for key in (
            'epoch', 'playId', 'entryId', 'state', 'position', 'track', 'mode',
        ))
    batch = states['song_batch']
    assert batch['batch'] and not batch['browseTabs'] and batch['checkedRows'] == 1
    back = states['song_batch_back']
    assert back['browse'] and back['category'] == '歌曲' and back['checkedRows'] == 0
    assert batch['syntheticHistoryReads'] == back['syntheticHistoryReads'] == first_added['syntheticHistoryReads']
    clear = states['song_clear_confirmation']
    assert clear['title'] == '清空歌曲播放记录？'
    assert '全部历史，不仅是筛选结果' in clear['body'] and '多端记录' in clear['body']
    assert states['song_clear_cancelled']['historySize'] == 300
    assert states['song_cleared']['historySize'] == 0 and states['song_cleared']['emptyMenuDisabled']
    assert states['other_categories']['withoutQueueMenu'] == ['podcast', 'audiobook', 'playlist', 'album']

    voice = states['voice_empty_filter_menu']
    assert voice['category'] == '声音' and voice['menu'] == MENU
    assert voice['visibleRows'] == voice['checkedRows'] == 0 and voice['filter'] == '不存在的节目'
    assert sorted(voice['nativeVoiceOrder']) == sorted(f'voice:{i}' for i in range(1, 101))
    played = states['voice_add_all_started']
    assert played['queueIds'] == played['nativeVoiceOrder'] == voice['nativeVoiceOrder']
    assert played['visibleRows'] == played['checkedRows'] == 0 and played['filter'] == voice['filter']
    voice_id = voice['nativeVoiceOrder'][0].split(':', 1)[1]
    player = played['nativePlayer']
    assert player['state'] == 'playing' and player['playId'] != held['playId']
    assert player['track']['voiceId'] == voice_id and player['track']['id'] == str(int(voice_id) + 1000)

    locale = read(output / 'locale.json')
    assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as connection:
        rows = connection.execute(
            "SELECT owner, value FROM state WHERE kind='play-history-mutations-v1'"
        ).fetchall()
    assert len(rows) == 1 and rows[0][0] == '2'
    operation = json.loads(rows[0][1])
    assert operation['kind'] == 'song' and operation['action'] == {'kind': 'clear'}
    assert operation['phase'] == 'confirmed' and operation['remote_planned']
    return dict(ok=True, actualLinuxWebView=True, nativeNullPlayback=True, stages=len(names),
                screenshots=hashes, songsInserted=300, voicesStartedFromFullList=100,
                syntheticHistoryWrites=1, realMusicRequests=0, realAccountWrites=0,
                locale=locale, network='loopback-only')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if not sys.platform.startswith('linux') or not os.environ.get('DISPLAY', '').startswith(':'):
        parser.error('a Linux X11 WebView session is required')
    if ({name for _, name in socket.if_nameindex()} != {'lo'}
            or len(Path('/proc/net/route').read_text(encoding='ascii').splitlines()) != 1):
        parser.error('use a sealed network namespace containing only loopback')
    os.environ['GDK_BACKEND'] = 'x11'
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/history_header_ui_probe.js'
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11', CUMULUS_PROOF_DIR=str(root / 'run'),
               CUMULUS_HISTORY_PROOF='1', CUMULUS_HISTORY_HEADER_PROOF='1',
               CUMULUS_UX_SCRIPT=str(source.resolve(strict=True)))
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
    report = audit(output, root / 'run/state/core.sqlite3', args.screenshots)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
