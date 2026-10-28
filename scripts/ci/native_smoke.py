"""Real desktop startup and native media, without accounts or remote requests."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def run_native(command, env, report, timeout=60):
    log_path = report.with_suffix('.log')
    try:
        with log_path.open('x', encoding='utf-8') as log:
            subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT,
                           check=True, timeout=timeout)
    except (subprocess.TimeoutExpired, subprocess.CalledProcessError, OSError) as error:
        issue = {'ok': False, 'stage': 'native_process', 'timeoutSeconds': timeout}
        if isinstance(error, subprocess.TimeoutExpired):
            issue['kind'] = 'timeout'
        elif isinstance(error, subprocess.CalledProcessError):
            issue.update(kind='exit', returnCode=error.returncode)
        else:
            issue.update(kind='spawn', errno=error.errno)
        report.with_suffix('.failure.json').write_text(json.dumps(issue, indent=2) + '\n', encoding='utf-8')
        if log_path.exists():
            print(log_path.read_text(encoding='utf-8', errors='replace')[-6000:])
        raise


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
    env['CUMULUS_PROOF_WEBVIEW_DIR'] = str(profile / 'webview')
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
    run_native([str(args.binary.resolve())], env, report)
    proof = json.loads(report.read_text(encoding='utf-8'))
    assert proof['ok'] and proof['productionDesktopStartup'] and proof['numericLocaleCompatible']
    assert proof['metadata_probe'] and proof['remote_requests'] == 0
    assert all(proof[key] for key in ['progress', 'pause', 'seek', 'paused_seek_verified_on_resume', 'speed', 'cue_engine_eof', 'cue_end_reset', 'cancel'])
    print(json.dumps({'ok': True, 'desktopNativeMedia': True, 'output': proof['output']}))


if __name__ == '__main__':
    main()
