"""Actual WebView recommendation cue with native focus, libmpv and SQLite."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import sys

from motion_smoke import launch
from queue_more_smoke import assert_requests

CUE = '列表播完已自动推荐，↓点击去设置'
PHASES = {
    'first': ['unfocused', 'at_zero', 'past_window', 'shown', 'seen', 'another_rec'],
    'reopen': ['restart_suppressed'],
}


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def assert_sqlite(root):
    path = root / 'state' / 'core.sqlite3'
    assert path.is_file()
    with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True) as connection:
        found = connection.execute(
            'SELECT owner,value FROM state WHERE kind=? AND key=? ORDER BY owner',
            ('queue-more-v1', 'tip-seen'),
        ).fetchall()
    assert found == [('2', 'true')], found


def verify_stage(row, label, base_player):
    proof, native, queue = row['syntheticQueueTip'], row['nativePlayer'], row['nativeQueue']
    current = next(entry for entry in queue['tracks'] if entry['entryId'] == queue['current'])
    assert current['moreRecommendation'] and current['entryId'] == native['entryId'], label
    assert native['playId'] == row['playId'] and native['queueRevision'] == queue['revision'], label
    assert native['state'] == row['state'] and abs(native['position'] - row['position']) < .6, label
    assert native['track']['id'] == row['trackId'] == ('52' if label == 'another_rec' else '51'), label
    assert proof['nativeVisible'] and row['visible'] and not row['drawerOpen'], label
    assert proof['nativeFocused'] == row['focused'] == (label != 'unfocused'), label
    assert proof['seen'] == (label in ['seen', 'another_rec', 'restart_suppressed']), label
    assert row['cue'] == (CUE if label == 'shown' else None), label
    assert row['cueCount'] == int(label == 'shown'), label
    assert not native['error'], label
    calls = row['calls']
    if label in ['unfocused', 'at_zero', 'past_window']:
        assert calls == [], label
    elif label in ['shown', 'restart_suppressed']:
        assert [(c['kind'], c['granted']) for c in calls] == [('begin', label == 'shown')], label
    else:
        assert [(c['kind'], c['granted']) for c in calls] == [('begin', True), ('seen', False)], label
        assert 4900 <= calls[1]['at'] - calls[0]['at'] < 8500, label
    if label == 'at_zero':
        assert native['state'] == 'paused' and native['position'] == 0, label
    elif label == 'past_window':
        assert native['state'] == 'playing' and native['position'] > 30, label
    else:
        assert native['state'] == 'playing' and 0 < native['position'] <= 30, label
    if label == 'seen':
        assert 4600 <= row['durationMs'] < 8500, label
    if label in ['unfocused', 'at_zero', 'past_window', 'shown', 'seen']:
        assert (native['playId'], row['sourceAudio']['loads']) == base_player, label
    elif label == 'another_rec':
        assert native['playId'] != base_player[0] and row['sourceAudio']['loads'] == base_player[1] + 1, label
    reads = row['syntheticQueueMore']['requests']
    assert len(reads) == 2, label
    assert_requests(reads, 'assigned')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true', help='optional Linux GTK captures on X11')
    args = parser.parse_args()
    if args.screenshots:
        import gi
        gi.require_version('Gtk', '3.0')
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk, Gtk
        assert Gtk.init_check([])[0], 'GTK could not connect to the capture display'
        assert Gdk.get_default_root_window().get_width() > 0, 'set GDK_BACKEND=x11 under Xvfb'
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_QUEUE_UI_PROOF='1',
               CUMULUS_QUEUE_MORE_UI_PROOF='1', CUMULUS_QUEUE_MORE_TIP_PROOF='1')
    for key, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                           ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                           ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / directory
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    source = (Path(__file__).resolve().parents[2] / 'desktop/examples/queue_more_tip_ui_probe.js').read_text(encoding='utf-8')
    count = 0
    for phase, labels in PHASES.items():
        reopen = phase == 'reopen'
        script = root / f'{phase}.js'
        script.write_text(f'window.__queueTipReopen={json.dumps(reopen)};\n' + source, encoding='utf-8')
        env['CUMULUS_UX_SCRIPT'] = str(script)
        if reopen:
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, output, args.screenshots)
        failure = output / 'failed.json'
        assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
        assert code == 0, f'{phase} cue probe exited {code}'
        complete = read(output / 'complete.json')
        assert complete['ok'] and complete['reopened'] == reopen and complete['count'] == len(labels)
        assert complete['realMusicRequests'] == complete['accountWrites'] == 0
        assert_sqlite(root)
        base_player = None
        for index, label in enumerate(labels, 1):
            path = output / f'ux_{index:02}_{label}.json'
            row = read(path)
            base_player = base_player or (row['nativePlayer']['playId'], row['sourceAudio']['loads'])
            verify_stage(row, label, base_player)
            if args.screenshots:
                assert path.with_suffix('.png').is_file(), label
        if sys.platform == 'linux':
            locale = read(output / 'locale.json')
            assert locale['numeric'] == 'C', locale
            for category, key in [('LC_MESSAGES', 'messages'), ('LC_CTYPE', 'characters')]:
                expected = env.get('LC_ALL') or env.get(category) or env.get('LANG') or 'C'
                assert expected.lower() not in {'c', 'posix'}, 'use a non-C GUI locale'
                assert locale[key].casefold().replace('-', '') == expected.casefold().replace('-', ''), locale
        count += len(labels)
    report = dict(ok=True, actualWebView=True, nativeLibmpvNull=True, nativeWindowFocus=True,
                  zeroAndPastThirtyRejected=True, fiveSecondCue=True, sqliteOncePerUidAcrossRestart=True,
                  states=count, screenshots=count if args.screenshots else 0, syntheticApi=True,
                  realMusicRequests=0, realAccountWrites=0, physicalAudioOutputVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
