"""Run only the no-network standalone WebView/native search playback fixture."""
import argparse
import json
import os
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / name
        path.mkdir(mode=0o700)
        env[key] = str(path)
    run = root / 'run'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = subprocess.run([str(args.binary.resolve()), '--synthetic', str(run)],
                                env=env, stdout=log, stderr=log, timeout=120, check=False)
    report = json.loads((run / 'report.json').read_text(encoding='utf-8'))
    assert result.returncode == 0 and report['ok'], report
    assert report['synthetic'] and report['actualWebView'] and report['nullOutput']
    assert report['realAccountWrites'] == 0
    assert report['reads'] == dict(account=1, comprehensive=1, songs=1, song_detail=1, audio_source=1)
    assert report['facts'] == dict(comprehensiveSections=1, songs=1, clockAdvanced=True, stopped=True)
    print(json.dumps(report))


if __name__ == '__main__':
    main()
