"""Verify production Tauri homepage in a real WebView with synthetic responses."""
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
    parser.add_argument('--screenshots', action='store_true', help='capture Linux GTK pixels')
    parser.add_argument('--frontend-dist', type=Path, help='local production snapshot for comparison')
    parser.add_argument('--probe-script', type=Path, help='local synthetic WebView script for comparison')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(mode=0o700)
    capture_root = None
    if args.screenshots:
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
        capture_root = Gdk.get_default_root_window()
        assert capture_root is not None, 'no GTK display'

    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'APPDATA', 'LOCALAPPDATA'):
        directory = root / key
        directory.mkdir()
        env[key] = str(directory)
    output = root / 'run'
    env['CUMULUS_PROOF_DIR'] = str(output)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    if args.probe_script:
        env['CUMULUS_DISCOVERY_HOME_PROOF_SCRIPT'] = str(args.probe_script.resolve())
    captured = set()
    stages = ('home', 'dark', 'seeded_style', 'ranking', 'retry', 'privacy')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 75
            while process.poll() is None:
                if time.monotonic() >= deadline:
                    raise TimeoutError('discovery homepage WebView did not finish')
                if capture_root is not None:
                    for stage in stages:
                        if stage in captured or not (output / f'{stage}.json').is_file():
                            continue
                        picture = Gdk.pixbuf_get_from_window(
                            capture_root, 0, 0, capture_root.get_width(), capture_root.get_height())
                        assert picture is not None, 'no GTK frame'
                        picture.savev(str(output / f'{stage}.png'), 'png', [], [])
                        captured.add(stage)
                time.sleep(.08)
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()

    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert process.returncode == 0, (root / 'host.log').read_text(encoding='utf-8')
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and not complete['realAccount']
    assert all((output / f'{stage}.json').is_file() for stage in stages)
    assert not args.screenshots or len(captured) == len(stages)
    style = json.loads((output / 'seeded_style.json').read_text(encoding='utf-8'))
    assert style['paths'].count('/api/homepage/category/daily/song/list') == 1
    retry = json.loads((output / 'retry.json').read_text(encoding='utf-8'))
    privacy = json.loads((output / 'privacy.json').read_text(encoding='utf-8'))
    paths = [path for path in privacy['paths'][len(retry['paths']):]
             if path in ('/api/user/setting', '/api/batch', '/api/pc/recent/listen/list',
                         '/api/pc/daily/rcmd/block')]
    assert paths.count('/api/batch') == 1 and paths.count('/api/user/setting') == 2, paths
    assert paths[-1] == '/api/user/setting', paths
    report = dict(ok=True, actualWebView=True, screenshots=len(captured), realAccount=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
