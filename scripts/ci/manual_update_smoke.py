"""Real WebView update check with synthetic HTTP and a closed browser launcher."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--probe-script', type=Path, help='optional local proof script')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(mode=0o700)
    output = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'APPDATA', 'LOCALAPPDATA'):
        directory = root / key
        directory.mkdir()
        env[key] = str(directory)
    if os.name != 'nt':
        stub = root / 'bin'
        stub.mkdir()
        browser = stub / 'xdg-open'
        browser.write_text(
            '#!/bin/sh\nprintf "%s\\n" "$#" "$1" >> "$CUMULUS_PROOF_DIR/browser.log"\n'
            'if test -f "$CUMULUS_PROOF_DIR/browser-fail"; then exit 2; fi\n',
            encoding='utf-8',
        )
        browser.chmod(0o700)
        env['PATH'] = str(stub) + os.pathsep + env.get('PATH', '')
    env['CUMULUS_PROOF_DIR'] = str(output)
    if args.probe_script:
        env['CUMULUS_MANUAL_UPDATE_PROOF_SCRIPT'] = str(args.probe_script.resolve())

    capture_root = None
    if args.screenshots:
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
        capture_root = Gdk.get_default_root_window()
        assert capture_root is not None, 'no GTK display'
    captured = set()
    stages = ('settings', 'current', 'network_error', 'available', 'absent')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 65
            while process.poll() is None:
                if time.monotonic() >= deadline:
                    raise TimeoutError('manual update WebView did not finish')
                if capture_root is not None:
                    for stage in stages:
                        if stage in captured or not (output / f'{stage}.json').is_file():
                            continue
                        picture = Gdk.pixbuf_get_from_window(
                            capture_root, 0, 0, capture_root.get_width(), capture_root.get_height())
                        assert picture is not None
                        picture.savev(str(output / f'{stage}.png'), 'png', [], [])
                        captured.add(stage)
                time.sleep(.06)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()

    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert process.returncode == 0, (root / 'host.log').read_text(encoding='utf-8')

    def stage(name):
        return json.loads((output / f'{name}.json').read_text(encoding='utf-8'))

    assert [stage(name)['metadataReads'] for name in stages] == [0, 1, 2, 3, 4]
    complete = stage('complete')
    assert complete['ok'] and not complete['realNetwork'] and complete['metadataReads'] == 6
    assert not args.screenshots or len(captured) == len(stages)
    page = 'https://github.com/RC-CHN/Cumulus-music/releases/tag/v999.1.0'
    assert (output / 'browser.log').read_text(encoding='utf-8').splitlines() == ['1', page, '1', page]
    report = dict(ok=True, actualWebView=True, screenshots=len(captured), metadataReads=6,
                  realNetwork=False, browserHandoffConfirmed=True,
                  nativeLauncherExercised=os.name != 'nt')
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
