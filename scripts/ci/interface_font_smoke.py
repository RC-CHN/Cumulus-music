"""Exercise the main interface font control in a sealed Linux Tauri WebView.

Run under Xvfb with a window manager in a network namespace containing only
loopback. Native catalog data and PCM are synthetic; IPC, SQLite, restarts and
CSS are real. The probe verifies the applied font stack, not installed glyphs.
"""

import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import struct
import sys

from desktop_lyrics_driver import launch


PHASES = {
    'first': [
        ('main', None, 12), ('default', None, 12), ('fangsong', '仿宋', 13),
        ('lishu', '华文隶书', 15), ('localized', '合成黑体', 12), ('kaishu', '楷体', 14),
    ],
    'reopen': [
        ('main', '楷体', 14), ('missing_kaishu', '楷体', 14), ('reset_default', None, 12),
    ],
    'reset': [('main', None, 12), ('reopened_default', None, 12)],
}
FONT_CHOICES = ['', '仿宋', '华文隶书', '合成黑体', '楷体']
LOCALE = 'en_US.UTF-8'


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def local_font_row(path):
    with contextlib.closing(sqlite3.connect(path.as_uri() + '?mode=ro', uri=True)) as connection:
        rows = connection.execute(
            "SELECT owner, key, value FROM state WHERE kind = 'interface-font-v1'"
        ).fetchall()
        lyric_rows = connection.execute(
            "SELECT count(*) FROM state WHERE kind = 'desktop-lyric-preferences-v1'"
        ).fetchone()[0]
    assert len(rows) == 1 and rows[0][:2] == ('local-device', 'family'), rows
    assert lyric_rows == 0, 'main font selection persisted a desktop lyric preference'
    return json.loads(rows[0][2])


def visible_options(phase, name):
    if name == 'main':
        return None
    missing = name == 'missing_kaishu'
    names = FONT_CHOICES if phase != 'reopen' else (
        ['', '楷体', *FONT_CHOICES[1:-1]] if missing else FONT_CHOICES[:-1]
    )
    return [
        {'value': value, 'label': value or '默认', 'disabled': missing and value == '楷体'}
        for value in names
    ]


def native_clock(player):
    return {
        'state': player['state'], 'playId': player['playId'], 'entryId': player['entryId'],
        'trackId': player['track']['id'], 'position': player['position'],
        'queueRevision': player['queueRevision'], 'queueLen': player['queueLen'],
        'nativeSeeking': player['nativeSeeking'],
    }


def screenshot(path):
    raw = path.read_bytes()
    assert raw.startswith(b'\x89PNG\r\n\x1a\n') and raw[12:16] == b'IHDR', path
    width, height = struct.unpack('>II', raw[16:24])
    assert width > 0 and height > 0 and len(raw) > 1000, path
    return dict(sha256=hashlib.sha256(raw).hexdigest(), width=width, height=height)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    assert sys.platform == 'linux', 'this X11 proof runner is Linux only'
    assert {name for _, name in socket.if_nameindex()} == {'lo'}, 'use a sealed network namespace'
    assert len(Path('/proc/net/route').read_text(encoding='ascii').splitlines()) == 1
    assert os.environ.get('DISPLAY', '').startswith(':'), 'use a local X11 display'
    binary = args.binary.resolve(strict=True)
    dist = args.frontend_dist.resolve(strict=True)
    assert binary.is_file() and (dist / 'index.html').is_file()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(
        CUMULUS_PROOF_DIR=str(root), CUMULUS_PROOF_FRONTEND_DIST=str(dist),
        CUMULUS_PROOF_CAPTURE_ACK='1', GDK_BACKEND='x11', LC_ALL=LOCALE,
    )
    for directory in ['DATA_HOME', 'CONFIG_HOME', 'CACHE_HOME', 'STATE_HOME', 'RUNTIME_DIR']:
        path = root / 'os-profile' / directory.lower()
        path.mkdir(parents=True, mode=0o700)
        env['XDG_' + directory] = str(path)

    evidence = {}
    screenshots = {}
    first_default_css = None
    for phase, expected in PHASES.items():
        output = root / phase
        env['CUMULUS_INTERFACE_FONT_PROOF'] = phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            exit_code = launch(binary, env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert exit_code == 0, f'{phase} font probe exit {exit_code}'
        complete = read(output / 'complete.json')
        assert complete['ok'] and complete['phase'] == phase and complete['stageCount'] == len(expected)
        assert read(output / 'native-input.json') == {'keys': [], 'regions': []}
        paths = sorted(output.glob('ux_*.json'))
        assert len(paths) == len(expected)
        baseline = None
        stages = []
        for index, (path, (name, family, size)) in enumerate(zip(paths, expected, strict=True), 1):
            stage = read(path)
            assert stage['captureHandshake']
            marker = 'captured\n' if args.screenshots else 'recorded\n'
            assert path.with_suffix('.captured').read_text(encoding='ascii') == marker
            assert stage['stage'] == path.stem == f'ux_{index:02}_{name}'
            assert stage['phase'] == phase and stage['surface'] == 'main'
            assert stage['inSettings'] == (name != 'main')
            assert stage['options'] == visible_options(phase, name), stage['options']
            assert stage['selection'] == ((family or '') if name != 'main' else None)
            assert stage['preference'] == {'family': family, 'sizePx': size}
            assert stage['css']['size'] == f'{size}px'
            assert stage['css']['firstFamily'] == (family or 'Microsoft YaHei')
            assert 'sans-serif' in stage['css']['family']
            if family is None:
                if first_default_css is None:
                    first_default_css = stage['css']
                assert stage['css'] == first_default_css, stage['css']
            assert stage['lyricPreferencesUnchanged']
            assert stage['playback'] == native_clock(stage['nativePlayer'])
            if baseline is None:
                baseline = stage['playback']
                assert baseline['state'] == 'paused' and baseline['trackId'] == '1'
                assert baseline['playId'] and baseline['entryId'] and baseline['queueRevision']
                assert baseline['queueLen'] == 2 and not baseline['nativeSeeking']
                assert abs(baseline['position'] - 4) < .1
            assert stage['playback'] == baseline
            if name != 'main':
                source = 'interface-missing' if phase == 'reopen' else 'interface'
                assert set(stage['fontSources']) == {source} and stage['fontReads'] > 0
            stages.append(stage)
        captures = [*stages, complete]
        # Settings mounts AudioOutput, which lazily creates a separate device
        # inspector. Its scheduling is independent of the active decoder.
        factories = [row['factoryCount'] for row in captures]
        assert factories[0] == 1 and factories[-1] == 2 and factories == sorted(factories), factories
        for row in captures:
            assert row['mainVisible'] and row['lyricWindows'] == []
            assert row['lyricRequests'] == row['pendingLyrics'] == 0
            assert row['locale']['numeric'] == 'C'
            assert row['locale']['messages'] == row['locale']['originalMessages'] == LOCALE
        assert native_clock(complete['nativePlayer']) == baseline
        saved = local_font_row(root / 'state/core.sqlite3')
        assert saved == ('楷体' if phase == 'first' else None), saved
        evidence[phase] = {'persistedFamily': saved, 'stages': [stage['stage'] for stage in stages]}
        if args.screenshots:
            pngs = sorted(output.glob('ux_*.png'))
            assert [path.stem for path in pngs] == [path.stem for path in paths]
            screenshots.update({f'{phase}/{path.name}': screenshot(path) for path in pngs})

    report = dict(ok=True, platform='linux-x11', stateCount=sum(map(len, PHASES.values())),
                  phases=evidence, screenshots=screenshots, network='loopback-only',
                  nativePlayback='synthetic PCM/null', locale=LOCALE, physicalFontsVerified=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n',
                                      encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
