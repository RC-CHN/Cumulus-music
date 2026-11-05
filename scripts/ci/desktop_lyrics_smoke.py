"""Real main/lyric WebViews, native PCM/null audio and isolated synthetic failures."""
import argparse
import json
import os
from pathlib import Path
import sys
from desktop_lyrics_driver import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, help='optional production frontend snapshot for local comparison')
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    local = parser.add_mutually_exclusive_group()
    local.add_argument('--embedded', action='store_true', help='Frozen local FLAC in both lyric surfaces')
    local.add_argument('--native-sidecar', action='store_true', help='Open unindexed native WAV and show its nested sidecar in both lyric surfaces')
    args = parser.parse_args()
    root = args.root.resolve()
    assert not root.exists(), 'new proof root required'
    root.mkdir(parents=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.embedded:
        env['CUMULUS_EMBEDDED_LYRIC_PROOF'] = '1'
    if args.native_sidecar:
        env['CUMULUS_NATIVE_SIDECAR_LYRIC_PROOF'] = '1'
    local = args.embedded or args.native_sidecar
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    total = 0
    for phase in (['first'] if local else ['first', 'reopen']):
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0, f'lyric probe exit {code}'
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['realMusicRequests'] == 0
        assert complete['mainVisible']
        if local:
            stages = [json.loads(p.read_text(encoding='utf-8')) for p in sorted(output.glob('ux_*.json'))]
            assert [stage['surface'] for stage in stages] == ['main', 'lyric']
            assert stages[0]['timedLine'] and stages[0]['nativeClockAdvanced'] and stages[1]['paintedRow']
            assert all(stage['fromEmbedded'] == args.embedded and stage['fromSidecar'] == args.native_sidecar
                       and not stage['overflow']
                       and stage['lyricRequests'] == 0 for stage in stages)
            if args.native_sidecar:
                assert stages[0]['unindexed'] and complete['unindexed']
                if sys.platform.startswith('linux'):
                    for stage in [*stages, complete]:
                        locale = stage['locale']
                        assert locale['numeric'] == 'C'
                        assert locale['messages'] == locale['originalMessages'] not in ('C', 'POSIX')
            assert stages[0]['playId'] == stages[1]['playId']
            assert all(stage['nativePlayer']['state'] == 'paused' and
                       stage['nativePlayer']['playId'] == stage['playId'] for stage in stages)
            assert complete['stageCount'] == 2 and complete['lyricRequests'] == 0
            assert len(complete['lyricWindows']) == 1 and complete['factoryCount'] in (1, 2)
            total += len(stages)
            if args.screenshots:
                assert len(list(output.glob('ux_*.png'))) == len(stages)
            continue
        assert complete['factoryCount'] == 2
        native = json.loads((output / 'native-input.json').read_text(encoding='utf-8'))
        assert native['regions']
        assert native['keys'] == ([] if phase == 'reopen' else ['d', 'd'])
        states = [json.loads(p.read_text(encoding='utf-8')) for p in sorted(output.glob('ux_*.json'))]
        assert len(states) == complete['stageCount'] == (3 if phase == 'reopen' else 22)
        assert len(complete['checks']) == (2 if phase == 'reopen' else 17)
        assert any(r['inputSuppressed'] for r in native['regions']) == (phase == 'first')
        assert all(not s['overflow'] and s['factoryCount'] == 2 for s in states)
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == len(states)
        total += len(states)
    report = dict(ok=True, states=total, screenshots=total if args.screenshots else 0,
                  actualWebViews=2, audioFactoryCount=complete['factoryCount'], playbackEngines=1,
                  deviceInspectors=complete['factoryCount'] - 1, realMusicRequests=0,
                  physicalAudioOutputVerified=False, physicalDisplayVerified=False)
    if args.embedded:
        report.update(embeddedLyrics=True, format='FLAC', sameNativePlayback=True)
    if args.native_sidecar:
        report.update(nativeOpenSidecar=True, format='PCM WAV', sameNativePlayback=True,
                      localIndexUnchanged=True)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
