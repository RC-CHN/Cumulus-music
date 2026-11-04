"""Official prioritized header guides in an isolated real WebView and native settings IPC."""
import argparse
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    windows = sys.platform == 'win32'
    if windows and (not args.runtime or args.screenshots):
        parser.error('Windows requires the staged media runtime and no GTK screenshots')
    if not windows and (sys.platform != 'linux' or args.runtime):
        parser.error('Linux uses its system libmpv')
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    base = {key: value for key, value in os.environ.items()
            if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))
            and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    if windows:
        from playback_power_smoke import windows_runtime

        runtime, _ = windows_runtime(binary, args.runtime)
        base['MPV_DLL_DIR_CUMULUS'] = str(runtime)
    if args.frontend_dist:
        base['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    source = (Path(__file__).resolve().parents[2] /
              'desktop/examples/playback_power_guide_ui_probe.js').read_text(encoding='utf-8')
    verified = []
    # Both CTAs mount SettingsPage, including its separate playback settings read.
    # phase, profile, fixture, screenshots, seen markers, renderer reads
    phases = [
        ('first', 'seen', 'offer', 3, (True, False), (3, 2)),
        ('reopen', 'seen', 'offer', 1, (True, False), (0, 1)),
        ('close', 'close', 'offer', 2, (True, False), (1, 1)),
        ('smtc', 'smtc', 'smtc', 3, (False, True), (1, 2)),
        ('smtc_reopen', 'smtc', 'smtc', 1, (True, True), (1, 0)),
        ('smtc_close', 'smtc_close', 'smtc_close', 3, (False, True), (0, 1)),
        ('smtc_legacy', 'smtc_legacy', 'smtc_legacy', 1, (True, False), (1, 1)),
        ('smtc_unavailable', 'smtc_unavailable', 'smtc_unavailable', 1, (True, False), (1, 1)),
        ('enabled', 'enabled', 'enabled', 1, (False, False), (1, 1)),
        ('control', 'control', 'control', 1, (False, False), (1, 1)),
    ]
    saved_system_media = None
    for phase, profile, assignment, count, seen, renderer_reads in phases:
        state = root / profile
        state.mkdir(exist_ok=True)
        env = {**base, 'CUMULUS_PROOF_DIR': str(state), 'CUMULUS_MOTION_PROOF': '1',
               'CUMULUS_PLAYBACK_POWER_GUIDE_PROOF': assignment}
        for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                          ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                          ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local'), ('TEMP', 'temp')]:
            directory = state / 'os-profile' / name
            directory.mkdir(parents=True, exist_ok=True, mode=0o700)
            env[key] = str(directory)
        if windows:
            env['TMP'] = env['TEMP']
        script = state / f'{phase}.js'
        script.write_text(f'window.__cumulusPowerGuidePhase = {json.dumps(phase)};\n' + source,
                          encoding='utf-8')
        env['CUMULUS_UX_SCRIPT'] = str(script)
        reopen = phase in ('reopen', 'smtc_reopen')
        if reopen:
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = state / ('reopen' if reopen else 'first')
        with (state / f'{phase}.log').open('x', encoding='utf-8') as log:
            status = launch(binary, env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert status == 0, f'guide phase {phase} exited {status}'
        report = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert report['ok'] and report['stageCount'] == count and report['writes'] == 0
        assert (report['marker'] == '1', report['smtcMarker'] == '1') == seen, report
        observed_reads = (report['powerReads'], report['mediaReads'])
        assert observed_reads == renderer_reads, (phase, observed_reads, renderer_reads)
        native = report['playbackPowerGuideFixture']
        assert native['assignmentReads'] == 1 and native['powerKeeps'] == 0, report
        assert native['savedPower'] is (True if phase == 'enabled' else None), report
        if phase == 'smtc':
            saved_system_media = native['savedSystemMedia']
            assert saved_system_media['enabled'] is False and saved_system_media['revision'] != 'initial'
        elif phase == 'smtc_reopen':
            assert native['savedSystemMedia'] == saved_system_media, report
        else:
            assert native['savedSystemMedia'] is None, report
        assert len(list(output.glob('ux_*.json'))) == count
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == count
        verified.append(phase)
    result = dict(ok=True, actualWebView=True, phases=verified, realAccountWrites=0,
                  stageCount=sum(p[3] for p in phases))
    (root / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n',
                                      encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
