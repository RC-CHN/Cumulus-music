"""Exercise actual Linux lyrics UI and native browser handoff without browsing."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    tools = root / 'tools'
    tools.mkdir()
    browser = tools / 'xdg-open'
    browser.write_text(
        '#!/bin/sh\n'
        '[ "$#" -eq 1 ] || exit 90\n'
        'receipt="$0.receipt"\n'
        'first=0\n'
        '[ -e "$receipt" ] || first=1\n'
        'printf "%s\\n" "$1" >> "$receipt"\n'
        '[ "$first" -eq 0 ]\n',
        encoding='utf-8',
    )
    browser.chmod(0o700)
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))
           and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / key
        path.mkdir(mode=0o700)
        env[key] = str(path)
    proof = Path(__file__).resolve().parents[2] / 'desktop/examples/lyric_contribution_ui_probe.js'
    env.update(PATH=str(tools) + os.pathsep + env['PATH'], LC_ALL='en_US.UTF-8',
               CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_LYRIC_CONTRIBUTION_PROOF='1', CUMULUS_UX_SCRIPT=str(proof))
    if args.frontend_dist:
        dist = args.frontend_dist.resolve(strict=True)
        assert (dist / 'index.html').is_file()
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(dist)
    out = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(binary, env, log, out, True)
    failed = out / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8')
    assert code == 0, code
    complete = json.loads((out / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['staleRejected'] and complete['toolbarVerified']
    assert complete['stageCount'] == 13 and complete['wikiHidesToolbar'], complete
    assert complete['emptyLyricFont'] and complete['fontPixelDelta'] == 2, complete
    images = sorted(out.glob('ux_*.png'))
    assert len(images) == 13
    urls = browser.with_suffix('.receipt').read_text(encoding='utf-8').splitlines()
    assert urls == ['https://music.163.com/#/wiki/song?type=2&songId=' + song
                    for song in ('1', '1', '2', '6', '6', '7', '8')], urls
    report = {'ok': True, 'actualLinuxWebView': True, 'screenshots': len(images),
              'browserCalls': urls, 'firstShellFailedAndRetried': True,
              'staleNativeSelectionRejected': True, 'toolbarVerified': True, 'wikiHidesToolbar': True,
              'emptyLyricFont': True, 'fontPixelDelta': complete['fontPixelDelta'],
              'realBrowserRequests': 0,
              'realAccountWrites': 0}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
