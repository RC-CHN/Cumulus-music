"""Real WebView index-only group removal on generated, isolated local media."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch


def run(binary, root, screenshots):
    root.mkdir(parents=True, exist_ok=False)
    run_root = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    if ffmpeg := os.environ.get('CUMULUS_LOCAL_FIXTURE_FFMPEG'):
        env['CUMULUS_LOCAL_FIXTURE_FFMPEG'] = ffmpeg
    env.update(CUMULUS_PROOF_DIR=str(run_root), CUMULUS_LOCAL_GROUP_HIDE_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'LOCALAPPDATA', 'APPDATA', 'TEMP', 'TMP'):
        profile = root / 'os-profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(binary, env, log, run_root, screenshots)
    failed = run_root / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    completed = json.loads((run_root / 'complete.json').read_text(encoding='utf-8'))
    assert completed['ok'] and completed['counts'] == [7, 5, 7]
    assert completed['stablePausedDecoder'] and completed['similarPrefixNeighborRetained']
    assert completed['realMusicRequests'] == 0
    media = run_root / 'media'
    expected = hashlib.sha256((media / '雨声.wav').read_bytes()).digest()
    for file in ('Bonus/Bonus Track.wav', 'Bonus/Deep/Nested audible.wav',
                 'Bonus plus/Neighbor audible.wav'):
        assert hashlib.sha256((media / file).read_bytes()).digest() == expected
    stages = {path.stem for path in run_root.glob('ux_*.png')}
    expected_stages = {'ux_01_group_hide_menu', 'ux_02_group_hidden', 'ux_03_group_restored'}
    assert stages == (expected_stages if screenshots else set())
    report = {'ok': True, 'actualWebView': True, 'nativeNullOutput': True,
              'counts': completed['counts'], 'sourceFilesIntact': 3,
              'stablePausedDecoder': True, 'realMusicRequests': 0, 'screenshots': len(stages)}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if args.screenshots and sys.platform != 'linux':
        parser.error('--screenshots uses the Linux X11 capture helper')
    run(args.binary.resolve(), args.root.resolve(), args.screenshots)
