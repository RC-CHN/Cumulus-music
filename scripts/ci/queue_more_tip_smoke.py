"""Actual WebView recommendation cue with native focus, libmpv and SQLite."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
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


MILESTONES = frozenset((
    'graphics_prepared', 'context_loaded', 'setup_enter', 'registry_opened',
    'client_managed', 'proof_listener_installed',
    'main_window_build_enter', 'main_window_built', 'focus_helper_build_enter', 'focus_helper_built',
    'main_page_loaded', 'script_queued', 'renderer_enter', 'daily_ready', 'daily_load_begin',
    'audio_factory_enter', 'audio_factory_returned', 'daily_playing', 'recommendations_loaded',
    'focus_enter', 'focus_returned', 'focus_observed', 'recommendation_playing',
    'focus_window_request_failed', 'focus_foreground_mismatch', 'focus_foreground_observed',
    'focus_controller_request_failed', 'focus_controller_requested', 'focus_native_mismatch',
    'focus_native_observed', 'focus_renderer_mismatch',
    'cue_check_begin', 'cue_denied', 'second_recommendation_playing',
    'capture_acknowledged', 'complete_written', 'shutdown_stop_enter',
    'shutdown_stop_returned', 'run_returned',
))
LOG_LIMIT = 256 * 1024
MILESTONE_LIMIT = 64
FAILURE_REASONS = frozenset((
    'child_exit', 'renderer_failure', 'missing_complete', 'launch_timeout', 'launch_error',
))


def phase_failure_receipt(root, phase, code, reason):
    if phase not in PHASES or reason not in FAILURE_REASONS:
        raise ValueError('unknown cue diagnostic phase or reason')
    output = root / phase
    try:
        with (root / f'{phase}.log').open('rb') as log:
            payload = log.read(LOG_LIMIT + 1)
    except FileNotFoundError:
        payload = b''
    prefix = f'queue-tip-{phase}:'.encode('ascii')
    labels = PHASES[phase]
    allowed = MILESTONES | {f'capture_written_{label}' for label in labels}
    names = []
    truncated = len(payload) > LOG_LIMIT
    # A trailing partial line is not proof the full breadcrumb was written.
    bounded = payload[:LOG_LIMIT].rpartition(b'\n')[0]
    for line in bounded.splitlines():
        if len(names) == MILESTONE_LIMIT:
            break
        if line.startswith(prefix):
            name = line[len(prefix):].decode('ascii', errors='replace')
            if name in allowed:
                names.append(name)
    status = None if code is None else code & 0xFFFFFFFF
    stages = [label for index, label in enumerate(labels, 1)
              if (output / f'ux_{index:02}_{label}.json').is_file()]
    receipt = dict(schema='cumulus-queue-tip-failure-v2', phase=phase, reason=reason,
                   windowsExitStatus=None if status is None else f'0x{status:08X}',
                   stackOverflow=status == 0xC00000FD, milestones=names,
                   lastMilestone=names[-1] if names else None,
                   milestoneLimitReached=len(names) == MILESTONE_LIMIT,
                   logReadTruncated=truncated, stageFilesPresent=stages,
                   completeWritten=(output / 'complete.json').is_file(),
                   failedWritten=(output / 'failed.json').is_file())
    temporary = root / f'{phase}-failure-receipt.tmp'
    temporary.write_text(json.dumps(receipt, indent=2) + '\n', encoding='utf-8')
    temporary.replace(root / f'{phase}-failure-receipt.json')
    return receipt


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
    eligible = label in ['unfocused', 'at_zero', 'past_window']
    assert bool(native.get('queueMoreTipEligible')) == row['eligible'] == eligible, label
    calls = row['calls']
    if label in ['unfocused', 'at_zero', 'past_window']:
        assert calls == [], label
    elif label == 'restart_suppressed':
        assert [(c['kind'], c['granted']) for c in calls] in [[], [('begin', False)]], label
    elif label == 'shown':
        assert [(c['kind'], c['granted']) for c in calls] == [('begin', True)], label
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
        trace_windows = sys.platform == 'win32'
        script.write_text(f'window.__queueTipReopen={json.dumps(reopen)};\n'
                          f'window.__queueTipPhaseMilestones={json.dumps(trace_windows)};\n' + source,
                          encoding='utf-8')
        env['CUMULUS_UX_SCRIPT'] = str(script)
        if reopen:
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        try:
            with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
                code = launch(args.binary.resolve(), env, log, output, args.screenshots)
        except (OSError, subprocess.TimeoutExpired) as error:
            if trace_windows:
                reason = 'launch_timeout' if isinstance(error, subprocess.TimeoutExpired) else 'launch_error'
                print(json.dumps(phase_failure_receipt(root, phase, None, reason)), flush=True)
            raise
        failure = output / 'failed.json'
        if trace_windows:
            reason = ('child_exit' if code != 0 else 'renderer_failure' if failure.is_file()
                      else 'missing_complete' if not (output / 'complete.json').is_file() else None)
            if reason:
                print(json.dumps(phase_failure_receipt(root, phase, code, reason)), flush=True)
        assert not failure.exists(), (f'{phase} cue renderer reported failure' if trace_windows
                                      else failure.read_text(encoding='utf-8') if failure.exists() else '')
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
                if label == 'shown':
                    paint = read(path.with_name('paint-' + path.name))
                    assert paint['samples'] and all(sample['ok'] for sample in paint['samples'])
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
