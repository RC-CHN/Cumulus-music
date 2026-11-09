"""Sealed Linux WebView lyric bounty intent proof. Never contacts the service."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def run_case(binary, root, dist, enabled):
    root.mkdir()
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / key
        path.mkdir(mode=0o700)
        env[key] = str(path)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/lyric_bounty_ui_probe.js'
    script = root / 'bounty.js'
    script.write_text(f'window.__lyricBountyEnabled = {json.dumps(enabled)};\n' + source.read_text(encoding='utf-8'),
                      encoding='utf-8')
    env.update(LC_ALL='en_US.UTF-8', CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_LYRIC_CONTRIBUTION_PROOF='1', CUMULUS_UX_SCRIPT=str(script))
    if enabled:
        env['CUMULUS_LYRIC_BOUNTY_PROOF'] = '1'
    if dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(dist)
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(binary, env, log, output, True)
    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8')
    assert code == 0, code
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and len(list(output.glob('ux_*.png'))) == complete['stageCount']
    if not enabled:
        assert complete['shippedGateClosed'] and complete['stageCount'] == 1 and 'lyricBounty' not in complete
        return
    expected = ['1:6', '2:8', '2:7']
    journal = {'1:6': 'accepted', '2:8': 'unknown', '2:7': 'accepted'}
    assert complete['staleRejected'] and complete['noAutoDispatch'] and complete['stageCount'] == 10
    assert complete['lyricBounty'] == {'calls': expected, 'journal': journal}
    prefixes = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3]
    for index, path in enumerate(sorted(output.glob('ux_*.json'))):
        report = json.loads(path.read_text(encoding='utf-8'))['lyricBounty']
        calls = expected[:prefixes[index]]
        assert report == {'calls': calls, 'journal': {key: journal[key] for key in calls}}, path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    args = parser.parse_args()
    assert len(Path('/proc/net/route').read_text().splitlines()) == 1, 'requires an isolated no-route namespace'
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    dist = args.frontend_dist.resolve(strict=True) if args.frontend_dist else None
    for enabled in (False, True):
        run_case(binary, root / ('synthetic' if enabled else 'shipped'), dist, enabled)
    result = dict(ok=True, actualLinuxWebView=True, screenshots=11, shippedGateClosed=True,
                  syntheticDispatches=['1:6', '2:8', '2:7'], realRequests=0, realCloudCreditSpend=0)
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
