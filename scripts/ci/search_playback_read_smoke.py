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
    parser.add_argument('--runtime', type=Path, help='directory containing packaged native media libraries')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--download', action='store_true', help='verify one synthetic download and local playback')
    mode.add_argument('--search-experiment', choices=('v3', 'control'),
                      help='verify a synthetic search experiment assignment and playback')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    binary = args.binary.resolve(strict=True)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    if args.runtime:
        runtime = args.runtime.resolve(strict=True)
        assert runtime.is_dir(), runtime
        if os.name == 'nt':
            names = ('libmpv-2.dll', 'mpv-2.dll')
            assert any((runtime / name).is_file() for name in names), 'requested runtime has no libmpv DLL'
            # The trusted SDK loader prefers siblings and never uses PATH.
            # Fail instead of silently testing an SDK outside the requested stage.
            assert binary.parent == runtime or not any(
                (binary.parent / name).is_file() for name in names
            ), 'a libmpv DLL beside the probe would override the requested runtime'
        env['MPV_DLL_DIR_CUMULUS'] = str(runtime)
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / name
        path.mkdir(mode=0o700)
        env[key] = str(path)
    run = root / 'run'
    mode = ('--synthetic-download' if args.download else
            f'--synthetic-search-{args.search_experiment}' if args.search_experiment else '--synthetic')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = subprocess.run([str(binary), mode, str(run)],
                                env=env, stdout=log, stderr=log, timeout=120, check=False)
    report = json.loads((run / 'report.json').read_text(encoding='utf-8'))
    assert result.returncode == 0 and report['ok'], report
    assert report['synthetic'] and report['actualWebView'] and report['nullOutput']
    assert report['realAccountWrites'] == 0
    reads = dict(account=1, comprehensive=1, songs=1, song_detail=1, audio_source=1)
    facts = dict(comprehensiveSections=1, songs=1, clockAdvanced=True, stopped=True)
    if args.search_experiment:
        reads['search_assignment'] = 1
        facts['searchExperiment'] = args.search_experiment
        if args.search_experiment == 'v3':
            del reads['comprehensive']
            reads.update(search_tabs=1, comprehensive_v3=1)
    if args.download:
        reads.update(song_detail=2, download_source=1)
        seed = Path(__file__).resolve().parents[2]/'core/tests/fixtures/tag_audio/seed.flac'
        facts['download'] = dict(publishedBytes=seed.stat().st_size, plain=True,
                                 receiptIntegrityVerified=True, publishedLedgerVerified=True,
                                 transportCutoffConfirmed=True, offlineNativeProgress=True, stopped=True)
        files = list((run/'downloaded').glob('*.flac'))
        assert len(files) == 1 and files[0].read_bytes() == seed.read_bytes()
    assert report['reads'] == reads
    assert report['facts'] == facts
    print(json.dumps(report))


if __name__ == '__main__':
    main()
