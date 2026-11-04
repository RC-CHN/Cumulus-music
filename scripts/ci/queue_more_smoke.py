"""Observe the official recommendation queue in an actual WebView and libmpv.

The account, two API responses, and PCM are synthetic. No upstream requests or
real account writes are allowed. Cases use independent app profiles.
"""
import argparse
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch

MORE = '/api/homepage/more/rcmd/song'
PRIVILEGE = '/api/song/enhance/privilege'
SEEDS = ['1', '2']
APPENDED = SEEDS + ['51', '52', '53']
RETAINED = SEEDS + ['52']
# stage -> (track IDs, decoder loads, fixture requests, live recommended rows)
ASSIGNED = {
    'unopened': (SEEDS, 1, 0, 0),
    'held': (SEEDS, 1, 1, 0),
    'appended': (APPENDED, 1, 2, 3),
    'reopened': (APPENDED, 1, 2, 3),
    'selected': (APPENDED, 2, 2, 3),
    'eof': (APPENDED, 3, 2, 3),
    'off_retained': (RETAINED, 3, 2, 0),
    'off_during_fetch': (RETAINED, 3, 3, 0),
    'late_discarded': (RETAINED, 3, 4, 0),
}
CONTROL = {'control': (SEEDS, 1, 0, 0)}
TAIL = {
    'tail_pending': (SEEDS, 2, 1, 0),
    'tail_appended': (APPENDED, 2, 2, 3),
    'tail_eof': (APPENDED, 3, 2, 3),
}
LATE_TAIL = {
    'late_tail_pending': (SEEDS, 2, 1, 0),
    'late_tail_ended': (SEEDS, 2, 1, 0),
    'late_tail_appended': (APPENDED, 2, 2, 3),
}
CASES = {'assigned': ASSIGNED, 'control': CONTROL, 'tail': TAIL, 'late_tail': LATE_TAIL}


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def assert_requests(requests, case):
    first_current = '2' if case in ['tail', 'late_tail'] else '1'
    assert [item['path'] for item in requests] == [MORE, PRIVILEGE, MORE, PRIVILEGE][:len(requests)]
    for index, request in enumerate(requests):
        payload = request['args']
        assert request['cache'] is False, request
        if index % 2 == 0:
            assert set(payload) == {'songIds', 'currentPlaySongId', 'source'}, request
            assert payload['source'] == 'curlist_more_rcmd', request
            assert payload['songIds'] == (SEEDS if index == 0 else RETAINED), request
            assert payload['currentPlaySongId'] == (first_current if index == 0 else '52'), request
        else:
            assert set(payload) == {'ids'}, request
            assert json.loads(payload['ids']) == (
                APPENDED[2:] if index == 1 else ['61', '62', '63']
            ), request


def verify_stages(output, case, screenshots):
    states = CASES[case]
    initial, chosen, after_eof, terminal = None, None, None, None
    for index, (label, (ids, loads, calls, tagged)) in enumerate(states.items(), 1):
        path = output / f'ux_{index:02}_{label}.json'
        row = read(path)
        native, queue = row['nativePlayer'], row['nativeQueue']
        model = queue['moreRecommendations']
        tracks = queue['tracks']
        actual_ids = [entry['track']['id'] for entry in tracks]
        assert actual_ids == ids and native['queueLen'] == len(ids), (label, actual_ids)
        assert queue['current'] == native['entryId'] and queue['revision'] == native['queueRevision'], label
        assert row['model'] == model and row['playId'] == native['playId'] and not native['error'], label
        assert row['sourceAudio']['loads'] == loads, (label, row['sourceAudio'])
        assert len(row['syntheticQueueMore']['requests']) == calls, label
        assert_requests(row['syntheticQueueMore']['requests'], case)
        assert [entry['moreRecommendation'] for entry in tracks] == [
            i >= len(ids) - tagged for i in range(len(ids))
        ], label
        if tagged:
            anchor = tracks[len(ids) - tagged]['entryId']
            assert model['separatorBefore'] == anchor, label
            if row['drawerOpen']:
                assert row['anchor'] == anchor, label
                expected_badges = {entry['entryId'] for entry in tracks if entry['moreRecommendation']}
                assert {r['entryId'] for r in row['rows'] if r['badge']} == expected_badges, label
        else:
            assert row['anchor'] is None and (model or {}).get('separatorBefore') is None, label
            assert not any(r['badge'] for r in row['rows']), label
        assert bool(model and model['visible']) == (case != 'control'), label
        if label == 'control':
            assert row['toggle'] is None and not (model and model['prefetchOnOpen']), label
        elif case in ['tail', 'late_tail']:
            assert not row['drawerOpen'] and row['toggle'] is None and row['anchor'] is None, label
            assert not row['rows'] and not row['actionCalls'], label
            terminal = terminal or native['playId']
            if label == 'tail_eof':
                assert native['track']['id'] == '51' and native['playId'] != terminal, label
                assert native['state'] == 'playing' and native['position'] > .12, label
            else:
                assert native['track']['id'] == '2' and native['playId'] == terminal != row['initialPlayId'], label
                if label in ['late_tail_ended', 'late_tail_appended']:
                    assert native['state'] == 'ended' and abs(native['position'] - 30) < .25, label
                else:
                    assert native['state'] == 'playing' and .12 < native['position'] < 29, label
            if case == 'late_tail':
                assert native['mode'] == 'sequential', label
        elif label in ['unopened', 'held', 'appended', 'reopened']:
            initial = initial or native['playId']
            assert native['playId'] == initial and native['track']['id'] == '1', label
            assert model['enabled'] and model['canToggle'], label
            if label == 'unopened':
                assert row['toggle'] is None and model['prefetchOnOpen'], label
            else:
                assert row['toggle'] == 'true' and native['state'] == 'paused', label
                assert abs(native['position'] - 3) < .25, label
        elif label == 'selected':
            chosen = native['playId']
            assert chosen != initial and native['track']['id'] == '51' and native['state'] == 'playing', label
        elif label == 'eof':
            after_eof = native['playId']
            assert after_eof not in [chosen, initial] and native['track']['id'] == '52', label
            assert native['state'] == 'playing' and native['position'] > .12, label
        else:
            assert native['playId'] == after_eof and native['track']['id'] == '52', label
            assert native['state'] == 'paused' and abs(native['position'] - 3) < .25, label
            assert not model['enabled'] and model['canToggle'] and row['toggle'] == 'false', label
        if screenshots:
            assert path.with_suffix('.png').is_file(), label
    return len(states)


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
        window = Gdk.get_default_root_window()
        assert window and window.get_width() > 0, 'screenshots require X11; set GDK_BACKEND=x11 under Xvfb'
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    script = (Path(__file__).resolve().parents[2] / 'desktop/examples/queue_more_ui_probe.js').read_text(encoding='utf-8')
    states = 0
    locale_observed = {}
    for name in CASES:
        control = name == 'control'
        case = root / name
        case.mkdir()
        env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
        env.update(CUMULUS_PROOF_DIR=str(case), CUMULUS_MOTION_PROOF='1',
                   CUMULUS_QUEUE_UI_PROOF='1', CUMULUS_QUEUE_MORE_UI_PROOF='1')
        if control:
            env['CUMULUS_QUEUE_MORE_CONTROL'] = '1'
        for key, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                               ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                               ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
            isolated = case / 'os-profile' / directory
            isolated.mkdir(parents=True, mode=0o700)
            env[key] = str(isolated)
        if args.runtime:
            env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
        if args.frontend_dist:
            env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
        isolated_script = case / 'probe.js'
        isolated_script.write_text(f'window.__queueMoreCase = {json.dumps(name)};\n' + script, encoding='utf-8')
        env['CUMULUS_UX_SCRIPT'] = str(isolated_script)
        output = case / 'first'
        with (case / 'host.log').open('x', encoding='utf-8') as log:
            status = launch(args.binary.resolve(), env, log, output, args.screenshots)
        failure = output / 'failed.json'
        assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
        assert status == 0, f'{name} recommendation probe exit {status}'
        complete = read(output / 'complete.json')
        assert complete['ok'] and complete['testCase'] == name
        assert complete['realMusicRequests'] == complete['accountWrites'] == 0
        count = verify_stages(output, name, args.screenshots)
        assert complete['stageCount'] == count
        assert_requests(complete['syntheticQueueMore']['requests'], name)
        expected_reads = 4 if name == 'assigned' else 0 if control else 2
        assert len(complete['syntheticQueueMore']['requests']) == expected_reads
        actions = [item['kind'] for item in complete['actionCalls']]
        if name == 'assigned':
            assert actions[0] == 'queue_more_opened' and actions.count('queue_more_toggle') == 3
        else:
            assert not actions
        states += count
        if sys.platform == 'linux':
            observed = read(output / 'locale.json')
            assert observed['numeric'] == 'C', observed
            for category, key in [('LC_MESSAGES', 'messages'), ('LC_CTYPE', 'characters')]:
                expected = env.get('LC_ALL') or env.get(category) or env.get('LANG') or 'C'
                assert expected.lower() not in {'c', 'posix'}, 'run with a non-C GUI locale'
                assert observed[key].casefold().replace('-', '') == expected.casefold().replace('-', ''), observed
            locale_observed[name] = observed
    report = dict(ok=True, actualWebView=True, nativeLibmpvNull=True, nativeEof=True,
                  controlledExperiment=True, automaticTail=True, lateTailStayedEnded=True, locales=locale_observed, states=states, screenshots=states if args.screenshots else 0,
                  syntheticApi=True, realMusicRequests=0, realAccountWrites=0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
