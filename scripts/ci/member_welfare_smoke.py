"""Exercise the native read-only welfare entry with closed account/assignment fixtures."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/member_welfare_ui_probe.js'
    script = source.read_text(encoding='utf-8')
    shared = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    shared['CUMULUS_MOTION_PROOF'] = '1'
    if args.runtime:
        shared['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + shared.get('PATH', '')
    if args.frontend_dist:
        shared['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    counts = {'assigned': 5, 'reopen': 1, 'guest': 1, 'control': 1}
    traces = {
        'assigned': [['assignment', 2], *([['list', 2]] * 4), ['assignment', 3], ['list', 3]],
        'reopen': [['assignment', 2], ['list', 2]],
        'guest': [],
        'control': [['assignment', 2]],
    }
    receipts = {}
    dismissed = []
    for mode, count in counts.items():
        state = root / ('assigned' if mode == 'reopen' else mode)
        state.mkdir(exist_ok=True)
        fixture = root / f'{mode}.js'
        fixture.write_text(f'window.__memberWelfareMode = {json.dumps(mode)};\n' + script,
                           encoding='utf-8')
        env = {**shared, 'CUMULUS_PROOF_DIR': str(state), 'CUMULUS_UX_SCRIPT': str(fixture),
               'CUMULUS_MEMBER_WELFARE_PROOF': mode}
        if mode == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        for variable in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME',
                         'XDG_RUNTIME_DIR', 'APPDATA', 'LOCALAPPDATA'):
            directory = state / 'os-profile' / variable
            directory.mkdir(parents=True, mode=0o700, exist_ok=True)
            env[variable] = str(directory)
        output = state / ('reopen' if mode == 'reopen' else 'first')
        log_path = root / f'{mode}.log'
        try:
            with log_path.open('x', encoding='utf-8') as log:
                status = launch(args.binary.resolve(strict=True), env, log, output,
                                args.screenshots, acknowledge_receipts=True)
        except (TimeoutError, subprocess.TimeoutExpired) as exc:
            stages = sorted(path.name for path in output.glob('*.json'))
            tail = log_path.read_text(encoding='utf-8', errors='replace')[-2048:]
            raise AssertionError(f'welfare mode {mode} did not exit; stages: {stages}; log: {tail}') from exc
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert status == 0, (mode, status)
        receipt = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert receipt['ok'] and receipt['mode'] == mode
        assert receipt['realMusicRequests'] == receipt['remoteAccountWrites'] == 0
        assert receipt['accountDialogDismissed'] == (mode in ('guest', 'control')), receipt
        if receipt['accountDialogDismissed']:
            dismissed.append(mode)
        player = receipt['nativePlayer']
        assert player['state'] == 'idle' and player['queueLen'] == 0, receipt
        assert player['playId'] is None and player['track'] is None, receipt
        assert receipt['memberWelfareFixture'] == {
            'trace': traces[mode], 'oldReplyReturned': mode == 'assigned',
        }, receipt
        assert len(list(output.glob('ux_*.json'))) == count
        assert len(list(output.glob('ux_*.png'))) == (count if args.screenshots else 0)
        receipts[mode] = receipt['memberWelfareFixture']
    report = {'ok': True, 'actualWebView': True, 'states': sum(counts.values()),
              'screenshots': sum(counts.values()) if args.screenshots else 0,
              'normalExitModes': list(receipts), 'accountDialogsDismissed': dismissed,
              'nativePlayerRemainedIdle': True,
              'realMusicRequests': 0, 'remoteAccountWrites': 0, 'synthetic': receipts}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
