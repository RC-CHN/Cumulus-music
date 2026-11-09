"""Open both Recently Played chapter links in a sealed GTK WebView and inspect native playback."""
import argparse
import json
import os
from pathlib import Path
import socket
import sys

from motion_smoke import launch

STAGES = ('podcast_subtitle', 'podcast_play', 'audiobook_subtitle', 'audiobook_play')
PROGRAMS = '/api/v6/dj/program/byradio'
RIGHTS = '/api/dj/play/url/check'
SOURCE = '/api/song/enhance/player/url/v1'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def requests(value, path):
    return [call['args'] for call in value['historyChapterCalls'] if call['path'] == path]


def verify(output, screenshots):
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['historyRadioChapterVerified']
    assert complete['stageCount'] == len(STAGES)
    names = [f'ux_{index:02d}_{stage}' for index, stage in enumerate(STAGES, 1)]
    assert sorted(path.stem for path in output.glob('ux_*.json')) == names
    assert sorted(path.stem for path in output.glob('ux_*.png')) == (names if screenshots else [])
    states = [read(output / f'{stage}.json') for stage in names]
    assert all(not value['syntheticHistoryWrites'] for value in [*states, complete])
    assert all(not call['cached'] for call in complete['historyChapterCalls'])
    assert requests(complete, '/api/play-record/djradio/list')
    assert all(body == {'filterVoiceBook': True}
               for body in requests(complete, '/api/play-record/djradio/list'))
    assert requests(complete, '/api/play-record/voice/book/timeline/list')
    assert all(body == {} for body in requests(complete, '/api/play-record/voice/book/timeline/list'))
    for path in ('/api/djradio/v3/get', '/api/dj/around/program/v1'):
        assert not requests(complete, path), f'unneeded endpoint: {path}'
    for index, count in enumerate((0, 1, 1, 2)):
        for path in (PROGRAMS, RIGHTS, SOURCE):
            assert len(requests(states[index], path)) == count, (names[index], path)
    assert requests(complete, PROGRAMS) == [
        {'radioId': '81', 'limit': 250, 'offset': 0},
        {'radioId': '82', 'limit': 250, 'offset': 0},
    ]
    assert requests(complete, RIGHTS) == [{'voiceId': '812'}, {'voiceId': '822'}]
    assert requests(complete, '/api/dj/program/detail') == [{'id': '812'}, {'id': '822'}]
    assert [json.loads(body['ids']) for body in requests(complete, SOURCE)] == [['1812'], ['1822']]
    for listing, playback, title, radio in zip(states[::2], states[1::2],
                                              ('播客第二集', '有声书第二章'), (81, 82), strict=True):
        assert listing['subtitle'] == '续听：' + title and playback['subtitle'] is None
        assert listing['chapter'] == playback['chapter'] == {
            'name': title, 'tag': '续听', 'recent': False, 'percent': None,
            'completed': False, 'playable': True,
        }
        assert listing['staticChapter'] == ('只有标题的单集' if radio == 81 else None)
        assert not listing['playing'] and 'nativePlayer' not in listing
        assert playback['playing']
        player, queue = playback['nativePlayer'], playback['nativeQueue']
        assert player['state'] == 'playing' and player['resourceKind'] == 'voice'
        assert player['position'] > .2 and not player['error']
        assert player['playId'] == playback['playId'] != listing['previousPlayId']
        assert queue['current'] == player['entryId'] == playback['entryId']
        assert player['queueRevision'] == queue['revision']
        assert player['queueLen'] == len(queue['tracks']) == 2
        voices = [str(radio * 10 + 1), str(radio * 10 + 2)]
        media = [str(int(voice) + 1000) for voice in voices]
        assert [row['track']['voiceId'] for row in queue['tracks']] == voices
        assert [row['track']['id'] for row in queue['tracks']] == media
        assert player['track']['voiceId'] == voices[1] and player['track']['id'] == media[1]
    assert states[0]['previousPlayId'] is None
    assert states[2]['previousPlayId'] == states[1]['playId']
    locale = read(output / 'locale.json')
    assert locale == {'numeric': 'C', 'messages': 'en_US.UTF-8', 'characters': 'en_US.UTF-8'}
    return {'ok': True, 'actualLinuxWebView': True, 'nativeNullPlayback': True, 'stages': len(names),
            'screenshots': len(names) if screenshots else 0, 'locale': locale, 'historyWrites': 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if not sys.platform.startswith('linux') or not os.environ.get('DISPLAY'):
        parser.error('a Linux X11 WebView session is required')
    if ({name for _, name in socket.if_nameindex()} != {'lo'}
            or len(Path('/proc/net/route').read_text(encoding='ascii').splitlines()) != 1):
        parser.error('run in a sealed network namespace with only loopback and no external routes')
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    env = {key: value for key, value in os.environ.items() if not key.upper().startswith('CUMULUS_')}
    env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11', CUMULUS_PROOF_DIR=str(root / 'run'),
               CUMULUS_HISTORY_PROOF='1', CUMULUS_HISTORY_RADIO_CHAPTER_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2]
                                    / 'desktop/examples/history_radio_chapter_ui_probe.js'))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / key
        path.mkdir(mode=0o700)
        env[key] = str(path)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(binary, env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    assert result == 0, f'native probe exit {result}'
    report = verify(output, args.screenshots)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
