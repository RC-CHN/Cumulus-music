"""Exercise production shortcut settings and local key dispatch in a real WebView.

DOM keys and synthetic PCM exercise IPC/native playback, not physical OS delivery.
Run shortcut_smoke.py separately for independent native key injection.
"""
import argparse
import json
import os
from pathlib import Path
import sqlite3
from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k:v for k,v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_SHORTCUT_UI_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path('desktop/examples/shortcuts_ui_probe.js').resolve()))
    for key, name in [('XDG_DATA_HOME','data'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),
                      ('XDG_RUNTIME_DIR','runtime'),('APPDATA','roaming'),('LOCALAPPDATA','local')]:
        directory = root/'os-profile'/name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH','')
    for phase, count in [('first',16), ('reopen',2)]:
        if phase == 'reopen': env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root/phase
        with (root/f'{phase}.log').open('x',encoding='utf-8') as log:
            result = launch(args.binary.resolve(),env,log,output,args.screenshots)
        if (output/'failed.json').exists(): raise AssertionError((output/'failed.json').read_text(encoding='utf-8'))
        assert result == 0, result
        complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount'] == count and complete['realMusicRequests'] == 0
        assert complete['nativePlayer']['state'] == 'paused'
        snapshots = [json.loads(p.read_text(encoding='utf-8')) for p in sorted(output.glob('ux_*.json'))]
        assert len(snapshots) == count
        baseline = snapshots[0]['audioFactoryCount']
        assert baseline >= 1
        if args.screenshots: assert len(list(output.glob('ux_*.png'))) == count
        for snap in snapshots:
            assert snap['audioFactoryCount'] == baseline, 'shortcut setting change must not recreate audio'
            assert snap['phase'] == snap['shortcuts']['phase']
            prefs = snap['shortcutSettings']['current']
            if prefs:
                assert prefs['global_enabled'] == snap['globalEnabled']
                assert prefs['media_enabled'] == snap['mediaEnabled']
        assert complete['shortcutSettings']['current']['bindings']['play_pause']['local'] == [79]
        assert complete['shortcutSettings']['current']['global_enabled'] is False
        assert complete['shortcutSettings']['current']['media_enabled'] is False
        assert complete['audioFactoryCount'] == baseline, 'settings/reload must not recreate audio'
        if phase == 'first':
            assert len({snap['nativePlayer']['playId'] for snap in snapshots[1:]}) == 1
    backups = []
    for path in (root/'state').rglob('core.sqlite3'):
        with sqlite3.connect(path) as connection:
            backups.extend(json.loads(v) for (v,) in connection.execute("SELECT value FROM state WHERE owner='local-device' AND kind='shortcut-recovery-v1'"))
            assert connection.execute("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name='reject_shortcut'").fetchone()[0] == 0
    assert len(backups) == 1 and backups[0]['current_raw'] == '{broken'
    report = dict(ok=True,actualWebView=True,states=18,screenshots=18 if args.screenshots else 0,
                  localKeyNativePlayback=True,recordingReloadRecovery=True,preferenceRestart=True,
                  malformedJsonRepair=True,realMusicRequests=0,physicalAudioOutputVerified=False,
                  physicalOSKeyboardVerified=False)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__': main()
