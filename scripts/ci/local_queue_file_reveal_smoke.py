"""Verify real Linux queue actions against a private FileManager1 service."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

from local_folder_reveal_smoke import fake_file_manager
from window_manager import launch


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def verify(binary, root, screenshots):
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_LOCAL_QUEUE_REVEAL_PROOF='1',
               LC_ALL='en_US.UTF-8')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / 'os-profile' / key
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    fixture_bin = root / 'fake-bin'
    fixture_bin.mkdir()
    fake_open = fixture_bin / 'xdg-open'
    fake_open.write_text(
        f'#!{sys.executable}\nfrom pathlib import Path\nPath({str(root / "unexpected-fallback")!r}).touch()\n',
        encoding='utf-8',
    )
    fake_open.chmod(0o700)
    env['PATH'] = str(fixture_bin) + os.pathsep + env.get('PATH', '')
    with fake_file_manager(root, env, exact_successes=2):
        with (root / 'host.log').open('x', encoding='utf-8') as log:
            code = launch(binary, env, log, run, screenshots)
        failed = run / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0
    complete = read(run / 'complete.json')
    assert complete['ok'] and complete['nativeCUE'] and complete['actualNcmSelection']
    assert complete['focusRestored'] and complete['samePlay']
    assert complete['negative'] == ['snapshot_changed', 'stale', 'invalid_input']
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    calls = [json.loads(line) for line in (root / 'show-items.jsonl').read_text(encoding='utf-8').splitlines()]
    media = run / 'media'
    expected = [media / 'album.wav', media / '归途.ncm']
    assert calls == [[[target.resolve(strict=True).as_uri()], ''] for target in expected]
    assert (media / '雨声.wav').is_file(), 'queue removal cannot delete the underlying file'
    assert not (root / 'unexpected-fallback').exists()
    pngs = {p.stem for p in run.glob('ux_*.png')}
    assert pngs == ({'ux_01_queue_local_cue', 'ux_02_queue_local_ncm', 'ux_03_queue_local_changed'}
                    if screenshots else set())
    locale = read(run / 'native-startup.json')
    assert locale['numericLocale'] == 'C' and locale['messageLocale'] == 'en_US.UTF-8'
    report = dict(actualWebView=True, exactFileManager1Calls=2, screenshots=len(pngs), **complete)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--private-child', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    assert sys.platform == 'linux'
    if args.private_child:
        verify(args.binary.resolve(strict=True), args.root.resolve(), args.screenshots)
    else:
        # Own the fixture's FileManager1 name on a fresh bus, never the desktop bus.
        env = {key: value for key, value in os.environ.items()
               if key not in ('DBUS_SESSION_BUS_ADDRESS', 'AT_SPI_BUS_ADDRESS', 'AT_SPI_DISPLAY')}
        subprocess.run(['dbus-run-session', '--', sys.executable, __file__, '--private-child',
                        '--binary', str(args.binary.resolve()), '--root', str(args.root.resolve()),
                        *(['--screenshots'] if args.screenshots else [])],
                       env=env, check=True, timeout=160)


if __name__ == '__main__':
    main()
