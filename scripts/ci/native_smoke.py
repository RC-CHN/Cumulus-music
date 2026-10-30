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


def read_native_report(report):
    if not report.is_file():
        progress = report.with_suffix('.progress.jsonl')
        last = None
        if progress.is_file():
            for line in progress.read_text(encoding='utf-8', errors='replace').splitlines():
                try:
                    last = json.loads(line).get('stage')
                except (json.JSONDecodeError, AttributeError):
                    pass
        issue = {'ok': False, 'stage': 'native_report', 'kind': 'missing_report', 'lastStage': last}
        report.with_suffix('.failure.json').write_text(json.dumps(issue, indent=2) + '\n', encoding='utf-8')
        log = report.with_suffix('.log')
        if log.is_file():
            print(log.read_text(encoding='utf-8', errors='replace')[-6000:])
        raise AssertionError('native process returned without its required verification report')
    return json.loads(report.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--auto', action='store_true', help='use the system audio output at zero volume')
    mode.add_argument('--filter-library', type=Path, help='verify recovered loudness PCM after GUI initialization')
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
    if args.filter_library:
        env['CUMULUS_LOUDNESS_LIBRARY'] = str(args.filter_library.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    run_native([str(args.binary.resolve())], env, report)
    proof = read_native_report(report)
    assert proof['ok'] and proof['productionDesktopStartup'] and proof['numericLocaleCompatible']
    if args.filter_library:
        assert proof['actualDecoder'] and proof['realMusicRequests'] == 0
        assert not proof['physicalAudioOutputVerified'] and not proof['businessPreferenceTested']
        assert len(proof['cases']) == 12
        assert {(c['rate'], c['channels']) for c in proof['cases']} == {(44100, 1), (48000, 2), (96000, 6)}
        for case in proof['cases']:
            assert case['maxAbsError'] <= 2e-6 and case['changed'] == case['enabled']
            assert case['stats']['processed_samples'] == case['samples']
            assert case['stats']['failed_blocks'] == 0 and case['stats']['active_instances'] == 0
        controls = proof['controls']
        assert controls['bypass']['observed_revision'] > controls['first']['observed_revision']
        assert controls['seek']['revision'] == controls['seek']['observed_revision']
        assert controls['reloaded']['processed_samples'] > 0 and controls['reloaded']['failed_blocks'] == 0
        print(json.dumps({'ok': True, 'desktopNativeLoudness': True, 'pcmCases': len(proof['cases'])}))
        return
    assert proof['metadata_probe'] and proof['remote_requests'] == 0
    assert all(proof[key] for key in ['progress', 'pause', 'seek', 'paused_seek_verified_on_resume', 'speed', 'cue_engine_eof', 'cue_end_reset', 'cancel'])
    print(json.dumps({'ok': True, 'desktopNativeMedia': True, 'output': proof['output']}))


if __name__ == '__main__':
    main()
