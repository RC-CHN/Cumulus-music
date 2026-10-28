"""Actual WebView catalog navigation using synthetic account/API data only."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sqlite3
import time


def launch(binary, env, log, root, screenshots):
    if not screenshots:
        return subprocess.run([str(binary.resolve())], env=env, stdout=log,
                              stderr=log, timeout=240).returncode, 0
    import gi
    gi.require_version('Gdk', '3.0')
    from gi.repository import Gdk
    process = subprocess.Popen([str(binary.resolve())], env=env, stdout=log, stderr=log)
    seen = set()
    try:
        deadline = time.monotonic() + 240
        while process.poll() is None:
            if time.monotonic() >= deadline:
                raise TimeoutError('catalog WebView did not finish')
            for path in (root / 'run/first').glob('ux_*.json'):
                if path in seen:
                    continue
                window = Gdk.get_default_root_window()
                picture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                assert picture is not None
                picture.savev(str(path.with_suffix('.png')), 'png', [], [])
                seen.add(path)
            time.sleep(.1)
        return process.returncode, len(seen)
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true', help='local Linux GTK captures; not required in CI')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--collections', action='store_true')
    mode.add_argument('--history', action='store_true')
    mode.add_argument('--settings', action='store_true')
    mode.add_argument('--relay', action='store_true')
    mode.add_argument('--relay-settings', action='store_true')
    mode.add_argument('--relay-publication', action='store_true')
    mode.add_argument('--relay-recovery', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root / 'run')
    if args.relay_publication:
        args.relay = True
        env['CUMULUS_RELAY_PUBLICATION_PROOF'] = '1'
    if args.relay_recovery:
        args.relay = True
        env['CUMULUS_RELAY_RECOVERY_PROOF'] = '1'
    if args.collections:
        env['CUMULUS_COLLECTION_PROOF'] = '1'
    if args.history:
        env['CUMULUS_HISTORY_PROOF'] = '1'
    if args.settings:
        env['CUMULUS_SETTINGS_PROOF'] = '1'
    if args.relay:
        env['CUMULUS_RELAY_PROOF'] = '1'
    if args.relay_settings:
        env['CUMULUS_RELAY_SETTING_PROOF'] = '1'
    for variable, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                                ('XDG_CACHE_HOME', 'cache'), ('APPDATA', 'roaming'),
                                ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / directory
        path.mkdir(parents=True)
        env[variable] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        returncode, captures = launch(args.binary, env, log, root, args.screenshots)
    output = root / 'run' / 'first'
    if (output / 'failed.json').exists():
        raise AssertionError((output / 'failed.json').read_text(encoding='utf-8'))
    assert returncode == 0, f'catalog probe exited {returncode}'
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    count = 14 if args.relay_recovery else 19 if args.relay else 22 if args.settings else 21 if args.relay_settings else 24 if args.history else 18
    assert complete['ok'] and complete['stageCount'] == count and complete['realMusicRequests'] == 0
    if args.screenshots:
        assert captures == count, 'missing WebView screenshots'
    if args.settings or args.relay_settings or args.relay_recovery:
        checks = complete['focusChecks']
        assert len(checks) == (7 if args.settings else 15 if args.relay_settings else 4)
        assert all(row['passed'] for row in checks), checks
    assert complete['playbackVerified'] == args.relay
    if args.relay_recovery:
        assert complete['syntheticRelayPulls'] == 2 and complete['syntheticQueueApplications'] == 2
        assert complete['decoder'] == 'libmpv' and complete['audioOutput'] == 'null'
        assert all(complete[key] for key in ['cancelFocus', 'lateIpcFenced', 'lostAckReadback', 'recoveryDidNotRestartAudio'])
        assert len(complete['syntheticRelayPublications']) == 4
        assert complete['publicationPhases'].count('confirmed') == 2
        assert complete['publicationPhases'].count('rejected') == 1
        assert complete['publicationPhases'].count('uncertain') == 1
        assert complete['publicationView']['blocked'] is None and complete['publicationView']['pending'] == 0
        with sqlite3.connect((root / 'run/state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
            cancelled = [json.loads(row[0]) for row in db.execute("SELECT value FROM state WHERE kind='relay-publication-cancelled-v1'")]
        assert len(cancelled) == 1 and cancelled[0]['device'] == 'SYNTHETIC_PRIVATE-local'
    elif args.relay:
        assert complete['syntheticRelayPulls'] == 4 and complete['syntheticQueueApplications'] == 2
        assert complete['decoder'] == 'libmpv' and complete['audioOutput'] == 'null'
        assert complete['mediaSubstitution'] == 'exact synthetic CDN URL to generated PCM'
        assert complete['autoDismiss']
        assert complete['focusHoldAfterPointerLeave'] and complete['hoverHoldAfterFocusLeave']
        assert complete['nativeWindowFocusHold'] and complete['nullRelatedFocusOutInjected']
        if args.relay_publication:
            uploads = complete['syntheticRelayPublications']
            assert len(uploads) >= 4 and len(uploads) == len(complete['publicationPhases'])
            assert all(phase == 'confirmed' for phase in complete['publicationPhases'])
            assert any(v.get('sourceType') or v.get('resources') for v in uploads)
            assert any(v.get('resource') == {'id': '5', 'type': 'voice'} and v.get('progress') == 18000 for v in uploads)
            assert not any('url' in v or 'deviceId' in v or 'cookie' in v for v in uploads)
    elif args.relay_settings:
        assert complete['syntheticRelaySettingWrites'] == 6 and complete['syntheticRelayPulls'] == 0
        assert complete['settingReadbackVerified'] and complete['noOptimisticCheckbox'] and complete['acceptCancelFocus']
        with sqlite3.connect((root / 'run/state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
            intents = [json.loads(row[0]) for row in db.execute("SELECT value FROM state WHERE kind='relay-setting-intents-v1'")]
            snapshots = [json.loads(row[0]) for row in db.execute("SELECT value FROM state WHERE kind='relay-setting-v1'")]
        assert sorted(op['phase'] for op in intents) == ['accepted_current_state', 'confirmed', 'confirmed', 'confirmed', 'confirmed', 'rejected']
        assert all(op['device_id'] == 'SYNTHETIC_PRIVATE-local' for op in intents)
        assert len(snapshots) == 1 and snapshots[0]['enabled'] is False
    elif args.settings:
        assert complete['syntheticSettingsWrites'] == 6 and complete['readbackBeforeSelection']
    elif args.history:
        assert complete['syntheticHistoryWrites'] == 2
    else:
        assert complete['collectionReadOnly'] if args.collections else complete['syntheticMembershipWrites']
    stages = [json.loads(p.read_text(encoding='utf-8')) for p in output.glob('ux_*.json')]
    assert len(stages) == count
    assert {v['theme'] for v in stages} == {'light', 'dark'}
    assert any(v['width'] == 760 and v['height'] == 540 for v in stages)
    if args.relay_recovery:
        by_name = {v['stage'][6:]: v for v in stages}
        assert by_name['blocked_playing']['publicationView']['blocked']['phase'] == 'rejected'
        assert by_name['uncertain_playing']['publicationView']['blocked']['phase'] == 'uncertain'
        assert by_name['prepared_offline']['publicationView']['preflightFailed']
        assert by_name['prepared_offline']['publicationView']['recovering']['phase'] == 'prepared'
        assert all(by_name[name]['publicationCalls'] == 1 for name in ['blocked_dark', 'confirm', 'cancelled_dialog', 'unknown_ipc', 'cancelled_unseen'])
        assert by_name['cancelled_unseen']['networkCalls'] == by_name['blocked_dark']['networkCalls']
        for name in ['blocked_playing', 'uncertain_playing']:
            assert by_name[name]['nativePlayer']['state'] == 'playing'
        for name in ['recovered_lost_ack', 'recovered', 'read_recovered']:
            assert by_name[name]['playerState'] == 'idle' and by_name[name]['publicationView']['blocked'] is None
    elif args.relay:
        assert all(not v['prompt'] or v['promptBottom'] <= v['playerTop'] - 3 for v in stages)
        by_name = {v['stage'][6:]: v for v in stages}
        for name in ['music_playing', 'voice_playing']:
            value = by_name[name]
            player, queue = value['nativePlayer'], value['nativeQueue']
            assert value['playing'] and player['state'] == 'playing' and player['error'] is None
            assert player['queueLen'] == len(queue['tracks']) == 2
            assert player['mode'] == 'order_loop'
            assert player['position'] >= 12.5 if name == 'voice_playing' else .2 < player['position'] < 10
        assert sum(v['playing'] for v in stages) == 2
        assert by_name['modal_suppression']['dialog'] and not by_name['modal_suppression']['prompt']
        assert not by_name['modal_restored']['dialog'] and by_name['modal_restored']['prompt']
        assert not by_name['auto_dismiss']['prompt'] and not by_name['closed_focus']['prompt']
    elif args.relay_settings:
        assert all(v['scrollBottom'] <= v['playerTop'] + 1 for v in stages)
        by_name = {v['stage'][6:]: v for v in stages}
        assert by_name['pending_enable']['busy'] and by_name['pending_enable']['disabled'] and not by_name['pending_enable']['checked']
        assert by_name['enabled']['checked'] and not by_name['disabled']['checked']
        assert by_name['lost_unapplied']['pending'] == by_name['lost_applied']['pending'] == by_name['read_failed']['pending'] == 1
        assert not by_name['accepted_current']['pending'] and not by_name['read_recovered']['pending']
        assert by_name['accept_dialog']['dialog'] and not by_name['accept_cancelled']['dialog']
        assert all(by_name[name]['disabled'] for name in ['missing', 'excluded', 'offline'])
        assert not by_name['restored']['checked'] and not by_name['restored']['disabled']
    elif args.settings:
        assert all(v['scrollBottom'] <= v['mainBottom'] + 1 for v in stages)
    elif args.history:
        assert all(v['rows'] <= 35 and v['footerBottom'] <= v['mainBottom'] + 1 for v in stages)
        assert {v['scope'] for v in stages} == {'歌曲', '播客', '有声书', '声音', '歌单', '专辑'}
    else:
        assert all(v['cards'] <= 30 for v in stages)
    if not args.collections and not args.history and not args.settings and not args.relay and not args.relay_settings:
        assert all(v['rows'] <= 30 for v in stages)
    result = {'ok': True, 'actualWebView': True, 'states': count,
                      'realMusicRequests': 0, 'playbackVerified': args.relay,
                      'collections': args.collections, 'history': args.history, 'settings': args.settings,
                      'relay': args.relay, 'relaySettings': args.relay_settings, 'physicalAudioOutputVerified': False,
                      'screenshots': captures}
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
