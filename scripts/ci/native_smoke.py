"""Real desktop startup and native media, without accounts or remote requests."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--auto', action='store_true', help='use the system audio output at zero volume')
    args = parser.parse_args()
    report = args.report.resolve()
    assert not report.exists(), 'proof report must be new'
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_REPORT'] = str(report)
    profile = report.with_name(report.stem + '-profile')
    profile.mkdir(exist_ok=False)
    for variable, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                                ('XDG_CACHE_HOME', 'cache'), ('APPDATA', 'roaming'),
                                ('LOCALAPPDATA', 'local')]:
        path = profile / directory
        path.mkdir()
        env[variable] = str(path)
    if args.auto:
        env['CUMULUS_PROOF_AUTO'] = '1'
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    subprocess.run([str(args.binary.resolve())], env=env, check=True, timeout=60)
    proof = json.loads(report.read_text(encoding='utf-8'))
    assert proof['ok'] and proof['productionDesktopStartup'] and proof['numericLocaleCompatible']
    assert proof['metadata_probe'] and proof['remote_requests'] == 0
    assert all(proof[key] for key in ['progress', 'pause', 'seek', 'speed', 'cue_engine_eof', 'cue_end_reset', 'cancel'])
    print(json.dumps({'ok': True, 'desktopNativeMedia': True, 'output': proof['output']}))


if __name__ == '__main__':
    main()
