"""Two actual WebViews, shared native playback and bounded mini-player failures."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--shortcuts', action='store_true', help='local DOM and independent OS keys')
    args = parser.parse_args()
    root = args.root.resolve()
    assert not root.exists(), 'new proof root required'
    root.mkdir(parents=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root)
    runner = launch
    if args.shortcuts:
        from mini_shortcut_driver import launch as shortcut_launch
        runner = shortcut_launch
        env['CUMULUS_MINI_SHORTCUT_PROOF'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    with (root / 'first.log').open('x') as log:
        code = runner(args.binary.resolve(), env, log, output, args.screenshots)
    assert not (output / 'failed.json').exists(), (output / 'failed.json').read_text() if (output / 'failed.json').exists() else ''
    assert code == 0, f'mini probe exit {code}'
    complete = json.loads((output / 'complete.json').read_text())
    assert complete['ok'] and complete['realMusicRequests'] == 0
    if args.shortcuts:
        states = [json.loads(p.read_text()) for p in sorted(output.glob('ux_*.json'))]
        assert len(states) == complete['stageCount'] == 6
        assert len(complete['checks']) == 6
        assert complete['localDomKeys'] and complete['independentGlobalKeys']
        assert all(v['factoryCount'] == 1 and not v['mainVisible'] and len(v['miniWindows']) == 1 for v in states)
        assert all(not v['overflow'] and v['width'] == 420 and v['height'] == 206 for v in states)
        assert states[0]['theme'] == 'light' and all(v['theme'] == 'dark' for v in states[2:])
        assert states[0]['nativePlayer']['state'] == states[4]['nativePlayer']['state'] == 'paused'
        assert states[0]['nativePlayer']['playId'] == states[4]['nativePlayer']['playId']
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == len(states)
        report = dict(ok=True, actualWebViews=2, states=len(states),
                      screenshots=len(states) if args.screenshots else 0,
                      sharedAudioFactoryCount=1, localDomKeys=True, independentGlobalKeys=True,
                      checks=complete['checks'], realMusicRequests=0, physicalAudioOutputVerified=False)
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report))
        return
    assert all(complete[k] for k in ['timeoutVerified', 'staleControlRejected', 'ipcIsolation',
                                    'reloadFallback', 'sharedAudio', 'closePolicy', 'accountIsolation'])
    states = [json.loads(p.read_text()) for p in sorted(output.glob('ux_*.json'))]
    assert len(states) == complete['stageCount'] == 8
    assert all(v['factoryCount'] == 1 and not v['mainVisible'] and len(v['miniWindows']) == 1 for v in states)
    assert all(not v['overflow'] and v['width'] == 420 and v['height'] in [206, 516] for v in states)
    assert states[0]['nativePlayer']['playId'] == states[1]['nativePlayer']['playId']
    assert states[0]['nativePlayer']['state'] == states[1]['nativePlayer']['state'] == 'paused'
    assert states[2]['rows'] == 30 and states[3]['rows'] == 10
    assert states[4]['theme'] == states[5]['theme'] == 'dark'
    assert states[0]['background'] != states[4]['background']
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(states)
    report = dict(ok=True, actualWebViews=2, states=len(states),
                  screenshots=len(states) if args.screenshots else 0, sharedAudioFactoryCount=1,
                  realMusicRequests=0, physicalAudioOutputVerified=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
