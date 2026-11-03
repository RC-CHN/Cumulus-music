"""Verify the daily history shortcut in a real WebView with synthetic reads."""
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
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/daily_history_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_MOTION_PROOF='1', CUMULUS_DAILY_HISTORY_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['stageCount'] == 2
    prefix = '/api/discovery/recommend/songs/history/'
    calls = [call for call in complete['dailyHistoryCalls'] if call['path'].startswith(prefix)]
    assert calls == [
        {'path': prefix + 'recent', 'args': {}},
        {'path': prefix + 'detail', 'args': {'date': '2026-10-26'}},
    ]
    assert len(list(output.glob('ux_*.png'))) == (2 if args.screenshots else 0)
    print(json.dumps({'ok': True, 'actualWebView': True, 'historyReads': len(calls),
                      'realAccountWrites': 0}))


if __name__ == '__main__':
    main()
